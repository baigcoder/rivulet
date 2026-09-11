use std::collections::HashMap;
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
/// Debrid hosts want this. Xtream live does not — see `IPTV_PLAYER_UA`.
const BROWSER_HEADERS: &[(&str, &str)] = &[
    ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"),
    ("Accept", "*/*"),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Connection", "keep-alive"),
];

/// What TiviMate / IPTV Smarters send. Xtream panels inspect UA: a
/// Chrome string gets the HTML5 HLS transcode (often 720p H.264); a
/// player string gets the original MPEG-TS — the FHD/4K HEVC feed the
/// channel is named for. Same string `streaming_m3u` already uses.
pub const IPTV_PLAYER_UA: &str = "VLC/3.0.18 LibVLC/3.0.18";

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
            .connect_timeout(Duration::from_secs(15))
            // Video must stay identity: gzip/br advertise Accept-Encoding, then
            // reqwest strips Content-Length and mpv sees a chunked file — it
            // cannot seek and Direct play sits on Buffering until a huge probe.
            .gzip(false)
            .brotli(false)
            .http1_only()
            .tcp_nodelay(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()
            .expect("failed to build stream HTTP client")
    })
}

/// The kind of an Xtream media URL, by the panel's own shape:
/// `/live/<user>/<pass>/<id>` (and `movie`, `series`), or
/// `/timeshift/<user>/<pass>/<duration>/<start>/<id>`.
///
/// The shape, not "the path contains /live/": 590 of the free channels sit on
/// CDN paths like `/hls/live/2020766/ndr_int/index.m3u8`, and calling those
/// Xtream sent them the VLC User-Agent — which some CDNs answer with 403 — and
/// the no-Range, one-connection handling a panel's MPEG-TS needs.
fn xtream_kind(url: &str) -> Option<&'static str> {
    let parsed = url::Url::parse(url).ok()?;
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    let (first, rest) = segments.split_first()?;
    let kind = match *first {
        "live" => "live",
        "movie" => "movie",
        "series" => "series",
        "timeshift" => "timeshift",
        _ => return None,
    };
    let shaped = if kind == "timeshift" {
        rest.len() >= 5
    } else {
        rest.len() == 3
    };
    shaped.then_some(kind)
}

fn is_xtream_media_url(url: &str) -> bool {
    xtream_kind(url).is_some()
}

fn url_path(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn looks_like_ts(url: &str) -> bool {
    url_path(url).ends_with(".ts")
}

fn looks_like_hls(url: &str) -> bool {
    let path = url_path(url);
    path.ends_with(".m3u8") || path.ends_with(".m3u")
}

/// Xtream live MPEG-TS (`/live/user/pass/id.ts`), not an HLS playlist
/// and not a finite VOD file. These feeds are one-connection, not
/// seekable, and often go silent without FIN — that is the "played,
/// then Buffering forever" stall.
fn is_live_mpegts(url: &str) -> bool {
    matches!(xtream_kind(url), Some("live" | "timeshift")) && !looks_like_hls(url)
}

/// Xtream/CDN often stop sending bytes without closing TCP. reqwest
/// waits forever, mpv empties `cache-secs` and sits on Buffering.
/// Cut the pipe so lavf can reconnect.
const LIVE_STALL: Duration = Duration::from_secs(5);

/// A `.ts` request that 302'd onto a playlist is the transcode ladder,
/// not the original 4K feed. Do not cache that hop.
fn is_hls_downgrade(from: &str, to: &str) -> bool {
    looks_like_ts(from) && looks_like_hls(to)
}

fn default_upstream_ua<'a>(url: &'a str, custom: Option<&'a str>) -> Option<&'a str> {
    if let Some(ua) = custom.filter(|s| !s.is_empty()) {
        return Some(ua);
    }
    if is_xtream_media_url(url) {
        return Some(IPTV_PLAYER_UA);
    }
    None
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
///
/// Ninety seconds, not ten minutes. The probes this exists for all
/// arrive within seconds of a zap, and the location it caches is not
/// ours to assume is durable: a panel's 302 commonly lands on a
/// tokenised CDN path whose own expiry is minutes away (one measured
/// here carried a timestamp four minutes out). Past that the cached hop
/// is a dead URL, and while the retry below does recover from one, the
/// recovery costs a stall and a second upstream connection — on an
/// account whose limit is often exactly one.
const REDIR_TTL: Duration = Duration::from_secs(90);

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
    if from == to || is_hls_downgrade(from, to) {
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
pub async fn run_proxy() -> anyhow::Result<()> {
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
    // Live MPEG-TS is worse: Range/HEAD opens a second Xtream slot, the
    // playing connection dies, and the player buffers until the user zaps.
    let is_head = request.starts_with("HEAD ");
    let live_ts = is_live_mpegts(&target_url);
    if is_head && live_ts {
        write_live_head(stream).await?;
        return Ok(());
    }
    let range = if live_ts {
        None
    } else {
        extract_header(&request, "Range").or_else(|| is_head.then(|| "bytes=0-0".to_string()))
    };
    eprintln!("[iptv-proxy] {} {target_url}", if is_head { "HEAD" } else { "GET" });

    let ua = default_upstream_ua(&target_url, custom_ua.as_deref());
    let rf = custom_referer.as_deref();
    // Hold only while following the resolver 302 — not while the movie
    // body streams, or a Range seek would wait on the first connection.
    // Live MPEG-TS keeps the lock for the body so a probe cannot steal
    // the one Xtream slot from the playing GET.
    let gate = url_gate(&target_url);
    let _resolve = gate.lock().await;
    let mut fetch_url = cached_redirect(&target_url).unwrap_or_else(|| target_url.clone());
    if is_hls_downgrade(&target_url, &fetch_url) {
        forget_redirect(&target_url);
        fetch_url = target_url.clone();
    }
    if fetch_url != target_url {
        eprintln!("[iptv-proxy] cached {fetch_url}");
    }

    let resp = match stream_get(&fetch_url, ua, rf, range.as_deref()).send().await {
        Ok(r) => r,
        Err(e) if fetch_url != target_url => {
            eprintln!("[iptv-proxy] cached upstream failed ({e}), retrying resolver");
            forget_redirect(&target_url);
            fetch_url = target_url.clone();
            match stream_get(&fetch_url, ua, rf, range.as_deref()).send().await {
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
        if let Ok(r) = stream_get(&fetch_url, ua, rf, range.as_deref()).send().await {
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
    // Where the bytes actually came from. A panel answers `/live/u/p/id.ts`
    // with a 302 onto its HLS ladder, and both the manifest test below and the
    // base each relative segment line is resolved against have to be *that*
    // URL, not the one we asked for.
    let final_url = resp.url().to_string();
    if !live_ts {
        drop(_resolve);
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // HLS manifests need their URLs rewritten so the webview fetches the
    // proxy, not the origin. The manifest itself is small (a few KB), so
    // buffering it is fine and lets us rewrite every line.
    //
    // `looks_like_hls`, not `ends_with(".m3u8")`: a panel's playlist URL
    // usually carries a token in the query, and it is the redirect target
    // rather than the `.ts` we asked for. Missing it here left the ladder
    // unsorted and its relative segment lines pointing at this proxy's own
    // origin — a 4K channel that played its bottom rung, or nothing at all.
    if content_type.contains("mpegurl") || looks_like_hls(&target_url) || looks_like_hls(&final_url)
    {
        // Bytes, not `text()`: a server that gzips its playlist unasked (this
        // client sends no Accept-Encoding, so nothing decodes it) became a
        // manifest of binary garbage rewritten line by line, and a channel
        // that plays direct failed here with "parse_playlist error".
        match resp.bytes().await {
            Ok(raw) => {
                let body = manifest_text(&raw);
                let rewritten = prefer_highest_hls_rung(&rewrite_m3u(
                    &body,
                    &final_url,
                    custom_ua.as_deref(),
                    custom_referer.as_deref(),
                ));
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

    // Stream the response through to the client in real-time. HLS segments
    // and live video chunks are never-ending streams — buffering the whole
    // body (the old approach) hangs forever waiting for EOF. Instead:
    //   1. Write response headers (status, content-type, CORS, Content-Length
    //      if the upstream told us).
    //   2. Forward each chunk from reqwest to the TCP stream as it arrives.
    //   3. When the upstream has no Content-Length, the response uses
    //      Transfer-Encoding: chunked so the browser knows where each frame
    //      ends.
    let upstream_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    // Chunked live TS is why lavf never reconnects: it treats each
    // chunk boundary as a file and will not reopen on EOF. HTTP/1.0
    // identity (close = end) is what Icecast / IPTV proxies send.
    let use_identity = live_ts || (upstream_length.is_none() && is_live_mpegts(&fetch_url));
    let mut header = if use_identity {
        format!(
            "HTTP/1.0 {} {}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("OK"),
        )
    } else {
        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {content_type}\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("OK"),
        )
    };
    if !use_identity {
        if let Some(len) = upstream_length {
            header.push_str(&format!("Content-Length: {len}\r\n"));
        } else {
            // Upstream used chunked transfer encoding (or no length at all).
            // Forward as chunked so the browser can frame each chunk.
            header.push_str("Transfer-Encoding: chunked\r\n");
        }
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
        if use_identity && (h == "Content-Range" || h == "Accept-Ranges") {
            continue;
        }
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
    let mut resp = resp;
    let use_chunked = !use_identity && upstream_length.is_none();
    loop {
        let next = if live_ts || use_identity {
            match tokio::time::timeout(LIVE_STALL, resp.chunk()).await {
                Ok(inner) => inner,
                Err(_) => {
                    eprintln!("[iptv-proxy] live stall, closing {target_url}");
                    break;
                }
            }
        } else {
            resp.chunk().await
        };
        match next {
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
fn rewrite_m3u(
    body: &str,
    base: &str,
    user_agent: Option<&str>,
    referer: Option<&str>,
) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if trimmed.starts_with('#') {
            // A tag can point at a file too: the audio and subtitle renditions
            // (`#EXT-X-MEDIA`), an fMP4 init segment (`#EXT-X-MAP`), a key, the
            // I-frame ladder. Left alone, a relative one resolved against this
            // proxy's own origin and got a 400 — a channel with separate audio
            // played silent or not at all.
            out.push_str(&rewrite_uri_attrs(line, base, user_agent, referer));
            out.push('\n');
            continue;
        }
        out.push_str(&proxied_path(
            &resolve_against(base, trimmed),
            user_agent,
            referer,
        ));
        out.push('\n');
    }
    out
}

/// A reference in a manifest, as the upstream meant it. An absolute URL is
/// passed through untouched — CDNs sign segments with per-request tokens, and
/// re-encoding one breaks it. Anything else resolves the way a browser would:
/// `../` walks up, and an absolute path keeps the base's port, which the old
/// hand-rolled join dropped.
fn resolve_against(base: &str, reference: &str) -> String {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return reference.to_string();
    }
    url::Url::parse(base)
        .and_then(|b| b.join(reference))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| reference.to_string())
}

/// The proxy path for an upstream URL.
///
/// An `#EXTVLCOPT:http-user-agent` or `http-referrer` on the playlist applies
/// to every request in the HLS chain, not only the initial .m3u8. Without
/// propagating it here, the manifest loads but the CDN rejects each segment
/// with 403 and the player can only say "Playback failed". Keep the headers in
/// the proxy URL so nested manifests inherit them too.
fn proxied_path(resolved: &str, user_agent: Option<&str>, referer: Option<&str>) -> String {
    let mut out = format!("/stream?url={}", urlencoding::encode(resolved));
    if let Some(ua) = user_agent {
        out.push_str("&X-Rivulet-Ua=");
        out.push_str(&urlencoding::encode(ua));
    }
    if let Some(rf) = referer {
        out.push_str("&X-Rivulet-Referer=");
        out.push_str(&urlencoding::encode(rf));
    }
    out
}

/// Every `URI="…"` in a tag line, pointed back at the proxy.
fn rewrite_uri_attrs(
    line: &str,
    base: &str,
    user_agent: Option<&str>,
    referer: Option<&str>,
) -> String {
    const KEY: &str = "URI=\"";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(KEY) {
        let value_start = at + KEY.len();
        out.push_str(&rest[..value_start]);
        let tail = &rest[value_start..];
        let Some(end) = tail.find('"') else {
            out.push_str(tail);
            return out;
        };
        let value = &tail[..end];
        // `data:` carries its bytes inline and `skd:` is a DRM key id — neither
        // is somewhere to fetch from.
        if value.is_empty() || value.starts_with("data:") || value.starts_with("skd:") {
            out.push_str(value);
        } else {
            out.push_str(&proxied_path(
                &resolve_against(base, value),
                user_agent,
                referer,
            ));
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// A manifest body as text, gunzipped first when the upstream compressed it
/// without being asked (the gzip magic is the only reliable sign: such servers
/// often send no `Content-Encoding` at all).
fn manifest_text(raw: &[u8]) -> String {
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        if std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(raw), &mut out).is_ok() {
            return String::from_utf8_lossy(&out).into_owned();
        }
    }
    String::from_utf8_lossy(raw).into_owned()
}

/// Master playlists often list the 720p rung first (the "default" a phone
/// can play). mpv then stays on that variant even when a 4K one exists.
/// Put the tallest / fattest rung first so `--hls-bitrate=max` has a real max.
fn prefer_highest_hls_rung(body: &str) -> String {
    if !body.contains("#EXT-X-STREAM-INF") {
        return body.to_string();
    }
    let lines: Vec<&str> = body.lines().collect();
    struct Variant {
        start: usize,
        end: usize,
        bandwidth: u64,
        height: u64,
    }
    let mut vars = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("#EXT-X-STREAM-INF") {
            let bandwidth = hls_bandwidth(lines[i]);
            let height = hls_height(lines[i]);
            let mut end = i + 1;
            while end < lines.len() && (lines[end].starts_with('#') || lines[end].trim().is_empty())
            {
                end += 1;
            }
            if end < lines.len() {
                vars.push(Variant {
                    start: i,
                    end: end + 1,
                    bandwidth,
                    height,
                });
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    if vars.len() < 2 {
        return body.to_string();
    }
    let first = vars[0].start;
    let last = vars.last().unwrap().end;
    // Only reorder a contiguous ladder — interleaved MEDIA tags stay put.
    if vars.windows(2).any(|w| w[0].end != w[1].start) {
        return body.to_string();
    }
    let mut order: Vec<usize> = (0..vars.len()).collect();
    order.sort_by(|a, b| {
        vars[*b]
            .height
            .cmp(&vars[*a].height)
            .then(vars[*b].bandwidth.cmp(&vars[*a].bandwidth))
    });
    if order.iter().copied().eq(0..vars.len()) {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    for line in &lines[..first] {
        out.push_str(line);
        out.push('\n');
    }
    for idx in order {
        for line in &lines[vars[idx].start..vars[idx].end] {
            out.push_str(line);
            out.push('\n');
        }
    }
    for line in &lines[last..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn hls_stream_inf(tag: &str) -> &str {
    tag.strip_prefix("#EXT-X-STREAM-INF:")
        .or_else(|| tag.strip_prefix("#EXT-X-I-FRAME-STREAM-INF:"))
        .unwrap_or(tag)
}

fn hls_attr(tag: &str, name: &str) -> Option<u64> {
    hls_stream_inf(tag)
        .split(',')
        .filter_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            if k.eq_ignore_ascii_case(name) {
                v.trim_matches('"').parse().ok()
            } else {
                None
            }
        })
        .next()
}

fn hls_bandwidth(tag: &str) -> u64 {
    hls_attr(tag, "BANDWIDTH")
        .or_else(|| hls_attr(tag, "AVERAGE-BANDWIDTH"))
        .unwrap_or(0)
}

fn hls_height(tag: &str) -> u64 {
    let rest = hls_stream_inf(tag);
    for part in rest.split(',') {
        let Some((k, v)) = part.trim().split_once('=') else {
            continue;
        };
        if k.eq_ignore_ascii_case("RESOLUTION") {
            let v = v.trim_matches('"');
            if let Some((_, h)) = v.split_once('x') {
                return h.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// YouTube video ids are always 11 characters from this alphabet.
fn valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id.bytes()
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
    // Controls on unless asked otherwise: a trailer the user opened is a
    // video they are watching, and the one caller that wants them gone
    // is the cover hero, which says so.
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
        "rel=0&playsinline=1&enablejsapi=1&vq=hd720&origin=http%3A%2F%2F127.0.0.1%3A3031",
    );
    if autoplay {
        params.push_str("&autoplay=1");
    }
    if mute {
        params.push_str("&mute=1");
    }
    // YouTube ignores `loop` unless `playlist` names this same video — but a
    // playlist, even one video long, is what makes it paint **previous and
    // next buttons** over the picture. On the cover hero that is the whole
    // complaint: a decorative background wearing a play button and two skip
    // arrows. So a hero loops by watching for the ended state and seeking
    // back to zero (`loop_ended` below), and only a player with visible
    // controls gets the playlist form.
    if looping && controls {
        params.push_str("&loop=1&playlist=");
        params.push_str(&id);
    }
    // The cover hero: mute is the app's own button, and nothing else here is
    // for pressing. `controls` takes the bar, `fs` the fullscreen button,
    // `iv_load_policy`/`cc_load_policy` the annotations and captions, and
    // `disablekb` the keyboard — which on a TV matters twice over, because a
    // d-pad press must never reach YouTube and seek the trailer.
    if !controls {
        params.push_str(
            "&controls=0&modestbranding=1&fs=0&disablekb=1&iv_load_policy=3&cc_load_policy=0",
        );
    }
    // Restarting on `ended` (state 0) is the loop for a hero. Harmless for a
    // playlist-looped player, which never reports ended in the first place.
    let loop_ended = if looping && !controls {
        r#"if(d&&d.info&&d.info.playerState===0){send("seekTo",[0,true]);send("playVideo");}"#
    } else {
        ""
    };
    // Forwards mute/unMute/quality from the page, and player state back up, so
    // the volume button does not reload the iframe and the hero can hide YouTube's
    // spinner until 720p is actually playing.
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
    send("setPlaybackQuality",["hd720"]);
    send("setPlaybackQualityRange",["hd720","hd720"]);
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
    if(d&&(d.event==="onReady"||(d.info&&d.info.playerState===1)))lock();
    {loop_ended}
  }});
  f.addEventListener("load",function(){{
    f.contentWindow.postMessage(JSON.stringify({{event:"listening"}}),yt);
    lock();
  }});
}})();
</script></body></html>"#
    );
    write_response(stream, 200, "OK", "text/html; charset=utf-8", html.as_bytes()).await
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

async fn write_live_head(stream: &mut tokio::net::TcpStream) -> anyhow::Result<()> {
    let mut header = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: video/mp2t\r\nAccept-Ranges: none\r\n",
    );
    for (k, v) in CORS_HEADERS {
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    header.push_str("Connection: close\r\n\r\n");
    stream.write_all(header.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
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
    use super::{
        default_upstream_ua, hls_bandwidth, hls_height, is_hls_downgrade, is_live_mpegts,
        is_xtream_media_url, looks_like_hls, manifest_text, prefer_highest_hls_rung, rewrite_m3u,
        IPTV_PLAYER_UA,
    };

    #[test]
    fn tag_uris_are_proxied_like_entry_lines() {
        let base = "https://cdn.example/live/master.m3u8?token=1";
        let body = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio/eng.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=1,AUDIO=\"a\"\n\
../video/720.m3u8\n";
        let out = rewrite_m3u(body, base, None, None);
        assert!(
            out.contains("URI=\"/stream?url=https%3A%2F%2Fcdn.example%2Flive%2Faudio%2Feng.m3u8\""),
            "the audio rendition goes through the proxy\n{out}"
        );
        assert!(
            out.contains("/stream?url=https%3A%2F%2Fcdn.example%2Fvideo%2F720.m3u8\n"),
            "`../` walks up a directory\n{out}"
        );
        // Inline data is not somewhere to fetch.
        let key = rewrite_m3u("#EXT-X-KEY:METHOD=AES-128,URI=\"data:text/plain;base64,AA\"\n", base, None, None);
        assert!(key.contains("URI=\"data:text/plain;base64,AA\""), "{key}");
    }

    #[test]
    fn an_absolute_path_keeps_the_port() {
        let out = rewrite_m3u("#EXTM3U\n/seg/1.ts\n", "http://host:8080/a/b.m3u8", None, None);
        assert!(out.contains("/stream?url=http%3A%2F%2Fhost%3A8080%2Fseg%2F1.ts"), "{out}");
    }

    #[test]
    fn a_gzipped_playlist_is_read_as_text() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"#EXTM3U\nseg.ts\n").unwrap();
        let raw = gz.finish().unwrap();
        assert_eq!(manifest_text(&raw), "#EXTM3U\nseg.ts\n");
        assert_eq!(manifest_text(b"#EXTM3U\n"), "#EXTM3U\n");
    }

    #[test]
    fn a_cdn_path_with_live_in_it_is_not_an_xtream_panel() {
        for url in [
            "https://ndrint.akamaized.net/hls/live/2020766/ndr_int/index.m3u8",
            "https://cdn4.skygo.mn/live/disk1/MNB2/HLSv3-FTA/MNB2.m3u8",
            "https://stream.syritv.al/live/syritv/playlist.m3u8",
        ] {
            assert!(!is_xtream_media_url(url), "{url}");
            assert!(!is_live_mpegts(url), "{url}");
            assert_eq!(default_upstream_ua(url, None), None, "{url} gets the browser UA");
        }
        assert!(is_xtream_media_url(
            "http://panel:8080/timeshift/u/p/120/2024-01-01:10-00/55.ts"
        ));
        assert!(is_xtream_media_url("http://panel:8080/movie/u/p/99.mkv"));
    }

    #[test]
    fn master_playlist_puts_the_highest_rung_first() {
        const LADDER: &str = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n\
low.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=3840x2160\n\
uhd.m3u8\n";
        let out = prefer_highest_hls_rung(LADDER);
        let uhd = out.find("uhd.m3u8").expect("4K rung stays");
        let low = out.find("low.m3u8").expect("low rung stays");
        assert!(uhd < low, "mpv plays the first variant — 4K must lead\n{out}");
        assert_eq!(
            hls_bandwidth("#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=3840x2160"),
            8_000_000
        );
        assert_eq!(
            hls_height("#EXT-X-STREAM-INF:BANDWIDTH=8000000,RESOLUTION=3840x2160"),
            2160
        );
    }

    #[test]
    fn master_playlist_uses_resolution_when_bandwidth_is_missing() {
        const LADDER: &str = "#EXTM3U\n\
#EXT-X-STREAM-INF:RESOLUTION=1280x720\n\
hd.m3u8\n\
#EXT-X-STREAM-INF:RESOLUTION=3840x2160\n\
uhd.m3u8\n";
        let out = prefer_highest_hls_rung(LADDER);
        let uhd = out.find("uhd.m3u8").expect("4K rung stays");
        let hd = out.find("hd.m3u8").expect("hd rung stays");
        assert!(uhd < hd, "height must rank the ladder when BANDWIDTH is absent\n{out}");
    }

    #[test]
    fn xtream_live_uses_a_player_ua_not_chrome() {
        assert!(is_xtream_media_url(
            "http://panel.example:8080/live/user/pass/1234.ts"
        ));
        assert_eq!(
            default_upstream_ua("http://panel.example:8080/live/user/pass/1234.ts", None),
            Some(IPTV_PLAYER_UA)
        );
        assert!(!IPTV_PLAYER_UA.contains("Mozilla"));
    }

    #[test]
    fn a_ts_to_hls_redirect_is_the_transcode_not_a_cache_hit() {
        assert!(is_hls_downgrade(
            "http://panel/live/u/p/1.ts",
            "http://panel/hls/1/index.m3u8"
        ));
        assert!(!is_hls_downgrade(
            "http://cdn/file.ts",
            "http://cdn/file.ts?token=1"
        ));
    }

    /// The manifest branch used to test `target_url.ends_with(".m3u8")`, which
    /// is false for both shapes a panel actually answers with: a playlist whose
    /// URL carries a token, and the `.m3u8` a `.ts` request was redirected to.
    /// Either one skipped the rung sort and the segment rewrite, so a 4K
    /// channel played its bottom rung — or, with relative segment lines,
    /// nothing at all.
    #[test]
    fn a_playlist_is_recognised_by_its_path_not_by_the_url_we_asked_for() {
        assert!(looks_like_hls("http://panel/hls/1/index.m3u8?token=abc"));
        assert!(looks_like_hls("http://panel/live/u/p/1.m3u8"));
        assert!(!"http://panel/hls/1/index.m3u8?token=abc".ends_with(".m3u8"));
        // The `.ts` we ask for is not a playlist; the URL it lands on is.
        assert!(!looks_like_hls("http://panel/live/u/p/1.ts"));
        assert!(looks_like_hls("http://panel/hls/1/index.m3u8"));
    }

    #[test]
    fn xtream_live_ts_is_mpegts_and_playlists_are_not() {
        assert!(is_live_mpegts(
            "http://panel.example:8080/live/user/pass/1234.ts"
        ));
        assert!(is_live_mpegts(
            "http://panel.example:8080/live/user/pass/1234"
        ));
        assert!(!is_live_mpegts(
            "http://panel.example:8080/live/user/pass/1234.m3u8"
        ));
        assert!(!is_live_mpegts(
            "http://panel.example:8080/movie/user/pass/99.mp4"
        ));
    }
}
