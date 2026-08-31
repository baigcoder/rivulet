//! The provider abstraction that hides Xtream and M3U behind one
//! shape. Every method here is something the sync module or the HTTP
//! API calls; nothing in here is a Tauri command.
//!
//! **Bulk, not per-channel.** `get_catalog` fetches the full category
//! and channel lists in one go, and the result is written to SQLite.
//! Everything the UI does afterwards — paging, search, filtering,
//! favourites — reads that table, never the provider. A provider is
//! contacted when the user connects, when they refresh, and when a
//! stream is opened; nothing else.
//!
//! Per-channel EPG has its own method, but it is a *fallback* for
//! providers with no bulk XMLTV endpoint. It costs one HTTP request
//! per channel, so it is only ever called for the channel being
//! watched — never in a loop over a catalog.
//!
//! The trait is async; the implementations own their own `reqwest`
//! client, timeouts, and retry loop.

use async_trait::async_trait;

use super::errors::PremiumError;
use super::models::{
    EpgProgram, IPTVCategory, IPTVChannel, PremiumAccount,
};

/// Both halves of a catalog import. Returned together because for an
/// M3U they come from a single pass over one download — asking for
/// categories and then channels would fetch a multi-hundred-megabyte
/// playlist twice.
pub struct Catalog {
    pub categories: Vec<IPTVCategory>,
    pub channels: Vec<IPTVChannel>,
}

#[async_trait]
pub trait IPTVProvider: Send + Sync {
    /// Run the auth handshake. The `ProviderConfig` (Xtream creds or
    /// M3U URL) was set at construction time and lives in the vault;
    /// the adapter just re-loads it.
    async fn authenticate(&self) -> Result<PremiumAccount, PremiumError>;

    /// All live categories, in one call.
    async fn get_categories(&self) -> Result<Vec<IPTVCategory>, PremiumError>;

    /// All live channels, in one call.
    async fn get_channels(&self) -> Result<Vec<IPTVChannel>, PremiumError>;

    /// Both lists at once. The default runs the two calls above, which
    /// is right for a provider with separate endpoints (Xtream);
    /// an adapter whose two lists come from one document overrides it.
    async fn get_catalog(&self) -> Result<Catalog, PremiumError> {
        let categories = self.get_categories().await?;
        let channels = self.get_channels().await?;
        Ok(Catalog { categories, channels })
    }

    /// Per-channel EPG, the next `limit` programs. The fallback for
    /// providers that don't ship a bulk XMLTV. Returning an empty
    /// list is *not* an error — "no EPG" is a valid answer.
    async fn get_epg(&self, channel_id: &str, limit: usize) -> Result<Vec<EpgProgram>, PremiumError>;

    /// Bulk EPG, when the provider has one (Xtream `xmltv.php`, or an
    /// M3U's `x-tvg-url` header). Returns the raw gzipped or plain
    /// XMLTV bytes; `sync` parses them. `None` means "not supported by
    /// this provider — fall back to per-channel `get_epg`".
    async fn get_bulk_epg(&self) -> Result<Option<Vec<u8>>, PremiumError> {
        Ok(None)
    }

    /// The upstream URL for a channel, resolved at request time.
    ///
    /// Only the redirector calls this, and only with the channel the
    /// signed token names. It is separate from the catalog because for
    /// Xtream the URL contains the account password: it is built here,
    /// used once as a `Location` header, and never stored or returned
    /// to the client. An adapter whose catalog already carries a
    /// per-channel URL (M3U) returns `None` and lets the caller use
    /// the stored one.
    async fn resolve_stream_url(
        &self,
        channel_id: &str,
    ) -> Result<Option<String>, PremiumError>;
}
