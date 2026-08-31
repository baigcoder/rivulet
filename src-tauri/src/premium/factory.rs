//! The one place a concrete adapter is chosen.
//!
//! Everything above this line — the sync module, the route handlers,
//! the redirector — talks to `dyn IPTVProvider` and never names
//! `XtreamAdapter` or `M3uAdapter`. That is what the spec's "the UI
//! must never depend on raw provider-specific response formats" comes
//! down to in practice: a third provider is a new file plus one arm of
//! the `match` below, and nothing else in the crate changes.
//!
//! The provider *type* is read from the connection row, not passed in
//! by the caller. A caller that could pick would be a caller that
//! could pick wrong — asking an Xtream adapter to decrypt an M3U
//! config fails at the vault, which is a confusing place to find out.

use std::sync::Arc;

use super::errors::PremiumError;
use super::m3u::M3uAdapter;
use super::provider::IPTVProvider;
use super::storage::{self, PremiumState};
use super::xtream::XtreamAdapter;

/// Build the adapter for a connection.
///
/// `PremiumError::ProviderNotConnected` when the id names no row;
/// `ServerError` when the row's `provider_type` is a string this build
/// doesn't know — which can only happen to a database written by a
/// *newer* build, so it is a downgrade, not corruption.
pub fn provider_for(
    state: Arc<PremiumState>,
    connection_id: &str,
) -> Result<Box<dyn IPTVProvider>, PremiumError> {
    let kind = {
        let conn = state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        storage::provider_type(&conn, connection_id)?
    };
    match kind.as_str() {
        "xtream" => Ok(Box::new(XtreamAdapter::new(
            state,
            connection_id.to_string(),
        ))),
        "m3u" => Ok(Box::new(M3uAdapter::new(state, connection_id.to_string()))),
        other => Err(PremiumError::ServerError(format!(
            "unknown provider type '{other}'"
        ))),
    }
}
