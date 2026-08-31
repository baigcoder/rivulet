use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveChannel {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epg_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_type: Option<String>,
    /// ISO 3166-1 alpha-2 code from the iptv-org country API (e.g. "PK").
    /// Lets the UI look up the flag without re-parsing the M3U.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    /// Flag emoji from the iptv-org country API (e.g. "🇵🇰").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_flag: Option<String>,
    /// `http-user-agent` from a #EXTVLCOPT line. Many iptv-org streams
    /// reject the webview's default UA; forwarding the one the playlist
    /// asks for is what makes them play.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// `http-referrer` from a #EXTVLCOPT line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveCategory {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpgProgram {
    pub channel_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub start: String,
    pub stop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IptvAccount {
    pub provider_type: String,
    pub server_url: String,
    pub username: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_trial: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_connections: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct FavoriteEntry {
    pub channel_id: String,
    pub provider_id: String,
}

/// Xtream API auth response (raw from provider).
#[derive(Debug, Clone, Deserialize)]
pub struct XtreamAuthResponse {
    pub user_info: Option<XtreamUserInfo>,
    #[allow(dead_code)]
    pub server_info: Option<XtreamProviderInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct XtreamUserInfo {
    #[serde(default, deserialize_with = "deserialize_auth")]
    pub auth: Option<i64>,
    pub status: Option<String>,
    pub exp_date: Option<String>,
    pub is_trial: Option<String>,
    pub active_cons: Option<String>,
    pub created_at: Option<String>,
    pub max_connections: Option<String>,
    pub username: Option<String>,
    pub message: Option<String>,
    pub allowed_output_formats: Option<Vec<String>>,
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

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct XtreamProviderInfo {
    pub url: Option<String>,
    pub port: Option<String>,
    pub https_port: Option<String>,
    pub server_protocol: Option<String>,
    pub rtmp_port: Option<String>,
    pub timezone: Option<String>,
    pub timestamp_now: Option<i64>,
    pub time_now: Option<String>,
    #[serde(default)]
    pub process: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct XtreamCategory {
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_int_option")]
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct XtreamStream {
    pub num: Option<i64>,
    pub name: Option<String>,
    pub stream_type: Option<String>,
    pub stream_id: Option<i64>,
    pub stream_icon: Option<String>,
    #[serde(rename = "epg_channel_id")]
    pub epg_channel_id: Option<String>,
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default, deserialize_with = "deserialize_int_string")]
    pub is_adult: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub category_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub custom_sid: Option<String>,
    #[serde(default, deserialize_with = "deserialize_int_option")]
    pub tv_archive: Option<i64>,
    #[serde(default)]
    pub direct_source: Option<String>,
    #[serde(default, deserialize_with = "deserialize_int_option")]
    pub tv_archive_duration: Option<i64>,
}

/// Deserialize a value that might be an int, bool, or string into an Option<String>.
#[allow(dead_code)]
fn deserialize_int_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexibleValue {
        Int(i64),
        Bool(bool),
        Str(String),
    }

    let v = Option::<FlexibleValue>::deserialize(deserializer)?;
    Ok(v.map(|a| match a {
        FlexibleValue::Int(n) => n.to_string(),
        FlexibleValue::Bool(b) => b.to_string(),
        FlexibleValue::Str(s) => s,
    }))
}

/// Deserialize a value that might be an int or null into Option<i64>.
#[allow(dead_code)]
fn deserialize_int_option<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FlexibleInt {
        Int(i64),
        Str(String),
    }

    let v = Option::<FlexibleInt>::deserialize(deserializer)?;
    Ok(v.and_then(|a| match a {
        FlexibleInt::Int(n) => Some(n),
        FlexibleInt::Str(s) => s.parse().ok(),
    }))
}

#[derive(Debug, Clone, Deserialize)]
pub struct XtreamEpgResponse {
    pub epg_listings: Option<Vec<XtreamEpgListing>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct XtreamEpgListing {
    pub id: Option<String>,
    pub epg_id: Option<String>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub description: Option<String>,
    pub channel_id: Option<String>,
    pub start_timestamp: Option<String>,
    pub end_timestamp: Option<String>,
    pub now_playing: Option<String>,
    pub has_archive: Option<String>,
}
