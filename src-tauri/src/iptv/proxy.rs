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
    let is_head = request.starts_with("HEAD ");
    let range = extract_header(&request, "Range")
        .or_else(|| is_head.then(|| "bytes=0-0".to_string()));
    eprintln!("[iptv-proxy] {} {target_url}", if is_head { "HEAD" } else { "GET" });

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
    let mut resp = resp;
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
    // Split the base into scheme+host and path so query strings on `base`
    // (rare on manifests, but possible) are preserved on relative resolves.
    let base_no_query = base.split('?').next().unwrap_or(base);
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let resolved = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            // Some CDNs sign segments with per-request tokens; a manifest
            // already has them baked in, so pass through unchanged.
            trimmed.to_string()
        } else if trimmed.starts_with('/') {
            // Absolute path — keep the base's scheme+host, drop its path.
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
            // Resolve relative to the manifest's directory.
            let base_dir = base_no_query.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            format!("{base_dir}/{trimmed}")
        };
        // An `#EXTVLCOPT:http-user-agent` or `http-referrer` on the
        // playlist applies to every request in the HLS chain, not only the
        // initial .m3u8. Without propagating it here, the manifest loads but
        // the CDN rejects each segment with 403 and the player can only say
        // "Playback failed". Keep the headers in the proxy URL so nested
        // manifests inherit them too.
        out.push_str(&format!("/stream?url={}", urlencoding::encode(&resolved)));
        if let Some(ua) = user_agent {
            out.push_str("&X-Rivulet-Ua=");
            out.push_str(&urlencoding::encode(ua));
        }
        if let Some(rf) = referer {
            out.push_str("&X-Rivulet-Referer=");
            out.push_str(&urlencoding::encode(rf));
        }
        out.push('\n');
    }
    out
}

/// YouTube video ids are always 11 characters from this alphabet.
fn valid_youtube_id(id: &str) -> bool {
    id.len() == 11
        && id.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_youtube_embed(request: &str) -> Option<(String, bool, bool, bool)> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1;
    let mut id: Option<String> = None;
    let mut autoplay = true;
    let mut mute = false;
    let mut looping = false;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        let decoded = urlencoding::decode(v).ok()?.into_owned();
        match k {
            "v" => id = Some(decoded),
            "autoplay" => autoplay = decoded != "0",
            "mute" => mute = decoded == "1",
            "loop" => looping = decoded == "1",
            _ => {}
        }
    }
    let id = id?;
    if !valid_youtube_id(&id) {
        return None;
    }
    Some((id, autoplay, mute, looping))
}

async fn serve_youtube_embed(
    stream: &mut tokio::net::TcpStream,
    request: &str,
) -> anyhow::Result<()> {
    let (id, autoplay, mute, looping) = match parse_youtube_embed(request) {
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
    // YouTube ignores loop unless playlist names this same video.
    if looping {
        params.push_str("&loop=1&playlist=");
        params.push_str(&id);
    }
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
