//! The Tauri commands that bootstrap the Premium API.
//!
//! Two things cannot arrive over HTTP, and both are here.
//!
//! The **bearer token** cannot: a route that handed out the token that
//! authorizes its own routes would authorize nothing. So the frontend
//! asks for it over Tauri IPC, which is reachable only from this app's
//! own webview, and presents it on every HTTP request afterwards.
//!
//! The **entitlement** cannot, for the same reason turned around: the
//! HTTP API is on loopback, and loopback is reachable by every process
//! on the machine. A `POST /entitlement` would let any of them grant
//! itself Premium TV. Over IPC only this webview can, and what it sends
//! is the subscription state it already holds in its settings store.
//!
//! Neither command touches provider credentials. The vault is opened by
//! `PremiumState` and read only inside the adapters.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use super::auth;
use super::entitlement::{EntitlementState, SubscriptionInfo};

/// The bearer token and when it stops being accepted, so the frontend
/// can re-mint before a long session expires rather than after a 401.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenResponse {
    pub token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

/// Mint a bearer token for the local Premium API.
///
/// Callable at any time and idempotent in effect — each call returns a
/// fresh hour-long token signed by the same keychain key, so a frontend
/// that lost its copy (a reload, a cleared `sessionStorage`) just asks
/// again.
#[tauri::command]
pub fn premium_api_token() -> Result<ApiTokenResponse, String> {
    let (token, expires_at) = auth::mint_api_token().map_err(|e| {
        // `ApiError`'s conversion produces the frontend-safe message;
        // the token path has no provider credentials in scope at all.
        crate::premium::errors::PremiumError::from(e).to_string()
    })?;
    Ok(ApiTokenResponse { token, expires_at })
}

/// Push the current subscription state to the API's gate.
///
/// Called at boot and on every subscription change. Anything other than
/// `tier: "premium"` closes the gate, and so does a past `expiresAtMs` —
/// the check is in `SubscriptionInfo::is_premium`, not here, so the
/// frontend cannot express "premium but expired" and have it read as
/// allowed.
#[tauri::command]
pub fn premium_set_entitlement(
    state: State<'_, Arc<EntitlementState>>,
    tier: String,
    expires_at_ms: Option<i64>,
) -> SubscriptionInfo {
    state.set(SubscriptionInfo { tier, expires_at_ms });
    // Echo what the gate now holds rather than returning nothing. It is
    // the only way the frontend can confirm the push landed, and a
    // mismatch between the settings store and the gate is exactly the
    // bug that would otherwise present as "Premium TV says I'm not
    // premium".
    state.get()
}

/// What the gate currently holds. For the settings panel and for
/// diagnosing the mismatch above.
#[tauri::command]
pub fn premium_entitlement(state: State<'_, Arc<EntitlementState>>) -> SubscriptionInfo {
    state.get()
}
