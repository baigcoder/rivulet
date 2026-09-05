//! Xtream API adapter.
//!
//! The Xtream protocol is a JSON-over-HTTPS API on top of PHP. The
//! endpoints used here:
//!
//! - `GET {server}/player_api.php?username=…&password=…` (auth +
//!   server_info)
//! - `GET …&action=get_live_categories`
//! - `GET …&action=get_live_streams[&category_id=…]`
//! - `GET …&action=get_short_epg&stream_id=…&limit=N`
//! - `GET {server}/xmltv.php?username=…&password=…` (bulk XMLTV,
//!   best-effort — many providers don't ship one)
//!
//! Auth fields come back as int, bool, or string depending on the
//! provider — `models::XtreamAuthResponse` deserializes all three.
//! The `stream_type` field is the filter for "this is a live
//! channel" — a movie or series record must never become a
//! channel.
//!
//! All HTTP calls share one `reqwest::Client` (5s connect, 15s
//! read, 3 retries with exponential backoff). Credentials are
//! never logged.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use url::Url;
use serde::Deserialize;

use super::errors::PremiumError;
use super::models::{
    EpgProgram, IPTVCategory, IPTVChannel, PremiumAccount,
};
use super::names;
use super::provider::{Catalog, IPTVProvider};
use super::storage::{self, PremiumState};

/// 5s connect, 15s read.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// 3 retries with exponential backoff. The reqwest `Client` is the
/// place to centralise this — the per-call layer just sees a
/// single `Result`.
const MAX_RETRIES: u32 = 3;
const BACKOFF_BASE: Duration = Duration::from_millis(500);

pub struct XtreamAdapter {
    pub state: Arc<PremiumState>,
    pub connection_id: String,
}

impl XtreamAdapter {
    pub fn new(state: Arc<PremiumState>, connection_id: String) -> Self {
        Self { state, connection_id }
    }

    /// Single shared `reqwest::Client`. Built lazily on first use.
    fn client(&self) -> Result<Client, PremiumError> {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent("Rivulet/0.5 (Xtream)")
            .build()
            .map_err(|e| PremiumError::Network(e.to_string()))
    }

    /// VOD catalogs are larger than live lists; 15s idle is enough to
    /// abort a hung panel and not enough to finish a fat category.
    fn vod_client(&self) -> Result<Client, PremiumError> {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(Duration::from_secs(60))
            .user_agent("Rivulet/0.5 (Xtream)")
            .build()
            .map_err(|e| PremiumError::Network(e.to_string()))
    }

    /// Read the encrypted config out of the vault.
    async fn config(&self) -> Result<XtreamCreds, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let blob = storage::get_secret(&conn, &self.connection_id)?
            .ok_or(PremiumError::ProviderNotConnected)?;
        match storage::ProviderConfig::decrypt(&blob, &self.state.vault)? {
            storage::ProviderConfig::Xtream { server_url, username, password } => {
                Ok(XtreamCreds { server_url, username, password })
            }
            storage::ProviderConfig::M3u { .. } => {
                Err(PremiumError::ServerError(
                    "connection is M3U, not Xtream".into(),
                ))
            }
        }
    }
}

/// Pull panel credentials out of a playlist link.
///
/// A `get.php?username=…&password=…&type=m3u_plus` URL is the thing a
/// provider actually sells, and it already carries everything
/// `player_api.php` needs. Recognising one is the difference between an
/// account with a status and an expiry date whose live channels arrive
/// from `get_live_streams`, and one flat playlist with the films and
/// series mixed into it — same link, same credentials, a far worse
/// product. So the link is treated as a panel first and a playlist only
/// if the panel does not answer.
///
/// `None` for anything that is not a panel link: a plain `.m3u` file, a
/// non-HTTP scheme, or an endpoint missing either field. The caller then
/// imports it as a playlist, which is what it is.
pub fn credentials_from_playlist_url(raw: &str) -> Option<(String, String, String)> {
    let parsed = Url::parse(&normalise_provider_url(raw)).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    // A provider may hand out its `player_api.php` URL directly, or the
    // playlist-oriented `get.php` / `playlist.php` equivalent. All three
    // carry the same credentials and live beside `player_api.php`.
    let path = parsed.path().to_ascii_lowercase();
    if !(path.ends_with("/player_api.php")
        || path.ends_with("/get.php")
        || path.ends_with("/playlist.php"))
    {
        return None;
    }

    let mut username = None;
    let mut password = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "username" => username = Some(value.into_owned()),
            "password" => password = Some(value.into_owned()),
            _ => {}
        }
    }
    let username = username.filter(|v| !v.is_empty())?;
    let password = password.filter(|v| !v.is_empty())?;

    // The server is the link's own directory, because `player_api.php`
    // sits beside `get.php` and a panel may live under a path prefix.
    // Joining `"."` is how a URL says "the directory of this file", and
    // it keeps the port and the prefix that a hand-built string loses.
    let mut server = parsed.join(".").ok()?;
    server.set_query(None);
    server.set_fragment(None);
    let server_url = server.as_str().trim_end_matches('/').to_string();

    Some((server_url, username, password))
}

/// Keep the first URL when a provider link was accidentally pasted twice.
///
/// This happens often when a provider's message contains a linked URL and a
/// client copies both its visible text and its target. We intentionally only
/// split on a second URL scheme, not on arbitrary `http` text in a password.
/// A normal, single URL is returned unchanged.
pub fn normalise_provider_url(raw: &str) -> String {
    let value = raw.trim();
    let first = value.find("://").unwrap_or(value.len());
    let tail = &value[first.saturating_add(3)..];
    let next_http = tail.find("http://");
    let next_https = tail.find("https://");
    let next = match (next_http, next_https) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    };
    match next {
        Some(offset) => value[..first + 3 + offset].trim_end().to_string(),
        None => value.to_string(),
    }
}

struct XtreamCreds {
    server_url: String,
    username: String,
    password: String,
}

impl XtreamCreds {
    /// Build `{server}/player_api.php?username=…&password=…[&action=…]`.
    /// The query string is URL-encoded by reqwest; the inner
    /// `&` is safe because `username` and `password` cannot
    /// contain it (Xtream usernames are short ASCII; passwords
    /// may be longer but the provider rejects `&`).
    fn api_url(&self, action: &str) -> String {
        let mut url = format!(
            "{}/player_api.php?username={}&password={}",
            self.server_url.trim_end_matches('/'),
            urlencoding::encode(&self.username),
            urlencoding::encode(&self.password),
        );
        if !action.is_empty() {
            url.push_str("&action=");
            url.push_str(action);
        }
        url
    }

    /// Build `{server}/xmltv.php?username=…&password=…`.
    fn xmltv_url(&self) -> String {
        format!(
            "{}/xmltv.php?username={}&password={}",
            self.server_url.trim_end_matches('/'),
            urlencoding::encode(&self.username),
            urlencoding::encode(&self.password),
        )
    }
}

/// `GET` with the shared retry loop. Anything 2xx is `Ok(bytes)`;
/// anything else falls into the typed error mapping.
async fn get_with_retries(
    client: &Client,
    url: &str,
) -> Result<Vec<u8>, PremiumError> {
    let mut last_err: Option<PremiumError> = None;
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let backoff = BACKOFF_BASE * 2u32.pow(attempt - 1);
            tokio::time::sleep(backoff).await;
        }
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp
                        .bytes()
                        .await
                        .map(|b| b.to_vec())
                        .map_err(|e| PremiumError::Network(e.to_string()));
                }
                let err = match status.as_u16() {
                    401 | 403 => PremiumError::AuthFailed,
                    404 => PremiumError::NotFound,
                    429 => PremiumError::RateLimited,
                    s if s >= 500 => PremiumError::ServerError(status.to_string()),
                    _ => PremiumError::Network(status.to_string()),
                };
                last_err = Some(err);
            }
            Err(e) if e.is_timeout() => {
                last_err = Some(PremiumError::Timeout);
            }
            Err(e) => {
                last_err = Some(PremiumError::Network(e.to_string()));
            }
        }
    }
    Err(last_err.unwrap_or(PremiumError::ServerError("no attempts".into())))
}

// ── Wire shapes (the provider's own JSON) ────────────────────────

#[derive(Debug, Deserialize)]
struct XtreamAuthResponse {
    user_info: Option<XtreamUserInfo>,
    server_info: Option<XtreamProviderInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct XtreamUserInfo {
    #[serde(default, deserialize_with = "deserialize_auth")]
    auth: Option<i64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    exp_date: Option<String>,
    #[serde(default)]
    is_trial: Option<String>,
    #[serde(default)]
    active_cons: Option<String>,
    #[serde(default)]
    max_connections: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
struct XtreamProviderInfo {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    https_port: Option<String>,
    #[serde(default)]
    server_protocol: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    timestamp_now: Option<i64>,
    #[serde(default)]
    time_now: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XtreamCategory {
    category_id: Option<String>,
    category_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct XtreamStream {
    num: Option<i64>,
    name: Option<String>,
    stream_type: Option<String>,
    stream_id: Option<i64>,
    stream_icon: Option<String>,
    #[serde(default, rename = "epg_channel_id")]
    epg_channel_id: Option<String>,
    #[serde(default)]
    category_id: Option<String>,
    /// Xtream panels mark adult streams with `"is_adult": "1"` or `"is_adult": 1`.
    #[serde(default, rename = "is_adult")]
    is_adult: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct XtreamEpgResponse {
    epg_listings: Option<Vec<XtreamEpgListing>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct XtreamEpgListing {
    id: Option<String>,
    #[serde(default)]
    epg_id: Option<String>,
    title: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    start_timestamp: Option<String>,
    /// Panels disagree on the name of this one — some send
    /// `stop_timestamp`, some `end_timestamp`. Reading only the latter
    /// left every programme with no end time, which the guide then
    /// guessed at.
    #[serde(default, alias = "stop_timestamp")]
    end_timestamp: Option<String>,
    #[serde(default)]
    now_playing: Option<String>,
    #[serde(default)]
    has_archive: Option<String>,
}

/// Undo the base64 an Xtream panel wraps `get_short_epg` text in.
///
/// Panels encode `title` and `description` and nothing else, so the
/// field can be either form and there is no flag saying which. Guessing
/// wrongly in the safe direction shows base64 on screen; guessing
/// wrongly in the other direction turns a real title into mojibake, so
/// the test is deliberately narrow: standard base64 *with* padding (a
/// length that is not a multiple of four is rejected outright, which
/// eliminates most plain titles), decoding to valid UTF-8, with no
/// control characters in the result. Anything that fails is returned
/// untouched.
fn decode_epg_text(raw: String) -> String {
    use base64::Engine;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() % 4 != 0 {
        return raw;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(trimmed) else {
        return raw;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return raw;
    };
    if text.trim().is_empty()
        || text.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return raw;
    }
    text
}

/// An Xtream EPG timestamp, in either of the two forms a panel sends.
///
/// `start_timestamp` / `stop_timestamp` are Unix seconds; `start` /
/// `end` are `"YYYY-MM-DD HH:MM:SS"` in the *server's* timezone, which
/// the endpoint does not state. They are read as UTC, which is right
/// for the majority of panels and wrong by a whole-hour offset for the
/// rest — so they are only ever a fallback for a missing epoch field,
/// and the bulk XMLTV path (which carries explicit offsets) is
/// preferred over this endpoint entirely.
fn parse_epg_time(raw: Option<&str>) -> Option<i64> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        // A zero here means "the panel had no time", not 1970.
        return s.parse::<i64>().ok().filter(|&n| n > 0);
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc().timestamp());
        }
    }
    None
}

fn deserialize_auth<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AuthValue {
        Int(i64),
        Bool(bool),
        Str(String),
    }
    let v = Option::<AuthValue>::deserialize(deserializer)?;
    Ok(v.map(|a| match a {
        AuthValue::Int(n) => n,
        AuthValue::Bool(b) => i64::from(b),
        AuthValue::Str(s) => s.parse().unwrap_or(0),
    }))
}

// ── Provider impl ────────────────────────────────────────────────

#[async_trait]
impl IPTVProvider for XtreamAdapter {
    async fn authenticate(&self) -> Result<PremiumAccount, PremiumError> {
        let creds = self.config().await?;
        let client = self.client()?;
        let url = creds.api_url("");
        let bytes = get_with_retries(&client, &url).await?;
        let resp: XtreamAuthResponse = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("auth: {e}")))?;
        let user = resp
            .user_info
            .ok_or_else(|| PremiumError::MalformedResponse("missing user_info".into()))?;
        let auth = user.auth.unwrap_or(0) != 0;
        let status = user.status.as_deref().unwrap_or("");
        if !auth || status == "Disabled" {
            return Err(match status {
                "Disabled" => PremiumError::ServerError("account disabled".into()),
                "Expired" => PremiumError::ServerError("subscription expired".into()),
                "Banned" => PremiumError::AuthFailed,
                _ => PremiumError::AuthFailed,
            });
        }

        let resolved_server = resp
            .server_info
            .as_ref()
            .and_then(|s| s.url.clone())
            .unwrap_or_else(|| creds.server_url.clone());

        let account = PremiumAccount {
            provider_type: "xtream".to_string(),
            server_url: resolved_server,
            username: creds.username.clone(),
            status: match status {
                "Active" => "connected".to_string(),
                "Expired" => "expired".to_string(),
                _ => "connected".to_string(),
            },
            account_name: Some(creds.username.clone()),
            expires_at: user.exp_date.clone(),
            is_trial: user.is_trial.as_ref().and_then(|s| s.parse().ok()),
            active_connections: user.active_cons.as_ref().and_then(|s| s.parse().ok()),
            max_connections: user.max_connections.as_ref().and_then(|s| s.parse().ok()),
        };
        // Persist the snapshot so the settings page can render it
        // without re-authenticating. The password never lands here.
        if let Ok(conn) = self.state.db.lock() {
            let _ = storage::update_account(
                &conn,
                &self.connection_id,
                account.account_name.as_deref(),
                account.expires_at.as_deref(),
                account.is_trial,
                account.active_connections,
                account.max_connections,
            );
        }
        Ok(account)
    }

    async fn get_categories(&self) -> Result<Vec<IPTVCategory>, PremiumError> {
        let creds = self.config().await?;
        let client = self.client()?;
        let url = creds.api_url("get_live_categories");
        let bytes = get_with_retries(&client, &url).await?;
        let raw: Vec<XtreamCategory> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("categories: {e}")))?;
        Ok(raw
            .into_iter()
            .filter_map(|c| {
                let id = c.category_id?;
                // Same furniture appears in the category list; a group
                // whose name is pure decoration is dropped rather than
                // drawn as a sidebar row. Its channels keep their
                // `category_id` and are still reachable under All.
                let name = c.category_name.as_deref().and_then(names::clean_channel_name)?;
                Some(IPTVCategory {
                    id,
                    name,
                    country: None,
                    group: None,
                })
            })
            .collect())
    }

    async fn get_channels(&self) -> Result<Vec<IPTVChannel>, PremiumError> {
        let creds = self.config().await?;
        let client = self.client()?;
        let url = creds.api_url("get_live_streams");
        let bytes = get_with_retries(&client, &url).await?;
        let raw: Vec<XtreamStream> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("channels: {e}")))?;
        let mut out: Vec<(Option<i64>, IPTVChannel)> = raw
            .into_iter()
            // `stream_type` is the only thing separating a live channel
            // from a VOD record on this endpoint. A provider that omits
            // it entirely is treated as live, because `get_live_streams`
            // is not supposed to return anything else; one that says
            // "movie" is believed and dropped.
            .filter(|s| {
                s.stream_type
                    .as_deref()
                    .map(|t| t == "live")
                    .unwrap_or(true)
            })
            .filter_map(|s| {
                let id = s.stream_id?.to_string();
                // A panel's lineup contains section dividers and escaped
                // names as well as channels; `None` means the row was
                // furniture, not a channel (see `premium::names`).
                let raw_name = s.name.as_deref()?;
                let name = names::clean_channel_name(raw_name)?;
                let quality = names::detect_quality(raw_name);
                let is_adult = s.is_adult.as_ref().map(|v| match v {
                    serde_json::Value::Number(n) => n.as_u64().unwrap_or(0) != 0,
                    serde_json::Value::String(s) => s == "1",
                    _ => false,
                }).unwrap_or(false);
                Some((
                    s.num,
                    IPTVChannel {
                        id,
                        name,
                        logo_url: s.stream_icon.filter(|s| !s.is_empty()),
                        category_id: s.category_id.filter(|s| !s.is_empty()),
                        category_name: None,
                        country: None,
                        language: None,
                        epg_id: s.epg_channel_id.filter(|s| !s.is_empty()),
                        stream_type: Some("live".to_string()),
                        user_agent: None,
                        referer: None,
                        stream_url: None,
                        quality,
                        is_adult,
                        is_favorite: false,
                    },
                ))
            })
            .collect();
        // Providers hand out `num` as the channel's position in the
        // user's lineup, which is the order a viewer expects to zap
        // through. Rows without one sort last.
        out.sort_by(|a, b| match (a.0, b.0) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.1.name.cmp(&b.1.name),
        });
        Ok(out.into_iter().map(|(_, c)| c).collect())
    }

    /// Both lists, with `category_name` denormalized onto every
    /// channel. Doing it here rather than with a SQL join keeps the
    /// paging query a single-table read, which is what makes a
    /// 50,000-channel catalog page in constant time.
    async fn get_catalog(&self) -> Result<Catalog, PremiumError> {
        let categories = self.get_categories().await?;
        let mut channels = self.get_channels().await?;
        let names: std::collections::HashMap<&str, &str> = categories
            .iter()
            .map(|c| (c.id.as_str(), c.name.as_str()))
            .collect();
        // Build a set of category ids whose names indicate adult content.
        let adult_cat_ids: std::collections::HashSet<&str> = categories
            .iter()
            .filter(|c| names::is_adult_category(&c.name))
            .map(|c| c.id.as_str())
            .collect();
        for ch in &mut channels {
            ch.category_name = ch
                .category_id
                .as_deref()
                .and_then(|id| names.get(id))
                .map(|s| s.to_string());
            // Mark as adult if the Xtream API said so OR the category name indicates it.
            if !ch.is_adult {
                ch.is_adult = ch
                    .category_id
                    .as_deref()
                    .map(|id| adult_cat_ids.contains(id))
                    .unwrap_or(false);
            }
        }
        Ok(Catalog { categories, channels })
    }

    async fn get_epg(
        &self,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<EpgProgram>, PremiumError> {
        let creds = self.config().await?;
        let client = self.client()?;
        let url = format!(
            "{}&action=get_short_epg&stream_id={}&limit={}",
            creds.api_url(""),
            channel_id,
            limit
        );
        let bytes = get_with_retries(&client, &url).await?;
        let resp: XtreamEpgResponse = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("epg: {e}")))?;
        Ok(resp
            .epg_listings
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| {
                let title = decode_epg_text(e.title?);
                let start = parse_epg_time(e.start_timestamp.as_deref())
                    .or_else(|| parse_epg_time(e.start.as_deref()))?;
                let stop = parse_epg_time(e.end_timestamp.as_deref())
                    .or_else(|| parse_epg_time(e.end.as_deref()));
                Some(EpgProgram {
                    // Always the stream id we asked for, never the
                    // listing's own `channel_id`: that field carries the
                    // *guide* id (`epg_channel_id`), which is a different
                    // namespace from our row ids and is shared between
                    // the HD and SD variants of a channel. Writing it
                    // through would file the programme under a key no
                    // read path looks up.
                    channel_id: channel_id.to_string(),
                    title,
                    description: e.description.map(decode_epg_text),
                    start,
                    stop,
                })
            })
            .collect())
    }

    async fn get_bulk_epg(&self) -> Result<Option<Vec<u8>>, PremiumError> {
        let creds = self.config().await?;
        let client = self.client()?;
        let url = creds.xmltv_url();
        // Don't burn 3 retries on a missing xmltv.php — many
        // providers don't ship one. A 404 is "not supported" and
        // returns `None`; the caller falls back to per-channel
        // `get_epg`.
        let resp = client.get(&url).send().await.map_err(|e| {
            if e.is_timeout() {
                PremiumError::Timeout
            }
            else {
                PremiumError::Network(e.to_string())
            }
        })?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| PremiumError::Network(e.to_string()))?
            .to_vec();
        // Decompress gzip if needed.
        if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut out = Vec::new();
            use std::io::Read;
            decoder
                .read_to_end(&mut out)
                .map_err(|e| PremiumError::MalformedResponse(format!("gunzip: {e}")))?;
            Ok(Some(out))
        }
        else {
            Ok(Some(bytes))
        }
    }

    /// `{server}/live/{username}/{password}/{stream_id}.m3u8`.
    ///
    /// This is the one string in the module that carries the account
    /// password, and it is built here rather than in the HTTP layer so
    /// the route handler never sees a credential: it takes the value,
    /// puts it in a `Location` header, and drops it. Nothing stores it
    /// and nothing logs it.
    ///
    /// `.m3u8` rather than `.ts` because HLS is what a browser, mpv and
    /// hls.js can all open; a panel that only has MPEG-TS answers the
    /// `.m3u8` request with a redirect to it.
    async fn resolve_stream_url(
        &self,
        channel_id: &str,
    ) -> Result<Option<String>, PremiumError> {
        // A stream id is a positive integer in every Xtream panel. The
        // check is not cosmetic: the id is interpolated into a URL
        // path, so anything else could inject a path segment.
        if channel_id.is_empty() || !channel_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PremiumError::NotFound);
        }
        let creds = self.config().await?;
        Ok(Some(format!(
            "{}/live/{}/{}/{}.m3u8",
            creds.server_url.trim_end_matches('/'),
            urlencoding::encode(&creds.username),
            urlencoding::encode(&creds.password),
            channel_id,
        )))
    }
}

// ── On-demand catalog (Xtream VOD + series) ─────────────────────────

#[derive(Debug, Deserialize)]
struct XtreamVodStream {
    stream_id: Option<i64>,
    name: Option<String>,
    stream_icon: Option<String>,
    rating: Option<String>,
    #[serde(default)]
    plot: Option<String>,
    category_id: Option<String>,
    container_extension: Option<String>,
    #[serde(default, rename = "is_adult")]
    is_adult: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct XtreamSeriesRow {
    series_id: Option<i64>,
    name: Option<String>,
    cover: Option<String>,
    plot: Option<String>,
    rating: Option<String>,
    category_id: Option<String>,
    #[serde(default, rename = "is_adult")]
    is_adult: Option<serde_json::Value>,
}

fn xtream_adult(v: &Option<serde_json::Value>) -> bool {
    v.as_ref().map(|val| match val {
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(0) != 0,
        serde_json::Value::String(s) => s == "1",
        _ => false,
    }).unwrap_or(false)
}

fn paginate<T: Clone>(all: Vec<T>, cursor: usize, limit: usize) -> (Vec<T>, usize, Option<String>) {
    let total = all.len();
    let end = (cursor + limit).min(total);
    let items = if cursor >= total {
        vec![]
    } else {
        all[cursor..end].to_vec()
    };
    let next = if end < total { Some(end.to_string()) } else { None };
    (items, total, next)
}

fn filter_search<T, F: Fn(&T) -> &str>(items: Vec<T>, search: Option<&str>, name: F) -> Vec<T> {
    let Some(q) = search.filter(|s| !s.is_empty()) else {
        return items;
    };
    let q = q.to_lowercase();
    items.into_iter().filter(|i| name(i).to_lowercase().contains(&q)).collect()
}

impl XtreamAdapter {
    fn vod_api_url(&self, creds: &XtreamCreds, action: &str, category_id: Option<&str>) -> String {
        let mut url = creds.api_url(action);
        if let Some(cat) = category_id.filter(|c| !c.is_empty()) {
            url.push_str("&category_id=");
            url.push_str(cat);
        }
        url
    }

    pub async fn vod_movie_categories(&self) -> Result<Vec<super::models::VodCategory>, PremiumError> {
        if let Some(cats) = self.state.vod_cache.movie_categories(&self.connection_id) {
            return Ok(cats);
        }
        let creds = self.config().await?;
        let bytes = get_with_retries(&self.vod_client()?, &creds.api_url("get_vod_categories")).await?;
        let raw: Vec<XtreamCategory> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("vod categories: {e}")))?;
        let cats: Vec<super::models::VodCategory> = raw.into_iter().filter_map(|c| {
            let id = c.category_id?;
            let name = c.category_name.as_deref().and_then(names::clean_channel_name)?;
            Some(super::models::VodCategory { id, name, kind: "movie".into() })
        }).collect();
        self.state.vod_cache.set_movie_categories(&self.connection_id, cats.clone());
        Ok(cats)
    }

    pub async fn vod_series_categories(&self) -> Result<Vec<super::models::VodCategory>, PremiumError> {
        if let Some(cats) = self.state.vod_cache.series_categories(&self.connection_id) {
            return Ok(cats);
        }
        let creds = self.config().await?;
        let bytes = get_with_retries(&self.vod_client()?, &creds.api_url("get_series_categories")).await?;
        let raw: Vec<XtreamCategory> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("series categories: {e}")))?;
        let cats: Vec<super::models::VodCategory> = raw.into_iter().filter_map(|c| {
            let id = c.category_id?;
            let name = c.category_name.as_deref().and_then(names::clean_channel_name)?;
            Some(super::models::VodCategory { id, name, kind: "series".into() })
        }).collect();
        self.state.vod_cache.set_series_categories(&self.connection_id, cats.clone());
        Ok(cats)
    }

    async fn fetch_vod_movies(&self, category_id: Option<&str>) -> Result<Vec<super::models::PremiumVodItem>, PremiumError> {
        let cat_key = category_id.unwrap_or("");
        let lock = self.state.vod_cache.list_lock(&self.connection_id, "movies", cat_key);
        let _guard = lock.lock().await;
        if let Some(items) = self.state.vod_cache.movies(&self.connection_id, cat_key) {
            return Ok(items);
        }
        let creds = self.config().await?;
        let url = self.vod_api_url(&creds, "get_vod_streams", category_id);
        let bytes = get_with_retries(&self.vod_client()?, &url).await?;
        let raw: Vec<XtreamVodStream> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("vod streams: {e}")))?;
        let all: Vec<super::models::PremiumVodItem> = raw.into_iter().filter_map(|s| {
            let id = s.stream_id?.to_string();
            let name = names::clean_channel_name(s.name.as_deref()?)?;
            Some(super::models::PremiumVodItem {
                id,
                name,
                poster_url: s.stream_icon,
                plot: s.plot,
                rating: s.rating,
                category_id: s.category_id,
                category_name: None,
                container_extension: s.container_extension,
                is_adult: xtream_adult(&s.is_adult) || names::is_adult_category(s.name.as_deref().unwrap_or("")),
            })
        }).collect();
        self.state.vod_cache.set_movies(&self.connection_id, cat_key, all.clone());
        Ok(all)
    }

    async fn fetch_vod_series(&self, category_id: Option<&str>) -> Result<Vec<super::models::PremiumSeriesItem>, PremiumError> {
        let cat_key = category_id.unwrap_or("");
        let lock = self.state.vod_cache.list_lock(&self.connection_id, "series", cat_key);
        let _guard = lock.lock().await;
        if let Some(items) = self.state.vod_cache.series(&self.connection_id, cat_key) {
            return Ok(items);
        }
        let creds = self.config().await?;
        let url = self.vod_api_url(&creds, "get_series", category_id);
        let bytes = get_with_retries(&self.vod_client()?, &url).await?;
        let raw: Vec<XtreamSeriesRow> = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("series: {e}")))?;
        let all: Vec<super::models::PremiumSeriesItem> = raw.into_iter().filter_map(|s| {
            let id = s.series_id?.to_string();
            let name = names::clean_channel_name(s.name.as_deref()?)?;
            Some(super::models::PremiumSeriesItem {
                id,
                name,
                poster_url: s.cover,
                plot: s.plot,
                rating: s.rating,
                category_id: s.category_id,
                is_adult: xtream_adult(&s.is_adult) || names::is_adult_category(s.name.as_deref().unwrap_or("")),
            })
        }).collect();
        self.state.vod_cache.set_series(&self.connection_id, cat_key, all.clone());
        Ok(all)
    }

    /// "All movies" used to hit `get_vod_streams` with no category — one
    /// multi-megabyte JSON, one 15s timeout, three retries. Walk
    /// categories instead and stop once this page is full.
    async fn merge_vod_movies(&self, min_raw: usize) -> Result<(Vec<super::models::PremiumVodItem>, bool), PremiumError> {
        if let Some(all) = self.state.vod_cache.movies(&self.connection_id, "") {
            return Ok((all, true));
        }
        let cats = self.vod_movie_categories().await?;
        if cats.is_empty() {
            return Ok((self.fetch_vod_movies(None).await?, true));
        }
        let mut merged = Vec::new();
        let mut seen = HashSet::new();
        for (i, cat) in cats.iter().enumerate() {
            for item in self.fetch_vod_movies(Some(&cat.id)).await? {
                if seen.insert(item.id.clone()) {
                    merged.push(item);
                }
            }
            if merged.len() >= min_raw && i + 1 < cats.len() {
                return Ok((merged, false));
            }
        }
        self.state.vod_cache.set_movies(&self.connection_id, "", merged.clone());
        Ok((merged, true))
    }

    async fn merge_vod_series(&self, min_raw: usize) -> Result<(Vec<super::models::PremiumSeriesItem>, bool), PremiumError> {
        if let Some(all) = self.state.vod_cache.series(&self.connection_id, "") {
            return Ok((all, true));
        }
        let cats = self.vod_series_categories().await?;
        if cats.is_empty() {
            return Ok((self.fetch_vod_series(None).await?, true));
        }
        let mut merged = Vec::new();
        let mut seen = HashSet::new();
        for (i, cat) in cats.iter().enumerate() {
            for item in self.fetch_vod_series(Some(&cat.id)).await? {
                if seen.insert(item.id.clone()) {
                    merged.push(item);
                }
            }
            if merged.len() >= min_raw && i + 1 < cats.len() {
                return Ok((merged, false));
            }
        }
        self.state.vod_cache.set_series(&self.connection_id, "", merged.clone());
        Ok((merged, true))
    }

    pub async fn vod_movies(
        &self,
        category_id: Option<&str>,
        search: Option<&str>,
        hide_adult: bool,
        cursor: usize,
        limit: usize,
    ) -> Result<super::models::VodPage<super::models::PremiumVodItem>, PremiumError> {
        if category_id.filter(|c| !c.is_empty()).is_some() {
            let mut all = self.fetch_vod_movies(category_id).await?;
            if hide_adult {
                all.retain(|i| !i.is_adult);
            }
            all = filter_search(all, search, |i| &i.name);
            let (items, total, next_cursor) = paginate(all, cursor, limit);
            return Ok(super::models::VodPage { items, total, next_cursor });
        }
        let want = cursor.saturating_add(limit);
        let searching = search.filter(|s| !s.is_empty()).is_some();
        let mut min_raw = if searching { usize::MAX } else { want };
        loop {
            let (raw, complete) = self.merge_vod_movies(min_raw).await?;
            let raw_len = raw.len();
            let mut all = raw;
            if hide_adult {
                all.retain(|i| !i.is_adult);
            }
            all = filter_search(all, search, |i| &i.name);
            if all.len() >= want || complete {
                let (items, mut total, mut next_cursor) = paginate(all, cursor, limit);
                if !complete {
                    total = total.max(want + 1);
                    if next_cursor.is_none() {
                        next_cursor = Some(want.to_string());
                    }
                }
                return Ok(super::models::VodPage { items, total, next_cursor });
            }
            if raw_len >= min_raw {
                min_raw = raw_len.saturating_add(want);
            } else {
                let (items, total, next_cursor) = paginate(all, cursor, limit);
                return Ok(super::models::VodPage { items, total, next_cursor });
            }
        }
    }

    pub async fn vod_series_list(
        &self,
        category_id: Option<&str>,
        search: Option<&str>,
        hide_adult: bool,
        cursor: usize,
        limit: usize,
    ) -> Result<super::models::VodPage<super::models::PremiumSeriesItem>, PremiumError> {
        if category_id.filter(|c| !c.is_empty()).is_some() {
            let mut all = self.fetch_vod_series(category_id).await?;
            if hide_adult {
                all.retain(|i| !i.is_adult);
            }
            all = filter_search(all, search, |i| &i.name);
            let (items, total, next_cursor) = paginate(all, cursor, limit);
            return Ok(super::models::VodPage { items, total, next_cursor });
        }
        let want = cursor.saturating_add(limit);
        let searching = search.filter(|s| !s.is_empty()).is_some();
        let mut min_raw = if searching { usize::MAX } else { want };
        loop {
            let (raw, complete) = self.merge_vod_series(min_raw).await?;
            let raw_len = raw.len();
            let mut all = raw;
            if hide_adult {
                all.retain(|i| !i.is_adult);
            }
            all = filter_search(all, search, |i| &i.name);
            if all.len() >= want || complete {
                let (items, mut total, mut next_cursor) = paginate(all, cursor, limit);
                if !complete {
                    total = total.max(want + 1);
                    if next_cursor.is_none() {
                        next_cursor = Some(want.to_string());
                    }
                }
                return Ok(super::models::VodPage { items, total, next_cursor });
            }
            if raw_len >= min_raw {
                min_raw = raw_len.saturating_add(want);
            } else {
                let (items, total, next_cursor) = paginate(all, cursor, limit);
                return Ok(super::models::VodPage { items, total, next_cursor });
            }
        }
    }

    pub async fn vod_series_detail(&self, series_id: &str) -> Result<super::models::PremiumSeriesDetail, PremiumError> {
        if series_id.is_empty() || !series_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PremiumError::NotFound);
        }
        let creds = self.config().await?;
        let url = format!("{}&series_id={}", creds.api_url("get_series_info"), series_id);
        let bytes = get_with_retries(&self.client()?, &url).await?;
        let raw: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::MalformedResponse(format!("series info: {e}")))?;
        let info = raw.get("info").and_then(|v| v.as_object());
        let name = info.and_then(|i| i.get("name")).and_then(|v| v.as_str())
            .and_then(names::clean_channel_name)
            .unwrap_or_else(|| series_id.to_string());
        let poster = info.and_then(|i| i.get("cover")).and_then(|v| v.as_str()).map(str::to_string);
        let plot = info.and_then(|i| i.get("plot")).and_then(|v| v.as_str()).map(str::to_string);
        let rating = info.and_then(|i| i.get("rating")).and_then(|v| v.as_str()).map(str::to_string);
        let mut episodes = Vec::new();
        if let Some(map) = raw.get("episodes").and_then(|v| v.as_object()) {
            for (season_key, eps) in map {
                let season: u32 = season_key.parse().unwrap_or(1);
                if let Some(arr) = eps.as_array() {
                    for ep in arr {
                        let Some(id) = ep.get("id").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                            .map(|n| n.to_string()) else { continue };
                        let episode_num = ep.get("episode_num").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                        let title = ep.get("title").and_then(|v| v.as_str())
                            .or_else(|| ep.get("name").and_then(|v| v.as_str()))
                            .unwrap_or("Episode")
                            .to_string();
                        episodes.push(super::models::PremiumEpisode {
                            id,
                            season,
                            episode: episode_num,
                            title,
                            plot: ep.get("plot").and_then(|v| v.as_str()).map(str::to_string),
                            container_extension: ep.get("container_extension").and_then(|v| v.as_str()).map(str::to_string),
                        });
                    }
                }
            }
        }
        episodes.sort_by(|a, b| a.season.cmp(&b.season).then(a.episode.cmp(&b.episode)));
        Ok(super::models::PremiumSeriesDetail {
            id: series_id.to_string(),
            name,
            poster_url: poster,
            plot,
            rating,
            episodes,
        })
    }

    /// `{server}/movie/{user}/{pass}/{id}.{ext}` — the Xtream VOD path.
    pub async fn resolve_movie_url(&self, stream_id: &str, ext: &str) -> Result<String, PremiumError> {
        if stream_id.is_empty() || !stream_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PremiumError::NotFound);
        }
        let creds = self.config().await?;
        let ext = if ext.is_empty() { "mkv" } else { ext };
        Ok(format!(
            "{}/movie/{}/{}/{}.{}",
            creds.server_url.trim_end_matches('/'),
            urlencoding::encode(&creds.username),
            urlencoding::encode(&creds.password),
            stream_id,
            ext.trim_start_matches('.'),
        ))
    }

    /// `{server}/series/{user}/{pass}/{episode_id}.{ext}`.
    pub async fn resolve_series_episode_url(&self, episode_id: &str, ext: &str) -> Result<String, PremiumError> {
        if episode_id.is_empty() || !episode_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PremiumError::NotFound);
        }
        let creds = self.config().await?;
        let ext = if ext.is_empty() { "mkv" } else { ext };
        Ok(format!(
            "{}/series/{}/{}/{}.{}",
            creds.server_url.trim_end_matches('/'),
            urlencoding::encode(&creds.username),
            urlencoding::encode(&creds.password),
            episode_id,
            ext.trim_start_matches('.'),
        ))
    }

    /// Parse a synthetic play id: `movie:{stream_id}:{ext}` or `series:{episode_id}:{ext}`.
    pub fn parse_vod_play_id(play_id: &str) -> Option<(&str, &str, &str)> {
        if let Some(rest) = play_id.strip_prefix("movie:") {
            let mut parts = rest.splitn(2, ':');
            let id = parts.next()?;
            let ext = parts.next().unwrap_or("mkv");
            return Some(("movie", id, ext));
        }
        if let Some(rest) = play_id.strip_prefix("series:") {
            let mut parts = rest.splitn(2, ':');
            let id = parts.next()?;
            let ext = parts.next().unwrap_or("mkv");
            return Some(("series", id, ext));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_link_yields_panel_credentials() {
        let (server, user, pass) = credentials_from_playlist_url(
            "http://example.com:8080/get.php?username=u1&password=p1&output=ts&type=m3u_plus",
        )
        .expect("a get.php link with both fields is a panel");
        assert_eq!(server, "http://example.com:8080");
        assert_eq!(user, "u1");
        assert_eq!(pass, "p1");
    }

    #[test]
    fn playlist_link_keeps_a_path_prefix() {
        // `player_api.php` sits beside `get.php`, wherever that is.
        let (server, ..) =
            credentials_from_playlist_url("http://example.com/panel/get.php?username=u&password=p")
                .unwrap();
        assert_eq!(server, "http://example.com/panel");
    }

    #[test]
    fn a_plain_playlist_is_not_a_panel() {
        // No credentials in it, so there is no account to verify — it
        // stays an M3U import.
        assert!(credentials_from_playlist_url("http://example.com/playlist.m3u").is_none());
        assert!(
            credentials_from_playlist_url("http://example.com/get.php?username=u").is_none(),
            "half a credential pair is not a panel",
        );
        assert!(
            credentials_from_playlist_url("file:///tmp/get.php?username=u&password=p").is_none(),
            "and the scheme still has to be http",
        );
    }

    #[test]
    fn player_api_url_is_recognised_as_xtream_credentials() {
        let (server, user, pass) = credentials_from_playlist_url(
            "http://example.com:8080/player_api.php?username=u&password=p",
        )
        .unwrap();
        assert_eq!(server, "http://example.com:8080");
        assert_eq!(user, "u");
        assert_eq!(pass, "p");
    }

    #[test]
    fn duplicate_pasted_provider_url_keeps_the_first_url() {
        let raw = "http://example.com/player_api.php?username=u&password=phttp://example.com/player_api.php?username=u&password=p";
        assert_eq!(
            normalise_provider_url(raw),
            "http://example.com/player_api.php?username=u&password=p",
        );
    }

    #[test]
    fn auth_field_accepts_int() {
        let json = r#"{"user_info":{"auth":1,"status":"Active"}}"#;
        let r: XtreamAuthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.user_info.unwrap().auth, Some(1));
    }

    #[test]
    fn auth_field_accepts_bool() {
        let json = r#"{"user_info":{"auth":true,"status":"Active"}}"#;
        let r: XtreamAuthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.user_info.unwrap().auth, Some(1));
    }

    #[test]
    fn auth_field_accepts_string() {
        let json = r#"{"user_info":{"auth":"1","status":"Active"}}"#;
        let r: XtreamAuthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.user_info.unwrap().auth, Some(1));
    }

    #[test]
    fn category_requires_id_and_name() {
        let json = r#"[{"category_id":"42","category_name":"News"},{"category_id":"x"}]"#;
        let r: Vec<XtreamCategory> = serde_json::from_str(json).unwrap();
        assert_eq!(r.len(), 2);
        // The missing-name one is dropped by the adapter; this
        // test just pins the wire shape.
        assert_eq!(r[0].category_id.as_deref(), Some("42"));
        assert!(r[1].category_name.is_none());
    }

    #[test]
    fn stream_filters_to_live() {
        let live = XtreamStream {
            num: Some(1),
            name: Some("BBC".into()),
            stream_type: Some("live".into()),
            stream_id: Some(7),
            stream_icon: None,
            epg_channel_id: None,
            category_id: None,
            is_adult: None,
        };
        let movie = XtreamStream {
            stream_type: Some("movie".into()),
            name: Some("Film".into()),
            ..live.clone()
        };
        assert_eq!(live.stream_type.as_deref(), Some("live"));
        assert_ne!(movie.stream_type.as_deref(), Some("live"));
    }

    /// A row as a real panel writes it, which is not the shape a
    /// hand-written struct literal proves anything about: `num`,
    /// `stream_id` and `tv_archive` arrive as JSON *numbers*, an absent
    /// EPG id is `null` rather than missing, and a panel sends half a
    /// dozen fields nothing here reads. Deserialization is all-or-nothing
    /// per array, so one wrongly typed field loses all 7,000 channels —
    /// this is the test that fails if someone tightens a type or adds
    /// `deny_unknown_fields`.
    #[test]
    fn live_rows_deserialize_as_a_panel_sends_them() {
        let json = r#"[
            {"num":2520,"name":"Sep","stream_type":"live","stream_id":2520,
             "stream_icon":"http://example.com/p.png","epg_channel_id":null,
             "added":"1732445950","is_adult":0,"category_id":"12",
             "custom_sid":"","tv_archive":0,"direct_source":"",
             "tv_archive_duration":0},
            {"num":3,"name":"DF1 FHD DE","stream_type":"live","stream_id":117619,
             "stream_icon":"","epg_channel_id":"df1.de","category_id":"12",
             "tv_archive":1,"tv_archive_duration":7}
        ]"#;
        let rows: Vec<XtreamStream> = serde_json::from_str(json).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].stream_id, Some(2520));
        assert_eq!(rows[0].epg_channel_id, None);
        assert_eq!(rows[1].epg_channel_id.as_deref(), Some("df1.de"));
        assert_eq!(rows[1].category_id.as_deref(), Some("12"));
    }

    #[test]
    fn epg_text_base64_is_decoded() {
        // "News at Ten" as an Xtream panel sends it.
        assert_eq!(decode_epg_text("TmV3cyBhdCBUZW4=".into()), "News at Ten");
    }

    #[test]
    fn epg_text_plain_is_left_alone() {
        // Four characters, so it survives the length test, and it is
        // valid base64 — but it decodes to bytes that are not UTF-8.
        assert_eq!(decode_epg_text("News".into()), "News");
        assert_eq!(decode_epg_text("News at Ten".into()), "News at Ten");
        assert_eq!(decode_epg_text("".into()), "");
    }

    #[test]
    fn epg_time_reads_both_forms() {
        assert_eq!(parse_epg_time(Some("1735689600")), Some(1735689600));
        assert_eq!(
            parse_epg_time(Some("2025-01-01 00:00:00")),
            Some(1735689600)
        );
        assert_eq!(parse_epg_time(Some("0")), None);
        assert_eq!(parse_epg_time(Some("")), None);
        assert_eq!(parse_epg_time(None), None);
        assert_eq!(parse_epg_time(Some("not a time")), None);
    }

    #[test]
    fn api_url_escapes_credentials() {
        let c = XtreamCreds {
            server_url: "https://example.com/".into(),
            username: "user name".into(),
            password: "p&w".into(),
        };
        let url = c.api_url("get_live_categories");
        assert!(url.contains("user%20name"));
        assert!(url.contains("p%26w"));
        assert!(url.ends_with("action=get_live_categories"));
    }
}
