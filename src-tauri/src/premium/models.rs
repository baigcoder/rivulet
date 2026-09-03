use serde::{Deserialize, Serialize};

/// A provider category. The same shape regardless of whether Xtream
/// or M3U produced it — adapters normalize before this is constructed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IPTVCategory {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// A live channel from the user's Premium provider. The same shape
/// regardless of provider — adapters normalize before this is
/// constructed. `stream_type` is kept raw ("live" / "movie" / etc.) so
/// the frontend can filter further if it ever needs to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IPTVChannel {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
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
    /// From #EXTVLCOPT:http-user-agent=. Forwarded to the stream
    /// proxy so the upstream sees the right UA.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// From #EXTVLCOPT:http-referrer=. Same reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    /// The upstream stream URL, when the provider hands out one per
    /// channel. M3U does; Xtream constructs it from the credentials at
    /// request time and leaves this `None`.
    ///
    /// `skip_serializing` rather than `skip_serializing_if`, because
    /// the client must never receive it: an M3U line routinely carries
    /// a per-account token, and a channel page is the one response
    /// large enough for that to leak unnoticed. The redirector reads
    /// this column server-side instead.
    #[serde(skip_serializing, default)]
    pub stream_url: Option<String>,
    /// Quality label inferred from the channel name (e.g. "4K UHD", "FHD",
    /// "HD"). `None` when the name carries no recognisable quality token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    /// Whether the channel belongs to an adult category. Derived from
    /// the Xtream `is_adult` flag and category name patterns during
    /// catalog sync. The frontend can filter these behind a toggle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_adult: bool,
    /// Whether the channel is in the user's favourites. The repository
    /// fills this in so a grid can draw its stars without fetching the
    /// favourite list alongside every page.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpgProgram {
    pub channel_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Unix timestamp (seconds since epoch).
    pub start: i64,
    /// Unix timestamp (seconds since epoch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<i64>,
}

/// What `build_playback_source` returns. `url` is the *raw* upstream
/// URL the player will open (or a short-lived redirector URL pointing
/// at it, see `player.rs`); `mime_type` lets the frontend pick a
/// `<source type>` without sniffing; `expires_at` is when the URL
/// stops being valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSource {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Unix epoch milliseconds. `None` means "no expiry".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// The `User-Agent` the upstream expects, when the playlist named
    /// one. The player hands it to mpv as `--user-agent=`. A header set
    /// on the redirect response cannot do this job: a header on a 302
    /// says nothing about the request the client makes next.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Same, for `Referer` (mpv's `--referrer=`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    /// Quality label from the channel name (e.g. "4K UHD", "FHD", "HD").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
}

/// The connected provider's account snapshot. The server URL and
/// username are *not* secrets on their own — the password is in the
/// `CredentialVault`, not here. This is what the dashboard and the
/// settings page render.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumAccount {
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

/// A page of channels for the dashboard. Xtream and M3U providers
/// have no real cursor pagination — we fetch the whole list once,
/// store it in SQLite, and paginate locally. `next_cursor` mirrors
/// the Free TV shape so the same infinite-scroll component works.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IPTVChannelPage {
    pub items: Vec<IPTVChannel>,
    pub total: usize,
    /// Opaque to the client. `None` when the last page is reached.
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryCount {
    pub name: String,
    pub count: usize,
}

/// The dashboard bundle: top-line counts, the previews the front
/// page renders (favorites, recent, per-country, per-category), and
/// the active source id. The whole bundle is what `/api/premium-tv/dashboard`
/// returns; it lives in memory once loaded and is invalidated when
/// the user disconnects or switches providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumDashboard {
    pub source_id: String,
    pub total_channels: usize,
    pub country_count: usize,
    pub category_count: usize,
    pub categories: Vec<CategoryCount>,
    pub countries: Vec<CountryCount>,
    pub favorite_previews: Vec<IPTVChannel>,
    pub recent_previews: Vec<IPTVChannel>,
    pub country_previews: Vec<PremiumChannelPreview>,
    pub category_previews: Vec<PremiumChannelPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumChannelPreview {
    pub name: String,
    pub count: usize,
    pub channels: Vec<IPTVChannel>,
}

/// What a catalog or EPG import ended up doing. Returned by
/// `/connect` and `/refresh` so the UI can say "1,284 channels in 37
/// groups" instead of just "done", and can tell an EPG-less provider
/// apart from a failed EPG fetch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub categories: usize,
    pub channels: usize,
    pub programs: usize,
    /// `false` when the provider ships no guide at all — which is
    /// normal, and is not an error the UI should shout about.
    pub epg_available: bool,
    /// Unix seconds. When the catalog was last written.
    pub synced_at: i64,
}

/// The connection's freshness, as `/status` reports it. The UI uses
/// this to decide whether to offer a refresh, and to show an empty
/// catalog as "still importing" rather than "no channels".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    pub channels: usize,
    pub categories: usize,
    /// Unix seconds, or `None` when the catalog has never imported.
    pub catalog_synced_at: Option<i64>,
    pub epg_synced_at: Option<i64>,
    /// True while an import is running in the background.
    pub syncing: bool,
}

// ── On-demand (Xtream VOD / series) ────────────────────────────────
// Fetched live from the panel — not cached in SQLite yet. The same
// pagination shape as channels keeps one infinite-scroll component.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VodCategory {
    pub id: String,
    pub name: String,
    /// `movie` or `series` — which Xtream action produced the row.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumVodItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_extension: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_adult: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumSeriesItem {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_adult: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumEpisode {
    pub id: String,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PremiumSeriesDetail {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    pub episodes: Vec<PremiumEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VodPage<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub next_cursor: Option<String>,
}

