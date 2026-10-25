//! The front door of the torrent engine on 127.0.0.1:3030.
//!
//! librqbit's HTTP API has no hook for a check of our own: it builds its router
//! and serves it in one call. Run bare on 3030, it answered every page in the
//! user's browser — and its writes (`/torrents/{id}/delete`, `/torrents` with an
//! `output_folder`) are POSTs a page can send cross-site without a preflight.
//!
//! So the engine listens on a port the OS picks, behind a password minted fresh
//! every launch, and this forwards to it after `local_guard` has looked at the
//! request. Nothing else changes: the page, mpv and the Android service still
//! talk to 3030, ranges and all, and librqbit still answers CORS itself.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine as _;
use futures_util::TryStreamExt;

/// What a hidden engine needs to be reached: where it is and how to log in.
pub struct Hidden {
    pub addr: SocketAddr,
    pub user: String,
    pub pass: String,
}

/// 128 random bits, hex — a password nobody outside this process ever sees.
pub fn secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Front {
    base: String,
    auth: String,
    client: reqwest::Client,
}

/// Connection-level headers belong to one hop and must not be copied across.
const HOP: [HeaderName; 4] = [
    header::CONNECTION,
    header::TRANSFER_ENCODING,
    header::HOST,
    header::UPGRADE,
];

fn header_str<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// An add names where to write in its query (`output_folder`, `sub_folder`),
/// so it is held to the same folders as `download_url` — see `folders`.
fn folder_allowed(query: Option<&str>) -> bool {
    let Some(query) = query else { return true };
    url::form_urlencoded::parse(query.as_bytes()).all(|(key, value)| match key.as_ref() {
        "output_folder" => crate::folders::may_write(std::path::Path::new(value.as_ref())),
        "sub_folder" => !std::path::Path::new(value.as_ref())
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_))),
        _ => true,
    })
}

async fn forward(State(front): State<Arc<Front>>, req: Request) -> Response {
    let headers = req.headers();
    if !crate::local_guard::allowed(
        header_str(headers, &header::ORIGIN),
        header_str(headers, &header::HOST),
    ) {
        return (StatusCode::FORBIDDEN, "not from this app").into_response();
    }

    if !folder_allowed(req.uri().query()) {
        return (StatusCode::FORBIDDEN, "that folder has not been chosen for downloads").into_response();
    }

    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let method = req.method().clone();
    let mut out = reqwest::header::HeaderMap::new();
    for (name, value) in req.headers() {
        if !HOP.contains(name) && name != header::AUTHORIZATION {
            out.append(name.clone(), value.clone());
        }
    }
    out.insert(
        header::AUTHORIZATION,
        front.auth.parse().expect("basic auth header is ascii"),
    );

    // Request bodies here are a magnet or a .torrent file — read whole, at the
    // engine's own 10 MB cap. (axum's body stream is not `Sync`, which a
    // streamed reqwest body has to be.)
    let body = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "body too large").into_response(),
    };
    let upstream = match front
        .client
        .request(method, format!("{}{path}", front.base))
        .headers(out)
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };

    let mut response = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers() {
        if !HOP.contains(name) {
            response = response.header(name, value);
        }
    }
    let stream = upstream.bytes_stream().map_err(std::io::Error::other);
    response
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

/// Serve the front door on `listener` until the process exits.
pub async fn serve(listener: tokio::net::TcpListener, hidden: Hidden) -> anyhow::Result<()> {
    let auth = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", hidden.user, hidden.pass));
    let front = Arc::new(Front {
        base: format!("http://{}", hidden.addr),
        auth: format!("Basic {auth}"),
        // No timeout: a stream lasts as long as the film. No redirects and no
        // decompression: the answer goes back exactly as the engine gave it,
        // Content-Length included, or mpv cannot seek.
        client: reqwest::Client::builder()
            .no_proxy()
            .no_gzip()
            .no_brotli()
            .redirect(reqwest::redirect::Policy::none())
            .tcp_nodelay(true)
            .build()?,
    });
    let app = Router::new().fallback(forward).with_state(front);
    axum::serve(listener, app).await?;
    Ok(())
}
