//! `PlaybackSource` builder for the HTTP API.
//!
//! The provider credentials never appear in a route query string, an
//! intermediate proxy log, or a player command line. `build_source`
//! returns a short-lived signed redirector URL; the player opens it;
//! the redirector resolves the real upstream URL server-side and
//! answers with a 302 to it.
//!
//! The two upstream headers travel in the `PlaybackSource`, not on the
//! redirect response. A header on a 302 describes that response — it
//! says nothing about the request the client makes to the `Location`
//! it names, which is the request the upstream actually sees. So the
//! player is told what to send, and sends it (`--user-agent=` /
//! `--referrer=` for mpv, the proxy's query parameters for the
//! webview fallback).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::errors::PremiumError;
use super::storage::PremiumState;

/// 30 seconds. Xtream stream URLs are long-lived, but a defensive
/// cap keeps a leaked URL from being useful past a few seconds.
const DEFAULT_TTL_MS: i64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedRedirect {
    pub token: String,
    pub expires_at: i64,
}

/// Mint a signed redirector token. The token is a JWT whose body
/// names the connection and channel; the signature is what the
/// `/premium-stream/:token` handler checks. The handler itself
/// lives in `src-tauri/src/api/routes_premium.rs`.
pub fn mint_redirector_token(
    state: &PremiumState,
    connection_id: &str,
    channel_id: &str,
    ttl_ms: Option<i64>,
) -> Result<SignedRedirect, PremiumError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|e| PremiumError::ServerError(format!("clock: {e}")))?;
    let ttl = ttl_ms.unwrap_or(DEFAULT_TTL_MS);
    let expires_at = now + ttl;
    let token = crate::api::auth::mint_stream_token(
        &state.vault,
        connection_id,
        channel_id,
        expires_at,
    )?;
    Ok(SignedRedirect { token, expires_at })
}

/// Resolve a redirector token back to the connection + channel it
/// was minted for. `Ok(None)` is "expired or signature doesn't match";
/// both are 404 to the player.
pub fn resolve_redirector_token(
    state: &PremiumState,
    token: &str,
) -> Result<Option<(String, String)>, PremiumError> {
    Ok(crate::api::auth::verify_stream_token(&state.vault, token)?)
}

/// Build a `PlaybackSource` for the frontend: the redirector URL,
/// not the raw upstream. This is the only shape the HTTP API
/// returns from `/api/premium-tv/channels/:id/play`.
///
/// The URL is absolute. A relative one would be resolved against the
/// document — `tauri://localhost` in the webview — and mpv, which is a
/// separate process with no document at all, could not resolve it
/// under any interpretation.
pub fn build_source(
    state: Arc<PremiumState>,
    connection_id: &str,
    channel_id: &str,
) -> Result<super::models::PlaybackSource, PremiumError> {
    // Read the headers before minting, so a channel that does not
    // exist is `NotFound` here rather than a valid-looking token the
    // redirector rejects 30 seconds later.
    let row = {
        let conn = state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        super::storage::stream_row(&conn, connection_id, channel_id)?
            .ok_or(PremiumError::NotFound)?
    };
    let redirect = mint_redirector_token(&state, connection_id, channel_id, None)?;
    Ok(super::models::PlaybackSource {
        url: format!("http://{}/premium-stream/{}", crate::api::ADDR, redirect.token),
        // Both providers are asked for HLS: Xtream's `.m3u8` endpoint,
        // and an M3U line that is overwhelmingly one. It is a hint for
        // picking a player path, not a promise — a provider that
        // redirects to MPEG-TS is mpv's and hls.js's problem to sniff,
        // and both do.
        mime_type: Some("application/x-mpegURL".to_string()),
        expires_at: Some(redirect.expires_at),
        user_agent: row.user_agent,
        referer: row.referer,
        quality: row.quality,
    })
}

/// Same redirector as live channels, but the token names a synthetic
/// play id (`movie:…` or `series:…`) that only the Xtream adapter
/// resolves. No SQLite row exists for VOD.
pub fn build_vod_source(
    state: Arc<PremiumState>,
    connection_id: &str,
    play_id: &str,
    ext: &str,
) -> Result<super::models::PlaybackSource, PremiumError> {
    let redirect = mint_redirector_token(&state, connection_id, play_id, None)?;
    let mime = vod_mime(ext);
    Ok(super::models::PlaybackSource {
        url: format!("http://{}/premium-stream/{}", crate::api::ADDR, redirect.token),
        mime_type: Some(mime),
        expires_at: Some(redirect.expires_at),
        user_agent: None,
        referer: None,
        quality: None,
    })
}

fn vod_mime(ext: &str) -> String {
    match ext.trim_start_matches('.').to_lowercase().as_str() {
        "mp4" => "video/mp4".into(),
        "mkv" => "video/x-matroska".into(),
        "avi" => "video/x-msvideo".into(),
        "m3u8" => "application/x-mpegURL".into(),
        "ts" => "video/mp2t".into(),
        _ => "video/mp4".into(),
    }
}
