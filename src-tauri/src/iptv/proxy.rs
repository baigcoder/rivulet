use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;

use reqwest::Client;

/// Proxy port — one above the torrent engine's 3030. Off that port to avoid
/// fighting for the bind, below the reserved dynamic range so it stays
/// predictable.
const PROXY_ADDR: &str = "127.0.0.1:3031";

/// Browser-like request headers, in case the upstream distinguishes by UA.
const BROWSER_HEADERS: &[(&str, &str)] = &[
    ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"),
    ("Accept", "*/*"),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Connection", "keep-alive"),
];

/// CORS headers added to every response. The webview's <video> element makes
/// the request without credentials, so an `*` origin is fine and avoids
/// echoing whatever the page sent.
const CORS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"),
    ("Access-Control-Allow-Headers", "Range, Content-Type"),
    (
        "Access-Control-Expose-Headers",
        "Content-Range, Content-Length, Content-Type",
    ),
    ("Access-Control-Max-Age", "86400"),
];

/// Shared reqwest client for *streaming* (HLS, Direct movies, live TS).
/// The IPTV JSON client caps a request at 300s, which kills a film at
/// five minutes and a live channel the same way. Connect-timeout only:
/// first-byte can wait on a debrid unlock; the body lasts as long as playback.
static STREAM: OnceLock<Client> = OnceLock::new();

fn stream_http() -> &'static Client {
    STREAM.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .gzip(false)
            .brotli(false)
            // Xtream / HLS panels often advertise HTTP/2 and then stall
            // or reset streams. That is a channel that takes ages to
            // start and hitches every few seconds. HTTP/1.1 is what
            // those servers actually serve, and mpv talks it too.
            .http1_only()
            .tcp_nodelay(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .expect("failed to build stream HTTP client")
    })
}

fn stream_get(
    url: &str,
    ua: Option<&str>,
    referer: Option<&str>,
    range: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = stream_http().get(url);
    if let Some(ua) = ua {
        req = req.header("User-Agent", ua);
    } else {
        for (k, v) in BROWSER_HEADERS {
            req = req.header(*k, *v);
        }
    }
    if let Some(rf) = referer {
        req = req.header("Referer", rf);
    }
    if let Some(r) = range {
        req = req.header("Range", r);
    }
    req
}

/// After the first 302, mpv still probes the *resolver* URL two or three
/// times. Remember the CDN location so those extra GETs skip the unlock.
const REDIR_TTL: Duration = Duration::from_secs(10 * 60);

struct Redirect {
    url: String,
    at: Instant,
}

static REDIRS: OnceLock<Mutex<HashMap<String, Redirect>>> = OnceLock::new();
static GATES: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();

fn redirs() -> &'static Mutex<HashMap<String, Redirect>> {
    REDIRS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_redirect(from: &str) -> Option<String> {
    let mut map = redirs().lock().unwrap_or_else(|e| e.into_inner());
    match map.get(from) {
        Some(c) if c.at.elapsed() < REDIR_TTL => Some(c.url.clone()),
        Some(_) => {
            map.remove(from);
            None
        }
        None => None,
    }
}

fn remember_redirect(from: &str, to: &str) {
    if from == to {
        return;
    }
    let mut map = redirs().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 64 {
        map.retain(|_, c| c.at.elapsed() < REDIR_TTL);
        if map.len() > 64 {
            map.clear();
        }
    }
    map.insert(
        from.to_string(),
        Redirect {
            url: to.to_string(),
            at: Instant::now(),
        },
    );
}

fn forget_redirect(from: &str) {
    redirs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(from);
}

// --- yt-dlp resolution cache ---------------------------------------------
// A YouTube trailer's <video> element issues several requests for one file —
// the initial GET, then Range seeks as it plays. Resolving the ID through
// yt-dlp each time respawns a ~40 MB self-extracting process per request, and
// the first cold run on an AppImage can take many seconds (or hang on
// YouTube's anti-bot throttling), which blocks the webview's media pipeline and
// appears as the detail page freezing. Cache the resolved direct URL per ID so
// only the first request pays the cost, and bound how long we ever wait.

const YTDLP_TTL: Duration = Duration::from_secs(60 * 60);
const YTDLP_TIMEOUT: Duration = Duration::from_secs(12);
/// One muxed file `<video src>` can play. `best` often prints a video URL and
/// an audio URL, which WebKit then hangs on — that is why Linux used to skip
/// this proxy entirely. Progressive AVC mp4 (format 18 as last resort) is
/// the shape GStreamer will actually decode.
const YTDLP_FORMAT: &str =
    "b[ext=mp4][vcodec^=avc1][height<=1080]/b[ext=mp4][height<=1080]/18";

struct YtResolved {
    url: String,
    at: Instant,
}

static YT_RESOLVED: OnceLock<Mutex<HashMap<String, YtResolved>>> = OnceLock::new();

fn yt_resolved() -> &'static Mutex<HashMap<String, YtResolved>> {
    YT_RESOLVED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_youtube(id: &str) -> Option<String> {
    let mut map = yt_resolved().lock().unwrap_or_else(|e| e.into_inner());
    match map.get(id) {
        Some(c) if c.at.elapsed() < YTDLP_TTL => Some(c.url.clone()),
        Some(_) => {
            map.remove(id);
            None
        }
        None => None,
    }
}

fn remember_youtube(id: &str, url: &str) {
    let mut map = yt_resolved().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 128 {
        map.retain(|_, c| c.at.elapsed() < YTDLP_TTL);
        if map.len() > 128 {
            map.clear();
        }
    }
    map.insert(
        id.to_string(),
        YtResolved {
            url: url.to_string(),
            at: Instant::now(),
        },
    );
}

fn url_gate(url: &str) -> Arc<AsyncMutex<()>> {
    let mut g = GATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if g.len() > 64 {
        g.clear();
    }
    g.entry(url.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

/// Tiny HTTP proxy: receives GET /stream?url=... and forwards the response
/// back. The webview's <video> element can then load streams it could not
/// load directly — HTTP from an HTTPS page, CORS-less origins, upstreams
/// that block the webview's UA — because the proxy uses the same
/// browser-shaped request an mpv binary would.
///
/// HLS is the only protocol in play; the manifest is fetched, rewritten so
/// every segment and nested playlist points back at the proxy, and returned.
/// A plain <video> then loads it like any other playlist.
///
/// `GET /health` returns 200 OK with the proxy version. The frontend polls
/// this to know the proxy is alive before navigating to the player.
pub async fn run_proxy(ytdlp: Option<PathBuf>) -> anyhow::Result<()> {
    // Remember the bundled yt-dlp path (if any) so every request's handler can
    // resolve a YouTube trailer to a direct stream without a PATH lookup.
    if let Some(p) = ytdlp {
        *ytdlp_path().lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    }

    let listener = TcpListener::bind(PROXY_ADDR).await?;
    eprintln!("[iptv-proxy] listening on {PROXY_ADDR}");

    loop {
        let (mut stream, _addr) = listener.accept().await?;
        tokio::spawn(async move {
            let _ = stream.set_nodelay(true);
            if let Err(e) = handle_connection(&mut stream).await {
                eprintln!("[iptv-proxy] connection error: {e}");
            }
        });
    }
}

/// The bundled yt-dlp binary, resolved once at proxy startup. Stored in a
/// `Mutex<Option<PathBuf>>` rather than a plain `Option` so the handlers can
/// read it concurrently while still letting `run_proxy` set it exactly once.
static YTDLP: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn ytdlp_path() -> &'static Mutex<Option<PathBuf>> {
    YTDLP.get_or_init(|| Mutex::new(None))
}

/// Spawn yt-dlp without the AppImage's library path. The bundled binary is
/// self-extracting; inheriting Ubuntu 22.04's libs from LD_LIBRARY_PATH is
/// how a trailer sat on "Starting…" until the 12s timeout.
///
/// yt-dlp.exe is a console app. Without CREATE_NO_WINDOW, every trailer
/// resolve flashes a terminal — same flag mpv/ffmpeg use on Windows.
fn ytdlp_command(bin: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(bin);
    if std::env::var_os("APPIMAGE").is_some() {
        cmd.env_remove("LD_LIBRARY_PATH");
        cmd.env_remove("APPDIR");
        cmd.env_remove("PYTHONHOME");
        cmd.env_remove("PYTHONPATH");
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.as_std_mut()
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    cmd
}

fn first_http_url(stdout: &str) -> Option<String> {
    let urls: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
        .collect();
    (urls.len() == 1).then(|| urls[0].to_string())
}

async fn handle_connection(stream: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    // Read the request. A <video> element on a live stream sends GET and
    // occasional range requests, all of which fit in a few KB. Some HLS
    // servers send large cookies in the request, though, so 32 KB is a
    // safer ceiling than the original 16.
    let mut buf = vec![0u8; 32768];
    let mut total = 0;
    loop {
        let n = stream.read(&mut buf[total..]).await?;
        if n == 0 {
            return Ok(());
        }
        total += n;
        if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if total == buf.len() {
            // Header too large — refuse rather than truncate.
            write_response(
                stream,
                431,
                "Request Header Fields Too Large",
                "text/plain",
                b"header too large",
            )
            .await?;
            return Ok(());
        }
    }
    let request = String::from_utf8_lossy(&buf[..total]).into_owned();

    // The browser preflights the first cross-origin request with OPTIONS.
    // Answer it and stop — the GET that follows will be the real thing.
    if request.starts_with("OPTIONS ") {
        write_preflight(stream).await?;
        return Ok(());
    }

    // Health check — no upstream call, just confirms the proxy is alive.
    if request.starts_with("GET /health") {
        write_response(stream, 200, "OK", "text/plain", b"OK").await?;
        return Ok(());
    }

    // YouTube embed shim. The packaged webview loads from tauri://localhost
    // on Linux/macOS, and YouTube's iframe rejects that origin (error 153).
    // Serving the embed from loopback HTTP gives YouTube a valid Referer while
    // the page around it stays on the custom protocol — same trick as
    // https://github.com/tauri-apps/tauri/issues/14422#issuecomment-2799999999
    if request.starts_with("GET /youtube-embed") {
        serve_youtube_embed(stream, &request).await?;
        return Ok(());
    }

    // YouTube direct stream — resolves a video ID via yt-dlp and proxies the
    // actual video bytes. GTK WebKit can play a direct <video> stream but not
    // a YouTube iframe embed. HEAD is the same resolve as GET: WebView2 probes
    // with HEAD before the element plays.
    if request.starts_with("GET /youtube-stream") || request.starts_with("HEAD /youtube-stream") {
        serve_youtube_stream(stream, &request).await?;
        return Ok(());
    }

    // Parse the request line and the URL parameter. Bad input gets a 400
    // rather than a 500 — the page will then fall back to the raw URL.
    let (target_url, custom_ua, custom_referer) = match parse_target(&request) {
        Some(parsed) => parsed,
        None => {
            write_response(
                stream,
                400,
                "Bad Request",
                "text/plain",
                b"missing or invalid url",
            )
            .await?;
            return Ok(());
        }
    };

    // Forward the range header if the client sent one — HLS segments and
    // mp4 files both support it, and the upstream's CDN may return a 206
    // that we pass through verbatim. A HEAD from lavf must not become a
    // full GET: that starts the movie twice and is the long Buffering wait.
    let is_head = request.starts_with("HEAD ");
    let range =
        extract_header(&request, "Range").or_else(|| is_head.then(|| "bytes=0-0".to_string()));
    eprintln!(
        "[iptv-proxy] {} {target_url}",
        if is_head { "HEAD" } else { "GET" }
    );

    let ua = custom_ua.as_deref();
    let rf = custom_referer.as_deref();
    // Hold only while following the resolver 302 — not while the movie
    // body streams, or a Range seek would wait on the first connection.
    let gate = url_gate(&target_url);
    let _resolve = gate.lock().await;
    let mut fetch_url = cached_redirect(&target_url).unwrap_or_else(|| target_url.clone());
    if fetch_url != target_url {
        eprintln!("[iptv-proxy] cached {fetch_url}");
    }

    let resp = match stream_get(&fetch_url, ua, rf, range.as_deref())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) if fetch_url != target_url => {
            eprintln!("[iptv-proxy] cached upstream failed ({e}), retrying resolver");
            forget_redirect(&target_url);
            fetch_url = target_url.clone();
            match stream_get(&fetch_url, ua, rf, range.as_deref())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[iptv-proxy] upstream error for {target_url}: {e}");
                    write_response(
                        stream,
                        502,
                        "Bad Gateway",
                        "text/plain",
                        e.to_string().as_bytes(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
        Err(e) => {
            eprintln!("[iptv-proxy] upstream error for {target_url}: {e}");
            write_response(
                stream,
                502,
                "Bad Gateway",
                "text/plain",
                e.to_string().as_bytes(),
            )
            .await?;
            return Ok(());
        }
    };

    let mut status = resp.status();
    let mut resp = resp;

    if fetch_url != target_url && !status.is_success() && status.as_u16() != 206 {
        forget_redirect(&target_url);
        fetch_url = target_url.clone();
        if let Ok(r) = stream_get(&fetch_url, ua, rf, range.as_deref())
            .send()
            .await
        {
            resp = r;
            status = resp.status();
        }
    }

    // Range Header Fallback:
    // Many IPTV servers (Xtream Codes live streams) reject `Range: bytes=...` requests
    // with 400, 416, 500, etc. If a range header was sent and upstream failed, retry WITHOUT Range header.
    if !status.is_success() && status.as_u16() != 206 && range.is_some() {
        eprintln!("[iptv-proxy] range request failed with {status}, retrying without Range header: {fetch_url}");
        if let Ok(nr_resp) = stream_get(&fetch_url, ua, rf, None).send().await {
            if nr_resp.status().is_success() || nr_resp.status().as_u16() == 206 {
                eprintln!("[iptv-proxy] request without Range succeeded for {fetch_url}");
                resp = nr_resp;
                status = resp.status();
            }
        }
    }

    // IPTV Smarters Pro Fallback:
    // If upstream returned an error (500, 502, 503, 404), try swapping .ts <-> .m3u8
    // because many Xtream servers only serve direct TS or vice versa.
    if !status.is_success() && status.as_u16() != 206 {
        let fallback_url = if target_url.contains(".m3u8") {
            Some(target_url.replace(".m3u8", ".ts"))
        } else if target_url.contains(".ts") {
            Some(target_url.replace(".ts", ".m3u8"))
        } else {
            None
        };

        if let Some(ref fb_url) = fallback_url {
            eprintln!("[iptv-proxy] upstream failed with {status}, retrying fallback: {fb_url}");
            if let Ok(fb_resp) = stream_get(fb_url, ua, rf, None).send().await {
                if fb_resp.status().is_success() || fb_resp.status().as_u16() == 206 {
                    eprintln!("[iptv-proxy] fallback succeeded: {fb_url}");
                    resp = fb_resp;
                    status = resp.status();
                }
            }
        }
    }

    remember_redirect(&target_url, resp.url().as_str());
    drop(_resolve);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // HLS manifests need their URLs rewritten so the webview fetches the
    // proxy, not the origin. The manifest itself is small (a few KB), so
    // buffering it is fine and lets us rewrite every line.
    if content_type.contains("mpegurl") || target_url.ends_with(".m3u8") {
        match resp.text().await {
            Ok(body) => {
                let rewritten = rewrite_m3u(
                    &body,
                    &fetch_url,
                    custom_ua.as_deref(),
                    custom_referer.as_deref(),
                );
                // A master playlist that only names unresolvable hosts
                // (CGTN's 2017 CloudFront file still lists live.cgtn.com)
                // would otherwise 200, then mpv would sit on Connecting
                // while every variant 502s. Fail the manifest instead so
                // the player skips in one beat.
                if playlist_has_uri(&body) && !playlist_has_uri(&rewritten) {
                    eprintln!(
                        "[iptv-proxy] no reachable streams in {fetch_url}"
                    );
                    write_response(
                        stream,
                        502,
                        "Bad Gateway",
                        "text/plain",
                        b"no reachable streams in playlist",
                    )
                    .await?;
                    return Ok(());
                }
                write_response(
                    stream,
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("OK"),
                    "application/vnd.apple.mpegurl",
                    rewritten.as_bytes(),
                )
                .await?;
            }
            Err(e) => {
                eprintln!("[iptv-proxy] manifest read error: {e}");
                write_response(
                    stream,
                    502,
                    "Bad Gateway",
                    "text/plain",
                    e.to_string().as_bytes(),
                )
                .await?;
            }
        }
        return Ok(());
    }

    relay_upstream(stream, status, &content_type, resp, is_head).await
}

/// Stream an upstream `reqwest` response through to the client in real-time.
/// HLS segments and live video chunks are never-ending streams — buffering the
/// whole body hangs forever waiting for EOF. Instead: (1) write response
/// headers (status, content-type, CORS, Content-Length if the upstream told
/// us), (2) forward each chunk as it arrives, (3) when the upstream has no
/// Content-Length use Transfer-Encoding: chunked so the browser knows where
/// each frame ends.
async fn relay_upstream(
    stream: &mut tokio::net::TcpStream,
    status: reqwest::StatusCode,
    content_type: &str,
    mut resp: reqwest::Response,
    is_head: bool,
) -> anyhow::Result<()> {
    let upstream_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let mut header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {content_type}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("OK"),
    );
    if let Some(len) = upstream_length {
        header.push_str(&format!("Content-Length: {len}\r\n"));
    } else {
        // Upstream used chunked transfer encoding (or no length at all).
        // Forward as chunked so the browser can frame each chunk.
        header.push_str("Transfer-Encoding: chunked\r\n");
    }
    for (k, v) in CORS_HEADERS {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    for h in [
        "Content-Range",
        "Accept-Ranges",
        "Cache-Control",
        "ETag",
        "Last-Modified",
    ] {
        if let Some(v) = resp.headers().get(h) {
            if let Ok(s) = v.to_str() {
                header.push_str(&format!("{h}: {s}\r\n"));
            }
        }
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).await?;
    if is_head {
        let _ = stream.shutdown().await;
        return Ok(());
    }

    // Read the response body in chunks. reqwest's `chunk()` reads up to a
    // chunk at a time and returns the decompressed bytes — perfect for
    // forwarding. The loop exits when the upstream closes the stream (for
    // HLS segments, this is a few MB; for live, it runs forever).
    //
    // We do NOT use `chunked` transfer encoding for finite responses —
    // Content-Length is already set. For infinite (live) responses we
    // wrap each frame in chunked encoding so the browser knows where each
    // chunk ends.
    let use_chunked = upstream_length.is_none();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if use_chunked {
                    // HTTP/1.1 chunked transfer encoding: each chunk is
                    // `<size in hex>\r\n<data>\r\n`, terminated by `0\r\n\r\n`.
                    let size_line = format!("{:X}\r\n", chunk.len());
                    if stream.write_all(size_line.as_bytes()).await.is_err()
                        || stream.write_all(&chunk).await.is_err()
                        || stream.write_all(b"\r\n").await.is_err()
                    {
                        // Client disconnected — stop forwarding and drop
                        // the upstream connection.
                        break;
                    }
                } else if stream.write_all(&chunk).await.is_err() {
                    break;
                }
            }
            Ok(None) => break, // Upstream closed.
            Err(_) => break,   // Upstream errored or timed out.
        }
    }
    if use_chunked {
        // Best-effort terminator; the client may already be gone.
        let _ = stream.write_all(b"0\r\n\r\n").await;
    }
    let _ = stream.shutdown().await;
    Ok(())
}

fn playlist_has_uri(body: &str) -> bool {
    body.lines().any(|line| {
        let t = line.trim();
        !t.is_empty() && !t.starts_with('#')
    })
}

fn resolve_manifest_uri(trimmed: &str, base: &str) -> String {
    let base_no_query = base.split('?').next().unwrap_or(base);
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.starts_with('/') {
        if let Ok(parsed) = url::Url::parse(base) {
            format!(
                "{}://{}{}",
                parsed.scheme(),
                parsed.host_str().unwrap_or(""),
                trimmed
            )
        } else {
            trimmed.to_string()
        }
    } else {
        let base_dir = base_no_query.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        format!("{base_dir}/{trimmed}")
    }
}

/// True when this URL's host has at least one DNS address. Relative
/// playlist lines never reach here. Negative answers live 30s so a
/// master with three variants on the same dead host is one lookup.
fn host_has_address(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return true;
    };
    let Some(host) = parsed.host_str() else {
        return true;
    };
    if host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return true;
    }
    let key = host.to_ascii_lowercase();
    static CACHE: OnceLock<Mutex<HashMap<String, (bool, Instant)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some((ok, at)) = guard.get(&key) {
            let ttl = if *ok {
                Duration::from_secs(300)
            } else {
                Duration::from_secs(30)
            };
            if at.elapsed() < ttl {
                return *ok;
            }
        }
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    let ok = format!("{host}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some();
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, (ok, Instant::now()));
    }
    ok
}

/// Rewrite every URL inside an HLS manifest so it points at the proxy. The
/// manifest is a list of relative or absolute paths, and without this the
/// <video> element fetches them from the origin, which is exactly what we
/// started the proxy to avoid.
///
/// Handles the three forms a manifest line can take:
///   - absolute: `https://cdn.example.com/seg.ts?token=abc`
///   - absolute path: `/path/to/seg.ts`
///   - relative: `seg.ts` or `subdir/seg.ts?token=abc`
/// And preserves any query string on the base URL when resolving relatives.
fn rewrite_m3u(body: &str, base: &str, user_agent: Option<&str>, referer: Option<&str>) -> String {
    rewrite_m3u_filter(body, base, user_agent, referer, host_has_address)
}

fn rewrite_m3u_filter(
    body: &str,
    base: &str,
    user_agent: Option<&str>,
    referer: Option<&str>,
    reachable: impl Fn(&str) -> bool,
) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut skip = vec![false; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let resolved = resolve_manifest_uri(trimmed, base);
        if (resolved.starts_with("http://") || resolved.starts_with("https://"))
            && !reachable(&resolved)
        {
            skip[i] = true;
            let mut j = i;
            while j > 0 {
                j -= 1;
                let prev = lines[j].trim();
                if prev.is_empty() || !prev.starts_with('#') || prev.starts_with("#EXTM3U") {
                    break;
                }
                skip[j] = true;
            }
        }
    }

    // Nested manifests often require the page that linked them as Referer.
    // Free-TV playlists rarely set EXTVLCOPT, so inherit the playlist origin.
    let inherited_referer = referer.map(str::to_string).or_else(|| {
        url::Url::parse(base).ok().and_then(|u| {
            let host = u.host_str()?;
            Some(format!("{}://{host}/", u.scheme()))
        })
    });

    let mut rewritten: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if skip[i] {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            rewritten.push((*line).to_string());
            continue;
        }
        let resolved = resolve_manifest_uri(trimmed, base);
        // An `#EXTVLCOPT:http-user-agent` or `http-referrer` on the
        // playlist applies to every request in the HLS chain, not only the
        // initial .m3u8. Without propagating it here, the manifest loads but
        // the CDN rejects each segment with 403 and the player can only say
        // "Playback failed". Keep the headers in the proxy URL so nested
        // manifests inherit them too.
        let mut uri = format!("/stream?url={}", urlencoding::encode(&resolved));
        if let Some(ua) = user_agent {
            uri.push_str("&X-Rivulet-Ua=");
            uri.push_str(&urlencoding::encode(ua));
        }
        if let Some(rf) = inherited_referer.as_deref() {
            uri.push_str("&X-Rivulet-Referer=");
            uri.push_str(&urlencoding::encode(rf));
        }
        rewritten.push(uri);
    }
    // ffmpeg/mpv pick the *first* EXT-X-STREAM-INF. IPTV masters list 720p
    // first so a cheap client can start; a 4K channel then looks like 720p.
    // Highest RESOLUTION / BANDWIDTH first is what TiviMate does.
    sort_hls_master(&mut rewritten);
    let mut out = String::with_capacity(body.len());
    for line in rewritten {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn hls_attr_u64(tag: &str, key: &str) -> Option<u64> {
    let needle = format!("{key}=");
    let rest = tag.split(&needle).nth(1)?;
    let token = rest.split([',', ' ', '\t']).next()?.trim();
    token.parse().ok()
}

fn hls_resolution_height(tag: &str) -> u64 {
    let rest = match tag.split("RESOLUTION=").nth(1) {
        Some(s) => s,
        None => return 0,
    };
    let token = rest.split([',', ' ', '\t']).next().unwrap_or("").trim();
    token
        .split('x')
        .nth(1)
        .and_then(|h| h.parse().ok())
        .unwrap_or(0)
}

fn hls_variant_rank(stream_inf: &str) -> u64 {
    let height = hls_resolution_height(stream_inf);
    let bw = hls_attr_u64(stream_inf, "AVERAGE-BANDWIDTH")
        .or_else(|| hls_attr_u64(stream_inf, "BANDWIDTH"))
        .unwrap_or(0);
    height.saturating_mul(1_000_000_000) + bw
}

/// Put the highest HLS rendition first. Leaves media playlists (no
/// EXT-X-STREAM-INF) and the header tags above the first variant alone.
fn sort_hls_master(lines: &mut Vec<String>) {
    let Some(first) = lines
        .iter()
        .position(|l| l.trim().starts_with("#EXT-X-STREAM-INF"))
    else {
        return;
    };
    let header = lines[..first].to_vec();
    let mut variants: Vec<(u64, Vec<String>)> = Vec::new();
    let mut i = first;
    while i < lines.len() {
        if !lines[i].trim().starts_with("#EXT-X-STREAM-INF") {
            break;
        }
        let rank = hls_variant_rank(&lines[i]);
        let mut block = vec![lines[i].clone()];
        i += 1;
        while i < lines.len() && lines[i].trim().starts_with('#') {
            block.push(lines[i].clone());
            i += 1;
        }
        if i < lines.len() {
            block.push(lines[i].clone());
            i += 1;
        }
        variants.push((rank, block));
    }
    variants.sort_by(|a, b| b.0.cmp(&a.0));
    let mut ordered = header;
    for (_, block) in variants {
        ordered.extend(block);
    }
    ordered.extend_from_slice(&lines[i..]);
    *lines = ordered;
}

/// YouTube video ids are always 11 characters from this alphabet.
fn valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_youtube_embed(request: &str) -> Option<(String, bool, bool, bool, bool)> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    let mut id: Option<String> = None;
    let mut autoplay = true;
    let mut mute = false;
    let mut looping = false;
    let mut controls = true;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        let decoded = urlencoding::decode(v).ok()?.into_owned();
        match k {
            "v" => id = Some(decoded),
            "autoplay" => autoplay = decoded != "0",
            "mute" => mute = decoded == "1",
            "loop" => looping = decoded == "1",
            "controls" => controls = decoded != "0",
            _ => {}
        }
    }
    let id = id?;
    if !valid_youtube_id(&id) {
        return None;
    }
    Some((id, autoplay, mute, looping, controls))
}

async fn serve_youtube_embed(
    stream: &mut tokio::net::TcpStream,
    request: &str,
) -> anyhow::Result<()> {
    let (id, autoplay, mute, looping, controls) = match parse_youtube_embed(request) {
        Some(v) => v,
        None => {
            write_response(stream, 400, "Bad Request", "text/plain", b"invalid v").await?;
            return Ok(());
        }
    };
    let mut params = String::from(
        "rel=0&playsinline=1&enablejsapi=1&vq=hd1080&origin=http%3A%2F%2F127.0.0.1%3A3031",
    );
    if autoplay {
        params.push_str("&autoplay=1");
    }
    if mute {
        params.push_str("&mute=1");
    }
    // A one-id playlist is how YouTube honours loop, and it paints previous /
    // next on the cover. The hero loops from ended → play instead.
    if looping && controls {
        params.push_str("&loop=1&playlist=");
        params.push_str(&id);
    }
    // Cover hero: mute/unmute is ours. Hide YouTube's title, play, and FS.
    if !controls {
        params.push_str(
            "&controls=0&modestbranding=1&fs=0&disablekb=1&iv_load_policy=3&cc_load_policy=0",
        );
    }
    let loop_ready = if looping && controls {
        r#"send("setLoop",[true]);"#
    } else {
        ""
    };
    let loop_ended = if looping {
        r#"if(d&&d.info&&d.info.playerState===0)send("seekTo",[0,true]);if(d&&d.info&&d.info.playerState===0)send("playVideo");"#
    } else {
        ""
    };
    // Forwards mute/unMute/quality from the page, and player state back up, so
    // the volume button does not reload the iframe and the hero can hide YouTube's
    // spinner until the trailer is actually playing.
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>*{{margin:0;padding:0;box-sizing:border-box}}html,body{{width:100%;height:100%;overflow:hidden;background:#000}}iframe{{width:100%;height:100%;border:none}}</style></head>
<body><iframe src="https://www.youtube.com/embed/{id}?{params}" allow="autoplay; encrypted-media; gyroscope; picture-in-picture; web-share" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>
<script>
(function(){{
  var f=document.querySelector("iframe");
  var yt="https://www.youtube.com";
  function send(func,args){{
    if(f.contentWindow)f.contentWindow.postMessage(JSON.stringify({{event:"command",func:func,args:args||[]}}),yt);
  }}
  function lock(){{
    send("setPlaybackQuality",["hd1080"]);
    send("setPlaybackQualityRange",["hd1080","hd1080"]);
  }}
  addEventListener("message",function(e){{
    if(!f.contentWindow)return;
    if(e.source===parent){{
      f.contentWindow.postMessage(typeof e.data==="string"?e.data:JSON.stringify(e.data),yt);
      return;
    }}
    if(e.source!==f.contentWindow)return;
    parent.postMessage(typeof e.data==="string"?e.data:JSON.stringify(e.data),"*");
    var d=e.data;if(typeof d==="string"){{try{{d=JSON.parse(d)}}catch(x){{return}}}}
    if(d&&d.event==="onReady"){{lock();{loop_ready}send("playVideo");}}
    if(d&&d.info&&d.info.playerState===1)lock();
    {loop_ended}
  }});
  f.addEventListener("load",function(){{
    f.contentWindow.postMessage(JSON.stringify({{event:"listening"}}),yt);
    lock();
    {loop_ready}
    send("playVideo");
  }});
}})();
</script></body></html>"#
    );
    write_response(
        stream,
        200,
        "OK",
        "text/html; charset=utf-8",
        html.as_bytes(),
    )
    .await
}

async fn serve_youtube_stream(
    stream: &mut tokio::net::TcpStream,
    request: &str,
) -> anyhow::Result<()> {
    let id = match parse_youtube_embed(request) {
        Some((id, _, _, _, _)) => id,
        None => {
            write_response(stream, 400, "Bad Request", "text/plain", b"invalid v").await?;
            return Ok(());
        }
    };

    let is_head = request.starts_with("HEAD ");
    let range =
        extract_header(request, "Range").or_else(|| is_head.then(|| "bytes=0-0".to_string()));

    // Use yt-dlp to resolve the direct video stream URL at up to 1080p. The
    // result is cached per ID so a <video>'s repeated Range requests don't each
    // respawn yt-dlp, and the whole resolution is bounded by a timeout so a
    // hung yt-dlp (AppImage cold run, YouTube throttling) returns a fast 502
    // instead of blocking the media pipeline free.
    if let Some(resolved) = cached_youtube(&id) {
        eprintln!("[iptv-proxy] cached youtube-stream {id} → {resolved}");
        return stream_resolved_youtube(stream, &id, &resolved, range.as_deref(), is_head).await;
    }

    let url = format!("https://www.youtube.com/watch?v={id}");
    let ytdlp = ytdlp_path()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| PathBuf::from("yt-dlp"));
    let cmd = ytdlp_command(&ytdlp)
        .arg("--get-url")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--format")
        .arg(YTDLP_FORMAT)
        .arg(&url)
        .output();

    let output = match tokio::time::timeout(YTDLP_TIMEOUT, cmd).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            eprintln!("[iptv-proxy] yt-dlp not found: {e}");
            write_response(
                stream,
                502,
                "Bad Gateway",
                "text/plain",
                b"yt-dlp not installed",
            )
            .await?;
            return Ok(());
        }
        Err(_) => {
            eprintln!("[iptv-proxy] yt-dlp timed out after {YTDLP_TIMEOUT:?} for {id}");
            write_response(
                stream,
                502,
                "Bad Gateway",
                "text/plain",
                b"yt-dlp timed out",
            )
            .await?;
            return Ok(());
        }
    };

    let resolved = match output {
        o if o.status.success() => {
            let resolved = match first_http_url(&String::from_utf8_lossy(&o.stdout)) {
                Some(url) => url,
                None => {
                    write_response(
                        stream,
                        502,
                        "Bad Gateway",
                        "text/plain",
                        b"yt-dlp returned no single URL",
                    )
                    .await?;
                    return Ok(());
                }
            };
            eprintln!("[iptv-proxy] yt-dlp resolved youtube-stream {id} → {resolved}");
            remember_youtube(&id, &resolved);
            resolved
        }
        o => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("[iptv-proxy] yt-dlp failed for {id}: {stderr}");
            write_response(stream, 502, "Bad Gateway", "text/plain", b"yt-dlp failed").await?;
            return Ok(());
        }
    };

    stream_resolved_youtube(stream, &id, &resolved, range.as_deref(), is_head).await
}

/// Given an already-resolved direct stream URL, fetch its bytes and proxy them
/// to the client. Separated so the cached path and the freshly-resolved path
/// share the same upstream handling.
async fn stream_resolved_youtube(
    stream: &mut tokio::net::TcpStream,
    id: &str,
    resolved: &str,
    range: Option<&str>,
    is_head: bool,
) -> anyhow::Result<()> {
    // Now stream the resolved CDN URL's bytes back through the loopback. A 302
    // to a cross-origin googlevideo URL is fragile — the webview <video> would
    // fetch it without our CORS headers or browser UA. Proxying means the
    // element only ever talks to 127.0.0.1:3031, and we control the headers.
    // Forward the client's Range so seeks hit the right byte window. Bound the
    // resolve handshake so a stalled CDN can't block the caller indefinitely.
    let send = stream_get(resolved, None, Some("https://www.youtube.com"), range).send();
    let resp = match tokio::time::timeout(Duration::from_secs(15), send).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("[iptv-proxy] upstream error for resolved youtube stream {id}: {e}");
            write_response(
                stream,
                502,
                "Bad Gateway",
                "text/plain",
                e.to_string().as_bytes(),
            )
            .await?;
            return Ok(());
        }
        Err(_) => {
            eprintln!("[iptv-proxy] upstream timed out for resolved youtube stream {id}");
            write_response(
                stream,
                502,
                "Bad Gateway",
                "text/plain",
                b"upstream timed out",
            )
            .await?;
            return Ok(());
        }
    };
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 206 {
        // Some CDNs reject Range outright; retry once without it.
        let retry = stream_get(resolved, None, Some("https://www.youtube.com"), None)
            .send()
            .await;
        if let Ok(r) = retry {
            if r.status().is_success() || r.status().as_u16() == 206 {
                let content_type = r
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();
                return relay_upstream(stream, r.status(), &content_type, r, is_head).await;
            }
        }
        eprintln!("[iptv-proxy] resolved youtube stream {id} failed with {status}");
        write_response(
            stream,
            status.as_u16(),
            status.canonical_reason().unwrap_or("Bad Gateway"),
            "text/plain",
            b"upstream error",
        )
        .await?;
        return Ok(());
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    relay_upstream(stream, status, &content_type, resp, is_head).await
}

fn parse_target(request: &str) -> Option<(String, Option<String>, Option<String>)> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    let mut url: Option<String> = None;
    let mut ua: Option<String> = None;
    let mut referer: Option<String> = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        let decoded = urlencoding::decode(v).ok()?.into_owned();
        match k {
            "url" => url = Some(decoded),
            "X-Rivulet-Ua" => ua = Some(decoded),
            "X-Rivulet-Referer" => referer = Some(decoded),
            _ => {}
        }
    }
    url.map(|u| (u, ua, referer))
}

fn extract_header(request: &str, name: &str) -> Option<String> {
    for line in request.lines().skip(1) {
        if line.is_empty() {
            return None;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in CORS_HEADERS {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn write_preflight(stream: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    let mut header = String::from("HTTP/1.1 204 No Content\r\n");
    for (k, v) in CORS_HEADERS {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nhttps://dead.invalid/a.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=200\nhttps://live.example/b.m3u8\n";

    #[test]
    fn dead_variant_hosts_are_dropped_with_their_tags() {
        let out = rewrite_m3u_filter(
            MASTER,
            "https://news.example/master.m3u8",
            None,
            None,
            |url| url.contains("live.example"),
        );
        assert!(
            !out.contains("dead.invalid"),
            "unresolvable variants must not reach mpv"
        );
        assert!(out.contains("live.example"), "reachable variants stay");
        assert!(out.contains("X-Rivulet-Referer="), "nested fetches inherit the playlist origin");
        assert_eq!(out.matches("#EXT-X-STREAM-INF").count(), 1);
    }

    #[test]
    fn master_playlist_puts_the_highest_rung_first() {
        const LADDER: &str = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=1280x720\nhttps://live.example/720.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=1920x1080\nhttps://live.example/1080.m3u8\n";
        let out = rewrite_m3u_filter(LADDER, "https://live.example/master.m3u8", None, None, |_| {
            true
        });
        let i1080 = out.find("1080.m3u8").expect("1080 variant");
        let i720 = out.find("720.m3u8").expect("720 variant");
        assert!(i1080 < i720, "ffmpeg/mpv pick the first STREAM-INF, so FHD must lead\n{out}");
    }

    #[test]
    fn a_master_of_only_dead_hosts_has_no_uri() {
        let out = rewrite_m3u_filter(MASTER, "https://news.example/master.m3u8", None, None, |_| {
            false
        });
        assert!(playlist_has_uri(MASTER));
        assert!(!playlist_has_uri(&out));
    }

    #[test]
    fn first_http_url_takes_a_single_line() {
        assert_eq!(
            first_http_url("https://googlevideo.example/v.mp4\n"),
            Some("https://googlevideo.example/v.mp4".into())
        );
    }

    #[test]
    fn first_http_url_rejects_separate_video_and_audio() {
        assert_eq!(
            first_http_url("https://v.example/a.webm\nhttps://v.example/b.m4a\n"),
            None
        );
        assert_eq!(first_http_url(""), None);
        assert_eq!(first_http_url("WARNING: something\n"), None);
    }
}
