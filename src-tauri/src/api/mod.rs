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

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;
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
        .route("/api/premium-tv/recent", get(routes_premium::recent))
        .route("/api/premium-tv/recent", post(routes_premium::add_recent))
        .route("/api/premium-tv/recent", delete(routes_premium::clear_recent))
        .route("/premium-stream/:token", get(routes_premium::stream_redirect))
        .layer(cors)
        .with_state(state)
}

/// The loopback address the Premium API binds. `pub` because
/// `premium::player` builds absolute redirector URLs from it: a
/// relative `/premium-stream/…` would resolve against `tauri://localhost`
/// in the webview and against nothing at all in mpv.
pub const ADDR: &str = "127.0.0.1:3032";

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
