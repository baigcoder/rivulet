//! The local HTTP API for Premium TV.
//!
//! One axum `Router` over `127.0.0.1:3032`, JWT-protected, gated
//! on the local subscription state. Bind is loopback-only and the
//! server runs for the lifetime of the process; see `lib.rs` for
//! the boot path.
//!
//! The Premium TV side lives in `src-tauri/src/premium/` and
//! exposes itself only through the handlers in `routes_premium.rs`.
//! Nothing in this module reads provider credentials directly —
//! those go through the `CredentialVault`, which loads them out of
//! the OS keychain-encrypted SQLite blob, hands the upstream URL
//! back to the player through a 302, and never returns the
//! password to a route.

pub mod auth;
pub mod commands;
pub mod entitlement;
pub mod routes_premium;

use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::premium::PremiumState;

/// What every error response looks like. `code` is a stable,
/// machine-readable string; `message` is what the frontend can
/// show. Neither ever contains the user's password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ApiError {
    /// 401 — the JWT is missing, expired, or has the wrong signature.
    Unauthorized(String),
    /// 403 — the request is fine but the local subscription is not
    /// premium-tier right now.
    PremiumRequired,
    /// 404 — the thing being asked for isn't there.
    NotFound(String),
    /// 400 — the request was malformed.
    BadRequest(String),
    /// 409 — the request is valid but conflicts with work already in
    /// flight. Only a catalog import produces this, and it is the reason
    /// a double-clicked *Refresh* does not start two downloads.
    Conflict(String),
    /// 502 — the *provider* failed: timed out, rate-limited us, or
    /// answered with something that isn't the protocol. Distinct from
    /// `Internal` because the frontend's answer differs: a 502 is worth
    /// retrying and worth naming the provider in, a 500 is a bug here.
    BadGateway(String),
    /// 500 — something in the API or the Premium module failed in a
    /// way the frontend can't recover from. The inner string is
    /// safe to log.
    Internal(String),
}

impl From<crate::premium::PremiumError> for ApiError {
    fn from(e: crate::premium::PremiumError) -> Self {
        use crate::premium::PremiumError as P;
        // `PremiumError`'s Display impls are contractually free of
        // credentials (see the module doc on `premium/errors.rs`), which
        // is what makes it safe to pass provider-facing text straight
        // through to the user. It is also the only text that tells them
        // whether to retry now, retry later, or check the URL they typed.
        let message = e.to_string();
        match e {
            P::AuthFailed => ApiError::Unauthorized("provider rejected credentials".into()),
            P::SessionExpired => ApiError::Unauthorized("provider session expired".into()),
            P::PremiumRequired => ApiError::PremiumRequired,
            P::ProviderNotConnected => ApiError::NotFound("no provider connected".into()),
            P::NotFound => ApiError::NotFound("not found".into()),
            P::Cancelled => ApiError::BadRequest("cancelled".into()),
            P::AlreadySyncing => {
                ApiError::Conflict("a channel import is already running".into())
            }
            // The provider misbehaved, not us. 502 rather than 500 so
            // the frontend can tell "their server is down" from "our
            // code broke" and offer a retry for the one and not the
            // other.
            P::Timeout
            | P::RateLimited
            | P::Network(_)
            | P::MalformedResponse(_)
            | P::ServerError(_) => ApiError::BadGateway(message),
            P::Database(_) | P::CredentialError(_) => ApiError::Internal(message),
        }
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::NotFound("not found".into()),
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl From<ApiError> for crate::premium::errors::PremiumError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::Unauthorized(_) => crate::premium::errors::PremiumError::AuthFailed,
            ApiError::PremiumRequired => crate::premium::errors::PremiumError::PremiumRequired,
            ApiError::NotFound(_) => crate::premium::errors::PremiumError::NotFound,
            ApiError::BadRequest(_) => crate::premium::errors::PremiumError::Cancelled,
            ApiError::Conflict(_) => crate::premium::errors::PremiumError::AlreadySyncing,
            ApiError::BadGateway(m) | ApiError::Internal(m) => {
                crate::premium::errors::PremiumError::ServerError(m)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match self {
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", m),
            ApiError::PremiumRequired => (
                StatusCode::FORBIDDEN,
                "PREMIUM_REQUIRED",
                "Premium TV is not available on this install.".to_string(),
            ),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "NOT_FOUND", m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "CONFLICT", m),
            ApiError::BadGateway(m) => (StatusCode::BAD_GATEWAY, "PROVIDER_ERROR", m),
            ApiError::Internal(m) => {
                eprintln!("[premium-api] internal error: {m}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    "Something went wrong on our side.".to_string(),
                )
            }
        };
        (
            status,
            Json(ErrorBody {
                code: code.to_string(),
                message,
            }),
        )
            .into_response()
    }
}

/// Shared state for every axum handler.
///
/// `entitlement` is a field and not a `Router` layer on purpose — see the
/// type's own doc comment. A handler that needs the gate takes it from
/// here, so a build that forgot to wire one up fails to compile instead
/// of failing at request time.
#[derive(Clone)]
pub struct ApiState {
    pub premium: Arc<PremiumState>,
    pub entitlement: Arc<entitlement::EntitlementState>,
}

/// Whether an Origin header belongs to this app or its local development
/// server. A browser origin includes its port, so accepting only
/// `http://localhost` rejects `http://localhost:3001` when the normal dev
/// port is already occupied.
fn is_local_app_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    origin == "tauri://localhost"
        || origin == "https://tauri.localhost"
        || origin == "http://tauri.localhost"
        || origin == "http://localhost"
        || origin.starts_with("http://localhost:")
        || origin == "http://127.0.0.1"
        || origin.starts_with("http://127.0.0.1:")
}

/// Build the router. CORS admits the Tauri origin and loopback development
/// origins only. The bearer token still authorizes every API route.
pub fn build_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| is_local_app_origin(origin)))
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    // axum 0.7 path syntax (`:id`). 0.8 changed it to `{id}`; a bump
    // that misses these silently stops matching.
    Router::new()
        .route("/api/premium-tv/health", get(|| async { "ok" }))
        .route("/api/premium-tv/status", get(routes_premium::status))
        .route("/api/premium-tv/account", get(routes_premium::account))
        .route("/api/premium-tv/connect", post(routes_premium::connect))
        .route("/api/premium-tv/disconnect", post(routes_premium::disconnect))
        .route("/api/premium-tv/refresh", post(routes_premium::refresh))
        .route("/api/premium-tv/dashboard", get(routes_premium::dashboard))
        .route("/api/premium-tv/categories", get(routes_premium::categories))
        .route(
            "/api/premium-tv/categories/counts",
            get(routes_premium::category_counts),
        )
        .route("/api/premium-tv/channels", get(routes_premium::channels))
        .route("/api/premium-tv/channels/:id", get(routes_premium::channel))
        .route("/api/premium-tv/channels/:id/epg", get(routes_premium::epg))
        .route("/api/premium-tv/channels/:id/play", post(routes_premium::play))
        .route("/api/premium-tv/channels/:id/qualities", get(routes_premium::quality_variants))
        // POST, not GET, because the id list is a request body: a page of
        // 60 channel ids does not fit a query string that every proxy and
        // log truncates at some length of its own choosing.
        .route("/api/premium-tv/epg/now-next", post(routes_premium::epg_now_next))
        .route("/api/premium-tv/favorites", get(routes_premium::favorites))
        .route("/api/premium-tv/favorites/:id", post(routes_premium::toggle_favorite))
        .route("/api/premium-tv/recent", get(routes_premium::recent).post(routes_premium::add_recent).delete(routes_premium::clear_recent))
        .route("/api/premium-tv/vod/movies/categories", get(routes_premium::vod_movie_categories))
        .route("/api/premium-tv/vod/series/categories", get(routes_premium::vod_series_categories))
        .route("/api/premium-tv/vod/movies", get(routes_premium::vod_movies))
        .route("/api/premium-tv/vod/series", get(routes_premium::vod_series))
        .route("/api/premium-tv/vod/series/:id", get(routes_premium::vod_series_detail))
        .route("/api/premium-tv/vod/movies/:id/play", post(routes_premium::vod_play_movie))
        .route("/api/premium-tv/vod/episodes/:id/play", post(routes_premium::vod_play_episode))
        .route("/premium-stream/:token", get(routes_premium::stream_redirect))
        .route("/api/premium-tv/proxy/image", get(proxy_image))
        .layer(cors)
        .with_state(state)
}

/// The loopback address the Premium API binds. `pub` because
/// `premium::player` builds absolute redirector URLs from it: a
/// relative `/premium-stream/…` would resolve against `tauri://localhost`
/// in the webview and against nothing at all in mpv.
pub const ADDR: &str = "127.0.0.1:3032";

// ── Channel logo proxy ─────────────────────────────────────────────

/// How long a failed logo is remembered as failed.
///
/// The point is not to save a byte of bandwidth, it is to stop the
/// webview asking again. A 50,000-channel grid scrolled twice asks for
/// the same dead logo twice, and each ask costs a connection attempt and
/// up to `LOGO_TIMEOUT` of a blocking thread.
const LOGO_MISS_TTL: Duration = Duration::from_secs(10 * 60);

// There is deliberately no per-*host* breaker here, and that is a
// correction rather than an omission.
//
// One shipped: three consecutive failures skipped a whole host for two
// minutes, on the theory that a provider points all 50,000 of its logos
// at one image host, so a dead host makes every per-URL miss a first
// offence. The theory was wrong about how these hosts fail. A real
// provider serves its channel logos and its film posters from the *same*
// endpoint on the same host, and answers 502 for every channel logo and
// 200 for every poster — deterministically, not intermittently. So the
// channel grid struck the host out, and then the Movies and TV shows
// tabs were refused artwork that would have loaded, each refusal cached
// by the webview for ten minutes. It turned one broken image class into
// no images at all.
//
// A host is not a useful unit of failure when one host serves both. The
// per-URL cache below is the right grain, and what made a dead logo
// expensive in the first place — an agent per request, an 8s timeout,
// unbounded blocking tasks — is fixed directly.

/// Total budget for one logo. Well under the old 8s: a logo the grid is
/// still waiting for after this is one the viewer has scrolled past.
const LOGO_TIMEOUT: Duration = Duration::from_secs(5);

/// Logos in flight at once.
///
/// Every fetch holds a `spawn_blocking` thread for as long as the
/// upstream takes, and a fast scroll asks for hundreds. Left unbounded
/// that saturates tokio's blocking pool — which is shared with
/// everything else in the process that blocks — so a dead image host
/// stalled work that had nothing to do with logos.
const LOGO_CONCURRENCY: usize = 8;

/// Cap on a logo body. A channel logo is a few KB; anything at this size
/// is not artwork, and the whole thing is read into memory.
const LOGO_MAX_BYTES: usize = 4 * 1024 * 1024;

/// One pooled agent for every logo, built once.
///
/// It was previously built per request, which threw away the connection
/// pool and the TLS session cache — sixty visible cards meant sixty
/// fresh handshakes to the same host.
static LOGO_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

fn logo_agent() -> &'static ureq::Agent {
    LOGO_AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::config::Config::builder()
                .timeout_global(Some(LOGO_TIMEOUT))
                .user_agent("Rivulet")
                .build(),
        )
    })
}

static LOGO_GATE: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

fn logo_gate() -> &'static Arc<tokio::sync::Semaphore> {
    LOGO_GATE.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(LOGO_CONCURRENCY)))
}

#[derive(Default)]
struct LogoHealth {
    /// URLs known to have failed, and when. Bounded — see `note_miss`.
    /// Per URL, never per host: see the note above the constants.
    misses: HashMap<String, Instant>,
}

static LOGO_HEALTH: OnceLock<Mutex<LogoHealth>> = OnceLock::new();

fn logo_health() -> &'static Mutex<LogoHealth> {
    LOGO_HEALTH.get_or_init(|| Mutex::new(LogoHealth::default()))
}

/// `true` if this exact URL is known to have failed recently.
///
/// This exact URL, and nothing broader. A sibling image on the same host
/// is a different question and gets its own request.
fn logo_is_known_bad(url: &str) -> bool {
    let mut health = logo_health().lock().unwrap_or_else(|e| e.into_inner());
    let miss = health.misses.get(url).copied();
    match miss {
        Some(at) if at.elapsed() < LOGO_MISS_TTL => true,
        Some(_) => {
            health.misses.remove(url);
            false
        }
        None => false,
    }
}

fn note_miss(url: &str) {
    let mut health = logo_health().lock().unwrap_or_else(|e| e.into_inner());
    // A catalog holds more distinct logo URLs than it is worth
    // remembering failures for — one provider here has 52,298. Drop the
    // expired entries first; if that is not enough, drop the lot. The
    // cost of forgetting is one more request per logo, which is what the
    // pooled agent and the concurrency gate are for.
    if health.misses.len() >= 8192 {
        health.misses.retain(|_, at| at.elapsed() < LOGO_MISS_TTL);
        if health.misses.len() >= 8192 {
            health.misses.clear();
        }
    }
    health.misses.insert(url.to_string(), Instant::now());
}

fn note_hit(url: &str) {
    let mut health = logo_health().lock().unwrap_or_else(|e| e.into_inner());
    health.misses.remove(url);
}

/// A logo the proxy could not get, in a form the page will not ask for
/// again.
///
/// `404`, not a placeholder image: the cards already draw their own
/// fallback on an `<img>` error (see `PremiumChannelCard`), and that is a
/// better tile than a transparent pixel. The `Cache-Control` is the whole
/// point — without it the webview re-requests every dead logo on every
/// scroll.
fn logo_unavailable() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_FOUND;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=600"),
    );
    response
}

/// Reject anything that is not a public address.
///
/// The old check was three literal hostnames, which let through every
/// other way of naming this machine or the network it is on: `127.0.0.2`,
/// `[::1]`, `10.x`, `192.168.x`, and the cloud metadata address. This
/// route takes a URL out of a provider's catalog and fetches it from
/// inside the user's network, so it is exactly the shape of thing that
/// should not be pointable at the user's router.
///
/// It is a check on the URL, not on where the URL resolves: a domain
/// whose A record is `192.168.1.1` still gets through. Closing that
/// needs the resolution and the connection to be the same decision,
/// which is a custom resolver on the agent rather than a test here.
fn logo_host_is_public(host: &url::Host<&str>) -> bool {
    use std::net::IpAddr;
    match host {
        url::Host::Domain(name) => {
            let lower = name.to_ascii_lowercase();
            !(lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
                || lower.ends_with(".home.arpa"))
        }
        url::Host::Ipv4(ip) => ip_is_public(&IpAddr::V4(*ip)),
        url::Host::Ipv6(ip) => ip_is_public(&IpAddr::V6(*ip)),
    }
}

fn ip_is_public(ip: &std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || v4.is_multicast()
                || o[0] == 0
                // 100.64.0.0/10, carrier-grade NAT — and what a phone
                // on mobile data sits behind.
                || (o[0] == 100 && (o[1] & 0xc0) == 64))
        }
        IpAddr::V6(v6) => {
            let mapped_is_private = v6
                .to_ipv4_mapped()
                .map(|m| !ip_is_public(&IpAddr::V4(m)))
                .unwrap_or(false);
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 unique-local.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 link-local.
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // An IPv4-mapped address is an IPv4 address wearing a hat.
                || mapped_is_private)
        }
    }
}

/// Image proxy for channel logos. Provider logo URLs are often `http://`
/// which the Android webview blocks as mixed content from its
/// `https://tauri.localhost` origin. This handler fetches the image
/// server-side and returns it with proper headers.
///
/// Uses `ureq` (blocking, webpki-roots) instead of `reqwest` because
/// reqwest's `rustls` feature pulls in `rustls-platform-verifier`, which
/// panics on Android unless JNI-initialized — and the premium API has no
/// access to the JNI env.
///
/// Everything above this function exists because of scale. A 50,000
/// channel catalog points at one logo host, and when that host is slow or
/// down the honest answer has to arrive quickly and stay cached, or the
/// grid waits on artwork nobody is looking at any more.
async fn proxy_image(
    axum::extract::RawQuery(raw): axum::extract::RawQuery,
) -> Result<Response, StatusCode> {
    let raw = raw.ok_or(StatusCode::BAD_REQUEST)?;
    let url = url::form_urlencoded::parse(raw.as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .ok_or(StatusCode::BAD_REQUEST)?;
    // Validate: only http(s) URLs, and nothing on this machine or this
    // network.
    let parsed = url::Url::parse(&url).map_err(|_| StatusCode::BAD_REQUEST)?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let Some(host) = parsed.host() else {
        return Err(StatusCode::BAD_REQUEST);
    };
    if !logo_host_is_public(&host) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if logo_is_known_bad(&url) {
        return Ok(logo_unavailable());
    }

    // Queue behind the gate rather than starting an unbounded number of
    // blocking fetches. A closed semaphore is the only error and nothing
    // closes this one.
    let Ok(permit) = logo_gate().clone().acquire_owned().await else {
        return Ok(logo_unavailable());
    };

    let url_clone = url.clone();
    let fetched = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let resp = logo_agent()
            .get(&url_clone)
            .call()
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();
        let mut buf = Vec::new();
        // `take` rather than a bare `read_to_end`: this reads a URL out
        // of a provider's catalog, and a provider does not get to decide
        // how much of this process's memory that is worth.
        resp.into_body()
            .into_reader()
            .take(LOGO_MAX_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        if buf.is_empty() || buf.len() > LOGO_MAX_BYTES {
            return Err(StatusCode::BAD_GATEWAY);
        }
        Ok::<_, StatusCode>((buf, content_type))
    })
    .await;

    let (body, content_type) = match fetched {
        Ok(Ok(pair)) => {
            note_hit(&url);
            pair
        }
        // A failed fetch and a panicked task are the same thing to the
        // page: no logo, and don't ask again for a while.
        Ok(Err(_)) | Err(_) => {
            note_miss(&url);
            return Ok(logo_unavailable());
        }
    };

    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("image/jpeg")),
    );
    // Cache for 7 days — logos rarely change
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800"),
    );
    Ok(response)
}

/// Run the server until the process exits. Bound to loopback only —
/// nothing outside the host can reach this address.
pub async fn run(state: ApiState) -> anyhow::Result<()> {
    let addr: SocketAddr = ADDR.parse()?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("[premium-api] listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
