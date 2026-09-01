//! Premium TV route handlers.
//!
//! Every handler in this file passes through the same two gates, in the
//! same order, before it touches anything: `require_auth` (the bearer
//! JWT this process minted) and `ensure_premium` (the entitlement held
//! in `ApiState`). The order matters only for which error the caller
//! sees first, but the *presence* of both on every route is the point —
//! the version of this file that this one replaces checked the
//! entitlement on exactly one of thirteen routes.
//!
//! The one route with no `Authorization` header is
//! `/premium-stream/:token`, and it is not an exception: the token in
//! its path is signed by the same key, names the one channel it is good
//! for, and expires in thirty seconds. It is the only route a player
//! opens directly, and a player cannot be made to send headers.
//!
//! **No handler here talks to a provider except through
//! `premium::sync` or `premium::factory`, and no handler here builds
//! SQL.** Reads go through `PremiumRepository`, writes through
//! `premium::storage`. That is what keeps the credential-bearing URL
//! construction in the adapter that owns the protocol, and out of a
//! function whose job is to serialize JSON.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::auth;
use super::ApiState;
use crate::premium::errors::PremiumError;
use crate::premium::models::{
    CatalogState, CategoryCount, EpgProgram, IPTVCategory, IPTVChannel, IPTVChannelPage,
    PlaybackSource, PremiumAccount, PremiumDashboard, SyncReport,
};
use crate::premium::repository::PremiumRepository;
use crate::premium::storage::{self, ProviderConfig};
use crate::premium::xtream::{credentials_from_playlist_url, normalise_provider_url};
use crate::premium::{factory, player, sync};

/// How many programmes `/epg` returns when the caller names no limit.
/// Enough for a now/next line plus an "up next" column; a guide panel
/// that wants the whole day asks for it.
const DEFAULT_EPG_LIMIT: usize = 6;

/// Ceiling on `/epg?limit=`. A channel's guide is days deep, and an
/// unbounded limit turns one request into a multi-megabyte response.
const MAX_EPG_LIMIT: usize = 200;

/// Ceiling on the ids one now/next batch may name. A page of channels
/// is 60; this is generous for a large grid and still bounds the SQL.
const MAX_NOW_NEXT_IDS: usize = 500;

/// Default and maximum page size for `/channels`.
const DEFAULT_PAGE: usize = 60;

// ── Gates ──────────────────────────────────────────────────────────

/// Pull the bearer token out of `Authorization`, verify it, and return
/// the verified claims. Any failure is `401` — including a missing
/// header, which is the common case on a first request before the
/// frontend has asked Tauri for a token.
fn require_auth(headers: &axum::http::HeaderMap) -> Result<auth::ApiClaims, super::ApiError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| super::ApiError::Unauthorized("missing Authorization".into()))?;
    let token = raw
        .strip_prefix("Bearer ")
        .ok_or_else(|| super::ApiError::Unauthorized("expected Bearer".into()))?;
    auth::verify_api_token(token)
}

/// The entitlement gate. Read fresh on every request, never cached in a
/// handler or trusted from the body: a subscription that lapses or is
/// revoked has to stop the *next* request, including one from a webview
/// that has been open since before it lapsed.
fn ensure_premium(state: &ApiState) -> Result<(), super::ApiError> {
    if state.entitlement.is_premium() {
        Ok(())
    }
    else {
        Err(super::ApiError::PremiumRequired)
    }
}

/// Both gates, in one call. Every handler starts with this line.
fn guard(state: &ApiState, headers: &axum::http::HeaderMap) -> Result<(), super::ApiError> {
    require_auth(headers)?;
    ensure_premium(state)
}

/// The connected provider's id, or a 404.
///
/// `ProviderNotConnected` rather than a generic not-found, because the
/// frontend's answer is specific: send the user to the connect form,
/// not to an error toast.
fn active_connection(state: &ApiState) -> Result<String, super::ApiError> {
    let conn = lock(state)?;
    storage::active_connection(&conn)?
        .ok_or_else(|| super::ApiError::from(PremiumError::ProviderNotConnected))
}

fn repo(state: &ApiState) -> PremiumRepository {
    PremiumRepository::new(state.premium.clone())
}

// ── /status ────────────────────────────────────────────────────────

/// `/status` answers two questions at once: which account is connected,
/// and how much of its catalog is on disk. They are together because
/// the page that asks needs both to decide what to render — an account
/// with an empty catalog is "still importing", not "no channels".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub account: Option<PremiumAccount>,
    /// `None` when no provider is connected, so a caller can branch on
    /// one field instead of two.
    pub catalog: Option<CatalogState>,
}

pub async fn status(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<StatusResponse>, super::ApiError> {
    guard(&state, &headers)?;
    // Not connected is a valid state with a 200 body, not a 404: the
    // connect screen calls this to find out that it should render
    // itself.
    let id = {
        let conn = lock(&state)?;
        storage::active_connection(&conn)?
    };
    let Some(id) = id else {
        return Ok(Json(StatusResponse { account: None, catalog: None }));
    };
    let row = {
        let conn = lock(&state)?;
        storage::get_connection(&conn, &id)?
    };
    let Some(row) = row else {
        return Ok(Json(StatusResponse { account: None, catalog: None }));
    };
    let catalog = sync::catalog_state(&state.premium, &id)?;
    Ok(Json(StatusResponse {
        account: Some(account_from_row(row)),
        catalog: Some(catalog),
    }))
}

/// Re-ask the provider who we are, and say what it answered.
///
/// `/status` reads the row on disk, which was written when the catalog
/// last imported — good enough for a name and an expiry date, useless
/// for the one field that changes minute to minute: how many of the
/// account's simultaneous connections are in use. That number is the
/// difference between "this channel is off the air" and "your other
/// device is watching", and a viewer who is told the second one knows
/// what to do about it.
///
/// One `player_api.php` round trip, no playlist, no guide — cheap
/// enough for the player to ask after a failure and nowhere near the
/// cost of `refresh?force=true`. The provider adapter writes the fresh
/// counts back to the row on its way through, so `/status` improves too.
pub async fn account(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<PremiumAccount>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    let provider = factory::provider_for(state.premium.clone(), &id)?;
    let fresh = provider.authenticate().await?;
    Ok(Json(scrub(fresh)))
}

/// Blank the two fields the client has no use for and a pasted bug
/// report must not carry. Same rule as `account_from_row` below, applied
/// to an account that came straight from a provider.
fn scrub(mut account: PremiumAccount) -> PremiumAccount {
    account.server_url = String::new();
    account.username = String::new();
    account
}

/// A stored connection row as the client sees it.
///
/// `server_url` and `username` are deliberately blank. They are in the
/// vault, the client has no use for them, and a status response is
/// exactly the sort of body that gets pasted into a bug report.
fn account_from_row(row: storage::ConnectionRow) -> PremiumAccount {
    PremiumAccount {
        provider_type: row.provider_type,
        server_url: String::new(),
        username: String::new(),
        status: row.status,
        account_name: row.account_name.or(Some(row.display_name)),
        expires_at: row.expires_at,
        is_trial: row.is_trial,
        active_connections: row.active_connections,
        max_connections: row.max_connections,
    }
}

// ── /connect ───────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub m3u_url: Option<String>,
    pub account_name: Option<String>,
}

/// What `/connect` and `/refresh` both return: who you are signed in as
/// and what came down the wire.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResponse {
    pub account: PremiumAccount,
    pub report: SyncReport,
}

/// Connect a provider: store the credentials, then authenticate and
/// import the catalog for real.
///
/// The row and the secret are written *before* the handshake because
/// the adapter reads its own configuration back out of the vault — the
/// provider is constructed from the database, not from this request
/// body. The cost of that ordering is a row that outlives a failed
/// handshake, so a failure deletes it: leaving it behind would leave the
/// app "connected" to a provider that rejected the password, and every
/// subsequent read would answer an empty catalog rather than sending the
/// user back to this form.
pub async fn connect(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ConnectRequest>,
) -> Result<Json<ConnectResponse>, super::ApiError> {
    guard(&state, &headers)?;

    let mut last: Option<super::ApiError> = None;
    for cfg in provider_candidates(&req)? {
        let kind = match cfg {
            ProviderConfig::Xtream { .. } => "xtream",
            ProviderConfig::M3u { .. } => "m3u",
        };
        let display = display_name(&req, &cfg);

        let id = storage::new_connection_id();
        {
            let conn = lock(&state)?;
            storage::insert_connection(&conn, &id, kind, &display)?;
            let blob = cfg.encrypt(&state.premium.vault)?;
            storage::set_secret(&conn, &id, &blob)?;
        }

        match sync::connect(state.premium.clone(), &id).await {
            Ok((account, report)) => return Ok(Json(ConnectResponse { account, report })),
            Err(e) => {
                // Roll the connection back. `delete_connection` cascades to
                // the secret, so the rejected password does not stay on disk
                // either.
                if let Ok(conn) = lock(&state) {
                    let _ = storage::delete_connection(&conn, &id);
                }
                last = Some(e.into());
            }
        }
    }
    // `provider_candidates` never returns an empty list, so there is an
    // error here whenever the loop fell through.
    Err(last.unwrap_or_else(|| {
        super::ApiError::BadRequest("that provider could not be reached".into())
    }))
}

/// What the account is called in the UI: what the user typed, else the
/// username the credentials carry — including the one parsed out of a
/// pasted panel link, which the request body itself does not have.
fn display_name(req: &ConnectRequest, cfg: &ProviderConfig) -> String {
    req.account_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| match cfg {
            ProviderConfig::Xtream { username, .. } => Some(username.clone()),
            ProviderConfig::M3u { .. } => req.username.clone(),
        })
        .unwrap_or_else(|| "Premium TV".to_string())
}

/// The configs to try, in order.
///
/// One entry for everything except a pasted panel link, which is two: a
/// `get.php?username=…&password=…` URL is a panel *and* a playlist, and
/// the panel is worth trying first — it is the only one of the two that
/// can report an account status and an expiry date, and its channel list
/// is live-only rather than the films and series the same link would
/// hand back as one flat playlist. A panel that turns out not to serve
/// `player_api.php` then falls back to importing the same URL as the
/// playlist it also is, instead of refusing a link that works.
fn provider_candidates(req: &ConnectRequest) -> Result<Vec<ProviderConfig>, super::ApiError> {
    let cfg = provider_config(req)?;
    if let ProviderConfig::M3u { url } = &cfg {
        if let Some((server_url, username, password)) = credentials_from_playlist_url(url) {
            return Ok(vec![
                ProviderConfig::Xtream { server_url, username, password },
                cfg,
            ]);
        }
    }
    Ok(vec![cfg])
}

/// Turn a connect body into a provider config, or a 400 naming what is
/// missing.
///
/// Xtream is checked first and requires all three fields: a body with a
/// server URL and no password is a half-filled form, and answering it
/// with "need either Xtream creds or an M3U url" tells the user nothing
/// about which field they left blank.
fn provider_config(req: &ConnectRequest) -> Result<ProviderConfig, super::ApiError> {
    let trimmed = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let server_url = trimmed(&req.server_url);
    let username = trimmed(&req.username);
    let password = trimmed(&req.password);
    let m3u_url = trimmed(&req.m3u_url);

    if server_url.is_some() || username.is_some() || password.is_some() {
        let server_url =
            server_url.ok_or_else(|| super::ApiError::BadRequest("server URL is required".into()))?;
        let username =
            username.ok_or_else(|| super::ApiError::BadRequest("username is required".into()))?;
        let password =
            password.ok_or_else(|| super::ApiError::BadRequest("password is required".into()))?;
        require_http_url(&server_url)?;
        return Ok(ProviderConfig::Xtream { server_url, username, password });
    }
    let url = m3u_url.ok_or_else(|| {
        super::ApiError::BadRequest("need either Xtream credentials or a playlist URL".into())
    })?;
    let url = normalise_provider_url(&url);
    require_http_url(&url)?;
    Ok(ProviderConfig::M3u { url })
}

/// Reject anything that is not `http://` or `https://`.
///
/// The adapters hand this string to reqwest, which would happily be
/// pointed at a `file://` path — and the answer to "what does this
/// provider return" would then be the contents of a local file. A
/// scheme check is the whole defence needed and it belongs here, at the
/// one point a URL enters the system.
fn require_http_url(url: &str) -> Result<(), super::ApiError> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(())
    }
    else {
        Err(super::ApiError::BadRequest(
            "the URL must start with http:// or https://".into(),
        ))
    }
}

pub async fn disconnect(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, super::ApiError> {
    // No entitlement gate. Disconnecting removes credentials, and a
    // user whose subscription lapsed must still be able to do that —
    // gating it would strand their provider details in the vault.
    require_auth(&headers)?;
    let conn = lock(&state)?;
    if let Some(id) = storage::active_connection(&conn)? {
        storage::delete_connection(&conn, &id)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── /refresh ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RefreshQuery {
    /// `true` re-imports regardless of age. The default respects the
    /// TTLs, which is what a page load wants — a visit should not cost a
    /// full playlist download.
    pub force: Option<bool>,
}

pub async fn refresh(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<RefreshQuery>,
) -> Result<Json<SyncReport>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    if q.force.unwrap_or(false) {
        return Ok(Json(sync::sync_catalog(state.premium.clone(), &id).await?));
    }
    // Nothing was stale. Report what is on disk rather than a 204: the
    // caller asked "is this current", and the counts are the answer.
    match sync::refresh_if_stale(state.premium.clone(), &id).await? {
        Some(report) => Ok(Json(report)),
        None => {
            let catalog = sync::catalog_state(&state.premium, &id)?;
            Ok(Json(SyncReport {
                categories: catalog.categories,
                channels: catalog.channels,
                programs: 0,
                epg_available: catalog.epg_synced_at.is_some(),
                synced_at: catalog.catalog_synced_at.unwrap_or(0),
            }))
        }
    }
}

// ── Catalog reads ──────────────────────────────────────────────────

pub async fn dashboard(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<PremiumDashboard>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    Ok(Json(repo(&state).build_dashboard(&id)?))
}

pub async fn categories(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<IPTVCategory>>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    Ok(Json(repo(&state).list_categories(&id)?))
}

/// The category list the sidebar renders: alphabetical, with a channel
/// count, and only groups that have channels in them.
///
/// Separate from `/categories` rather than a flag on it, because the two
/// answer different questions and a route whose response shape changes
/// with a query parameter is a route every client has to branch on.
/// `/categories` is the provider's declared group list — what a filter
/// dropdown wants, empty groups included. This one is derived from the
/// channel table, so a provider's forty empty groups are not forty rows
/// the user can click into and find nothing.
///
/// It is also not `/dashboard`'s `categories`: that field is the top 200
/// by size, for a front page, and a sidebar ordered by popularity is a
/// sidebar nobody can find anything in.
pub async fn category_counts(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ChannelsQuery>,
) -> Result<Json<Vec<CategoryCount>>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    Ok(Json(repo(&state).category_counts(&id, q.hide_adult.unwrap_or(false))?))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsQuery {
    pub cursor: Option<String>,
    pub category: Option<String>,
    pub country: Option<String>,
    pub search: Option<String>,
    pub favorites_only: Option<bool>,
    pub hide_adult: Option<bool>,
    pub limit: Option<usize>,
}

pub async fn channels(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ChannelsQuery>,
) -> Result<Json<IPTVChannelPage>, super::ApiError> {
    guard(&state, &headers)?;
    let id = active_connection(&state)?;
    let page = repo(&state).query_channels(
        &id,
        q.category.as_deref(),
        q.country.as_deref(),
        q.search.as_deref(),
        q.favorites_only.unwrap_or(false),
        q.hide_adult.unwrap_or(false),
        q.cursor.as_deref(),
        q.limit.unwrap_or(DEFAULT_PAGE),
    )?;
    Ok(Json(page))
}

pub async fn channel(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<IPTVChannel>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    repo(&state)
        .channel_by_id(&cid, &id)?
        .map(Json)
        .ok_or_else(|| super::ApiError::NotFound("channel not found".into()))
}

// ── EPG ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct EpgQuery {
    pub limit: Option<usize>,
}

/// One channel's guide.
///
/// An empty result triggers exactly one per-channel fetch, and only for
/// the channel being asked about. That is the fallback for a provider
/// with no bulk XMLTV — the case where the catalog imported fine and the
/// guide is simply not available in bulk. It is bounded to this one
/// channel on purpose: the same call across a catalog is thousands of
/// requests and a rate-limit, which is why `sync` never does it.
///
/// A failed fallback is not a failed request. The guide is an
/// enhancement over a working stream, so the error is logged and the
/// response is an empty list — which the UI renders as "no guide"
/// rather than as a broken panel.
pub async fn epg(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EpgQuery>,
) -> Result<Json<Vec<EpgProgram>>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    let limit = q.limit.unwrap_or(DEFAULT_EPG_LIMIT).clamp(1, MAX_EPG_LIMIT);
    let r = repo(&state);
    let cached = r.read_epg(&cid, &id, limit)?;
    if !cached.is_empty() {
        return Ok(Json(cached));
    }
    match sync::sync_channel_epg(state.premium.clone(), &cid, &id).await {
        Ok(0) => Ok(Json(Vec::new())),
        Ok(_) => Ok(Json(r.read_epg(&cid, &id, limit)?)),
        Err(e) => {
            eprintln!("[premium-api] per-channel EPG for {id} failed: {e}");
            Ok(Json(Vec::new()))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowNextRequest {
    pub channel_ids: Vec<String>,
}

/// Now-and-next for a page of channels in one request.
///
/// The grid draws a "now playing" line under every card. Per-card
/// requests would be sixty round trips per page and a visibly staggered
/// grid; this is one query, two rows per channel. No fallback fetch
/// here — that is per-channel by nature, and a batch is the one place it
/// must never happen.
pub async fn epg_now_next(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<NowNextRequest>,
) -> Result<Json<Vec<EpgProgram>>, super::ApiError> {
    guard(&state, &headers)?;
    if req.channel_ids.len() > MAX_NOW_NEXT_IDS {
        return Err(super::ApiError::BadRequest(format!(
            "at most {MAX_NOW_NEXT_IDS} channel ids per request"
        )));
    }
    let cid = active_connection(&state)?;
    Ok(Json(repo(&state).epg_now_next(&cid, &req.channel_ids)?))
}

// ── Playback ───────────────────────────────────────────────────────

/// Mint a playback source: a redirector URL, the two upstream headers
/// the player should send, and when the URL stops working.
///
/// The upstream URL is not built here and never reaches the client.
pub async fn play(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<PlaybackSource>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    let source = player::build_source(state.premium.clone(), &cid, &id)?;
    Ok(Json(source))
}

/// Find quality variants for a channel — other channels with a similar
/// base name but different quality labels. Returns an empty list when
/// no variants exist.
pub async fn quality_variants(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<IPTVChannel>>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    Ok(Json(repo(&state).quality_variants(&cid, &id)?))
}

// ── Favourites and history ─────────────────────────────────────────

pub async fn favorites(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<IPTVChannel>>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    Ok(Json(repo(&state).favorite_channels(&cid, 200)?))
}

pub async fn toggle_favorite(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<FavoriteResponse>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    // The new state, not 204. A toggle whose caller has to guess the
    // result is a toggle that shows the wrong star whenever two
    // requests race or one is retried.
    let is_favorite = repo(&state).toggle_favorite(&cid, &id)?;
    Ok(Json(FavoriteResponse { is_favorite }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteResponse {
    pub is_favorite: bool,
}

pub async fn recent(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<IPTVChannel>>, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    Ok(Json(repo(&state).recent_channels(&cid, 20)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRecentRequest {
    pub channel_id: String,
}

pub async fn add_recent(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddRecentRequest>,
) -> Result<StatusCode, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    repo(&state).add_recent(&cid, &req.channel_id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_recent(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, super::ApiError> {
    guard(&state, &headers)?;
    let cid = active_connection(&state)?;
    repo(&state).clear_recent(&cid)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── The redirector ─────────────────────────────────────────────────

/// `/premium-stream/:token` — the only route a player opens, and the
/// only place the upstream URL exists.
///
/// The token is a JWT naming the connection and channel, signed by this
/// process's key and valid for thirty seconds. Verifying it *is* the
/// authorization: a player cannot send an `Authorization` header, so the
/// proof has to be in the path. The entitlement is checked as well, and
/// that is not redundant — the token outlives a revocation by up to its
/// TTL, and this is where that window is closed.
///
/// The upstream URL is asked of the adapter (`resolve_stream_url`),
/// which is the only code that knows how its protocol builds one. For
/// Xtream that means the password appears in this function's `Location`
/// header and nowhere else: not in the response body, not in a log line,
/// not in the token, and not in anything the client can read.
pub async fn stream_redirect(
    State(state): State<ApiState>,
    Path(token): Path<String>,
) -> Result<Response, super::ApiError> {
    ensure_premium(&state)?;
    let (connection_id, channel_id) = player::resolve_redirector_token(&state.premium, &token)?
        .ok_or_else(|| super::ApiError::NotFound("stream not found".into()))?;

    // The stored URL first, because for an M3U it is the answer and no
    // provider needs constructing to give it. Scoped so the guard is
    // dropped before the await below — a `MutexGuard` held across one
    // would make this handler non-`Send`, and axum would reject it.
    let stored = {
        let conn = lock(&state)?;
        storage::stream_row(&conn, &connection_id, &channel_id)?
            .ok_or_else(|| super::ApiError::NotFound("channel not found".into()))?
    };

    let upstream = match stored.stream_url.filter(|u| !u.is_empty()) {
        Some(url) => url,
        None => {
            let provider = factory::provider_for(state.premium.clone(), &connection_id)?;
            provider
                .resolve_stream_url(&channel_id)
                .await?
                .ok_or_else(|| super::ApiError::NotFound("stream not available".into()))?
        }
    };

    // A bare 302. The upstream headers are not set here: a header on a
    // redirect describes the redirect, not the request the client makes
    // to the `Location` it names — so they travel in the `PlaybackSource`
    // for the player to apply to the request that actually reaches the
    // provider.
    Ok(Redirect::temporary(&upstream).into_response())
}

// ── Helpers ────────────────────────────────────────────────────────

fn lock(
    state: &ApiState,
) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, super::ApiError> {
    state
        .premium
        .db
        .lock()
        .map_err(|e| super::ApiError::Internal(format!("lock: {e}")))
}
