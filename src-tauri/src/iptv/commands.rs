//! Tauri commands for the Free TV IPTV subsystem.
//!
//! The query surface here is what the frontend talks to. The previous
//! design loaded the whole channel list into Vue; this one keeps the
//! dataset in SQLite and returns a 60-row page plus the total and a
//! cursor. Every list page goes through one of the `live_*` commands
//! below, and the page itself is what lives in the frontend.
//!
//! `IptvState` is the only managed Tauri state the rest of the crate
//! touches. The DB lives in a `Mutex<Connection>`. The streaming
//! importer opens the same file in a separate connection (WAL keeps
//! them in sync).
//!
//! `IptvCommandError` is what the frontend sees. Every error string is
//! safe to render — none of the variants contain user credentials.
//!
//! **Premium TV (Xtream / user-added M3U) has moved to a separate
//! `premium/` module and a local HTTP API at 127.0.0.1:3032.** Nothing
//! in this file touches the premium code path; the Free TV proxy on
//! :3031 and the built-in iptv-org import are all that's left.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::categories::Category;
use super::countries::Country;
use super::db;
use super::errors::IptvError;
use super::models::EpgProgram;
use super::sources;
use super::streaming_m3u;

/// The proxy port the free-TV stream proxy listens on.
pub const PROXY_PORT: u16 = 3031;

/// Shared Free-TV state managed by Tauri. One per process, lives for
/// the whole run. The premium path no longer lives here — the new
/// `premium/` module owns its own connection and its own state.
pub struct IptvState {
    /// SQLite connection. The single source of truth for the free
    /// channel list, EPG cache, favorites, recent and source registry.
    /// Wrapped in a `Mutex` because rusqlite's `Connection` is `!Sync`.
    pub db: Arc<db::Mutex<Connection>>,
    /// Path to the on-disk DB file. Captured at construction so the
    /// streaming importer (which needs an owned `Connection` to cross
    /// `await` points) can open the same file and share the WAL.
    pub db_path: PathBuf,
    /// Active free-TV import's cancellation token, if any.
    /// `live_cancel_import` flips it; the importer polls it between
    /// chunks.
    pub import_cancel: std::sync::Mutex<Option<CancellationToken>>,
}

impl IptvState {
    /// Open a fresh state. The DB is opened at `db_path`; the
    /// import-cancel slot starts empty. The path itself is stored so
    /// the streaming importer can open the same file in a separate
    /// connection.
    pub fn open(db_path: &Path) -> Result<Self, IptvError> {
        let arc = db::open(db_path)?;
        let conn = Arc::try_unwrap(arc)
            .ok()
            .and_then(|m| m.into_inner().ok())
            .ok_or_else(|| IptvError::Database("failed to unwrap connection".into()))?;
        Ok(Self {
            db: Arc::new(db::Mutex::new(conn)),
            db_path: db_path.to_path_buf(),
            import_cancel: std::sync::Mutex::new(None),
        })
    }
}

fn map_err(e: IptvError) -> String {
    e.to_string()
}

fn poison_err<T>(_: std::sync::PoisonError<T>) -> IptvError {
    IptvError::Database("mutex poisoned".into())
}

// ── Stream proxy URL (Free TV only) ────────────────────────────────

#[tauri::command]
pub fn proxy_free_stream_url(
    url: String,
    user_agent: Option<String>,
    referer: Option<String>,
) -> String {
    let mut qs = format!("url={}", urlencoding::encode(&url));
    if let Some(ua) = user_agent.filter(|s| !s.is_empty()) {
        qs.push_str("&X-Rivulet-Ua=");
        qs.push_str(&urlencoding::encode(&ua));
    }
    if let Some(rf) = referer.filter(|s| !s.is_empty()) {
        qs.push_str("&X-Rivulet-Referer=");
        qs.push_str(&urlencoding::encode(&rf));
    }
    format!("http://127.0.0.1:{PROXY_PORT}/stream?{qs}")
}

#[tauri::command]
pub fn iptv_proxy_health() -> bool {
    true
}

// ── Source registry ─────────────────────────────────────────────────

#[tauri::command]
pub async fn live_list_sources(state: State<'_, IptvState>) -> Result<Vec<db::SourceRow>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::list_sources(&conn).map_err(map_err)
}

#[tauri::command]
pub async fn live_active_source(
    state: State<'_, IptvState>,
) -> Result<Option<db::SourceRow>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::get_active_source(&conn).map_err(map_err)
}

#[tauri::command]
pub async fn live_set_active(state: State<'_, IptvState>, id: String) -> Result<(), String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::activate_source(&conn, &id).map_err(map_err)
}

#[tauri::command]
pub async fn live_remove_source(state: State<'_, IptvState>, id: String) -> Result<(), String> {
    // Refuse to remove the built-in free source. The frontend hides the
    // button for it; this is a belt-and-braces.
    if id == sources::FREE_TV_SOURCE_ID {
        return Err("Cannot remove the built-in Free TV source.".into());
    }
    let conn = state.db.lock().map_err(map_err)?;
    db::delete_source(&conn, &id).map_err(map_err)
}

// ── Dashboard ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn live_dashboard(
    state: State<'_, IptvState>,
    source_id: String,
) -> Result<db::Dashboard, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::build_dashboard(&conn, &source_id).map_err(map_err)
}

// ── Channel queries (the only thing the browser ever asks for) ──────

#[tauri::command]
pub async fn live_query_channels(
    state: State<'_, IptvState>,
    source_id: String,
    country: Option<String>,
    category: Option<String>,
    group: Option<String>,
    language: Option<String>,
    quality: Option<String>,
    search: Option<String>,
    favorites_only: Option<bool>,
    sort: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<db::ChannelPage, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::query_channels(
        &conn,
        &source_id,
        country.as_deref(),
        category.as_deref(),
        group.as_deref(),
        language.as_deref(),
        quality.as_deref(),
        search.as_deref(),
        favorites_only.unwrap_or(false),
        sort.as_deref().unwrap_or("recommended"),
        cursor.as_deref(),
        limit.unwrap_or(60),
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn live_search_channels(
    state: State<'_, IptvState>,
    source_id: String,
    query: String,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<db::ChannelPage, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::search_channels(
        &conn,
        &source_id,
        &query,
        cursor.as_deref(),
        limit.unwrap_or(60),
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn live_country_channels(
    state: State<'_, IptvState>,
    source_id: String,
    country: String,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<db::ChannelPage, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::country_channels(
        &conn,
        &source_id,
        &country,
        cursor.as_deref(),
        limit.unwrap_or(60),
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn live_category_channels(
    state: State<'_, IptvState>,
    source_id: String,
    category: String,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<db::ChannelPage, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::category_channels(
        &conn,
        &source_id,
        &category,
        cursor.as_deref(),
        limit.unwrap_or(60),
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn live_group_channels(
    state: State<'_, IptvState>,
    source_id: String,
    group: String,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<db::ChannelPage, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::group_channels(
        &conn,
        &source_id,
        &group,
        cursor.as_deref(),
        limit.unwrap_or(60),
    )
    .map_err(map_err)
}

#[tauri::command]
pub async fn live_country_stats(
    state: State<'_, IptvState>,
    source_id: String,
    limit: Option<i64>,
) -> Result<Vec<db::CountryCount>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::country_stats(&conn, &source_id, limit.unwrap_or(200)).map_err(map_err)
}

#[tauri::command]
pub async fn live_category_stats(
    state: State<'_, IptvState>,
    source_id: String,
    limit: Option<i64>,
) -> Result<Vec<db::CategoryCount>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::category_stats(&conn, &source_id, limit.unwrap_or(200)).map_err(map_err)
}

#[tauri::command]
pub async fn live_group_stats(
    state: State<'_, IptvState>,
    source_id: String,
    limit: Option<i64>,
) -> Result<Vec<db::GroupCount>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::group_stats(&conn, &source_id, limit.unwrap_or(200)).map_err(map_err)
}

// ── Channel for the player (Free TV only) ──────────────────────────

#[tauri::command]
pub async fn live_resolve_stream(
    state: State<'_, IptvState>,
    source_id: String,
    channel_id: String,
) -> Result<LiveStream, String> {
    let row = {
        let conn = state.db.lock().map_err(map_err)?;
        db::channel_by_id(&conn, &source_id, &channel_id)
            .map_err(map_err)?
            .ok_or_else(|| "channel not found".to_string())?
    };

    let stream_url = row.stream_url.clone();

    // The webview wants HLS. If the upstream is `.ts`, try the `.m3u8`
    // form through the proxy — most iptv-org providers serve both at
    // the same path.
    let proxied = if stream_url.ends_with(".ts") {
        let mut m3u8 = stream_url.clone();
        m3u8.truncate(m3u8.len() - 3);
        m3u8.push_str(".m3u8");
        proxy_free_stream_url(m3u8, row.user_agent.clone(), row.referer.clone())
    } else {
        proxy_free_stream_url(stream_url.clone(), row.user_agent.clone(), row.referer.clone())
    };
    Ok(LiveStream {
        id: row.id,
        name: row.name,
        logo_url: row.logo_url,
        stream_url: proxied,
        user_agent: row.user_agent,
        referer: row.referer,
        category_name: row.category_name,
        country: row.country,
        epg_id: row.epg_id,
    })
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiveStream {
    pub id: String,
    pub name: String,
    pub logo_url: Option<String>,
    /// The proxy URL the webview loads.
    pub stream_url: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub category_name: Option<String>,
    pub country: Option<String>,
    pub epg_id: Option<String>,
}

// ── Favorites / recent ──────────────────────────────────────────────

#[tauri::command]
pub async fn live_toggle_favorite(
    state: State<'_, IptvState>,
    source_id: String,
    channel_id: String,
) -> Result<bool, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::toggle_favorite(&conn, &source_id, &channel_id).map_err(map_err)
}

#[tauri::command]
pub async fn live_favorites(
    state: State<'_, IptvState>,
    source_id: String,
    limit: Option<i64>,
) -> Result<Vec<db::ChannelRow>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::favorite_channels(&conn, &source_id, limit.unwrap_or(60)).map_err(map_err)
}

#[tauri::command]
pub async fn live_recent(
    state: State<'_, IptvState>,
    source_id: String,
    limit: Option<i64>,
) -> Result<Vec<db::ChannelRow>, String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::recent_channels(&conn, &source_id, limit.unwrap_or(60)).map_err(map_err)
}

#[tauri::command]
pub async fn live_add_recent(
    state: State<'_, IptvState>,
    source_id: String,
    channel_id: String,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(map_err)?;
    db::upsert_recent(&conn, &source_id, &channel_id).map_err(map_err)
}

// ── EPG (Free TV: iptv-org) ────────────────────────────────────────

/// Per-channel EPG is a no-op for Free TV. Free TV uses the iptv-org
/// `get_free_tv_epg` path keyed by `tvg-id`, not a stream_id. The
/// endpoint is kept so the Free TV store's EPG path doesn't break — it
/// just returns an empty list. Premium TV's EPG is served by the new
/// HTTP API.
#[tauri::command]
pub async fn live_get_live_epg(
    _state: State<'_, IptvState>,
    _channel_id: String,
) -> Result<Vec<EpgProgram>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub async fn live_channel_epg_batch(
    _state: State<'_, IptvState>,
    _channel_ids: Vec<String>,
) -> Result<Vec<EpgProgram>, String> {
    Ok(Vec::new())
}

// ── Imports (Free TV only) ─────────────────────────────────────────

/// Re-fetch the built-in free TV source. The M3U importer runs once
/// during boot (see `lib.rs`); this command is what the frontend's
/// "Refresh" button calls.
#[tauri::command]
pub async fn live_refresh_free_tv(
    state: State<'_, IptvState>,
    app: AppHandle,
) -> Result<db::SourceRow, String> {
    let playlist_key = super::m3u::free_playlist_key();
    let source_id = sources::FREE_TV_SOURCE_ID.to_string();
    let cancel_token = CancellationToken::new();
    {
        let mut slot = state
            .import_cancel
            .lock()
            .map_err(poison_err)
            .map_err(map_err)?;
        *slot = Some(cancel_token.clone());
    }
    let app2 = app.clone();
    let db_path = state.db_path.clone();
    let sid = source_id.clone();
    let cancel = cancel_token.clone();
    let result = tauri::async_runtime::spawn(async move {
        // Same loop as the boot import: first playlist wipes, the rest append.
        for (i, (country, playlist)) in super::m3u::free_playlists().iter().enumerate() {
            streaming_m3u::stream_into_source(
                Some(&app2),
                &db_path,
                playlist,
                &sid,
                *country,
                i == 0,
                || cancel.is_cancelled(),
            )
            .await?;
        }
        Ok::<(), IptvError>(())
    })
    .await;

    {
        let mut slot = state
            .import_cancel
            .lock()
            .map_err(poison_err)
            .map_err(map_err)?;
        *slot = None;
    }

    match result {
        Ok(Ok(_)) => {
            let conn = state.db.lock().map_err(map_err)?;
            db::activate_source(&conn, &source_id).map_err(map_err)?;
            db::get_source(&conn, &source_id)
                .map_err(map_err)?
                .ok_or_else(|| "free source missing".to_string())
        }
        Ok(Err(e)) => Err(map_err(e)),
        Err(e) => Err(format!("refresh task panicked: {e}")),
    }
}

#[tauri::command]
pub async fn live_cancel_import(state: State<'_, IptvState>) -> Result<(), String> {
    let slot = state
        .import_cancel
        .lock()
        .map_err(poison_err)
        .map_err(map_err)?;
    if let Some(t) = slot.as_ref() {
        t.cancel();
    }
    Ok(())
}

// ── iptv-org reference data (unchanged surface) ─────────────────────

#[tauri::command]
pub async fn get_iptv_countries(app: AppHandle) -> Result<Vec<Country>, String> {
    let map = super::countries::fetch_countries(&app)
        .await
        .map_err(map_err)?;
    Ok(map.into_values().collect())
}

#[tauri::command]
pub async fn get_iptv_categories(app: AppHandle) -> Result<Vec<Category>, String> {
    let map = super::categories::fetch_categories(&app)
        .await
        .map_err(map_err)?;
    Ok(map.into_values().collect())
}

#[tauri::command]
pub async fn get_free_tv_epg_channel_mapping(
    app: AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    super::epg::fetch_channel_mapping(&app)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn get_free_tv_epg(app: AppHandle, tvg_id: String) -> Result<Vec<EpgProgram>, String> {
    super::epg::fetch_guide(&app, &tvg_id)
        .await
        .map_err(map_err)
}
