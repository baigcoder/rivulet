//! Premium entitlement gate.
//!
//! **Server-side, and not a user account.** The spec's rule is that a
//! frontend check is not a check: every stream authorization has to be
//! validated by whatever serves the stream. Here that is the axum server
//! on loopback, and this is what it validates against.
//!
//! What it is *not* is an account system. This project has no server, no
//! user database and no sync (see CLAUDE.md), so there is no remote
//! authority to ask. The entitlement is a local flag: the frontend
//! pushes the subscription state it has over Tauri IPC at boot and
//! whenever it changes, and every `/api/premium-tv/*` request is checked
//! against the copy held here.
//!
//! That is a real, documented limitation rather than a papered-over one:
//! a user with a text editor can grant themselves the flag on their own
//! machine. What it does buy is the property the spec actually asks for —
//! no route serves a stream on the strength of a claim made in the
//! request. A page cannot enable Premium TV by setting a variable, a
//! stale webview cannot keep playing after the entitlement is revoked,
//! and the check lives on one side of a process boundary with the
//! credentials.
//!
//! Tauri IPC is the trusted channel: it is reachable only from this app's
//! own webview, whereas the HTTP API — even bound to loopback — is
//! reachable by any local process. So entitlement arrives by `invoke`
//! and never by an HTTP body.

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    /// `"premium"` unlocks the API. Any other value — `"free"`,
    /// `"unknown"`, an empty string — does not.
    pub tier: String,
    /// Unix epoch milliseconds. `None` is a plan with no expiry, which
    /// is why this is an `Option` and not a `0` sentinel: `0` would read
    /// as "expired in 1970" under one interpretation and "never expires"
    /// under the other, and the two are opposite answers.
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
}

impl Default for SubscriptionInfo {
    /// Denied. The default has to be the closed state — an entitlement
    /// that has not been told anything must not be premium, or a failure
    /// to deliver the flag becomes a way past the gate.
    fn default() -> Self {
        Self {
            tier: "free".to_string(),
            expires_at_ms: None,
        }
    }
}

impl SubscriptionInfo {
    pub fn is_premium(&self) -> bool {
        if self.tier != "premium" {
            return false;
        }
        match self.expires_at_ms {
            Some(at) => at > now_ms(),
            None => true,
        }
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The live entitlement, shared between the Tauri command that writes it
/// and the route handlers that read it.
///
/// It is a field of `ApiState` rather than an axum `Extension`, and that
/// is deliberate. As an `Extension` it was a layer someone had to
/// remember to add — and nobody did, so the one route that read it
/// returned a 500 for the life of the feature while the other twelve
/// checked nothing at all. In `ApiState` it is a struct field: a handler
/// that wants it takes it from state, and a build that forgot to supply
/// it does not compile.
#[derive(Debug, Default)]
pub struct EntitlementState {
    inner: RwLock<SubscriptionInfo>,
}

impl EntitlementState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Replace the entitlement. Called from the Tauri command the
    /// frontend invokes at boot and on every subscription change.
    pub fn set(&self, info: SubscriptionInfo) {
        // A poisoned lock here would mean a panic while holding a write
        // guard on a two-field struct, which cannot happen — but the
        // recovery is still "keep the old value", never "grant access".
        if let Ok(mut guard) = self.inner.write() {
            *guard = info;
        }
    }

    pub fn get(&self) -> SubscriptionInfo {
        self.inner
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// The gate itself.
    pub fn is_premium(&self) -> bool {
        self.inner
            .read()
            .map(|g| g.is_premium())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_denied() {
        assert!(!EntitlementState::default().is_premium());
        assert!(!SubscriptionInfo::default().is_premium());
    }

    #[test]
    fn premium_without_expiry_is_allowed() {
        assert!(SubscriptionInfo {
            tier: "premium".into(),
            expires_at_ms: None,
        }
        .is_premium());
    }

    #[test]
    fn expired_premium_is_denied() {
        assert!(!SubscriptionInfo {
            tier: "premium".into(),
            expires_at_ms: Some(1),
        }
        .is_premium());
    }

    #[test]
    fn other_tiers_are_denied_however_long_they_run() {
        assert!(!SubscriptionInfo {
            tier: "free".into(),
            expires_at_ms: Some(i64::MAX),
        }
        .is_premium());
    }

    #[test]
    fn revoking_takes_effect_on_the_next_read() {
        let state = EntitlementState::new();
        state.set(SubscriptionInfo {
            tier: "premium".into(),
            expires_at_ms: None,
        });
        assert!(state.is_premium());
        state.set(SubscriptionInfo::default());
        assert!(!state.is_premium(), "a revoked entitlement is denied at once");
    }
}
