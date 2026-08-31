//! JWT mint/verify for the local Premium HTTP API.
//!
//! The JWT is *not* an end-user authentication token. It is a
//! "this process owns this socket" proof that the Vue frontend and
//! the Rust axum server share: both run inside the same Tauri
//! process, the secret is in the OS keychain, the token is short
//! lived, and only the local origin can present it. The threat it
//! answers is "another app on the same machine reaches the loopback
//! port" — not "a remote user authenticates to a remote server."
//!
//! The same key also signs the per-channel redirector token. A
//! redirector token is just a JWT whose body names the connection
//! and channel and whose `exp` is 30 seconds from now. The
//! `/premium-stream/:token` handler verifies the signature and the
//! expiry, reads the encrypted config out of the vault, and
//! redirects to the real upstream URL with the right
//! `Referer`/`User-Agent`. The raw URL with the password in it
//! never appears in a route query string, a proxy log, or a player
//! command line.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::ApiError;
use crate::premium::crypto::CredentialVault;

/// The slot the JWT key is stored under. 32 random bytes, hex-encoded
/// into the keychain.
pub const JWT_KEY_SLOT: &str = "rivulet-api-jwt";

/// 1h. Short enough that a leaked token is useless by the time the
/// user notices, long enough that the Vue side never has to
/// re-mint inside a single session.
pub const DEFAULT_TTL_SECS: i64 = 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClaims {
    /// Standard JWT `sub` — always "premium-api".
    pub sub: String,
    /// Standard JWT `exp` — seconds since epoch.
    pub exp: i64,
    /// Standard JWT `iat` — seconds since epoch.
    pub iat: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamClaims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
    /// The `iptv_premium_connections.id` the token is for.
    pub connection_id: String,
    /// The `iptv_premium_channels.id` to play.
    pub channel_id: String,
    /// Random per mint, so no two authorizations are the same string.
    ///
    /// Not decoration. `iat`/`exp` are whole seconds, so two mints for
    /// one channel inside the same second used to produce a
    /// byte-identical URL — and the player's "the source changed, so
    /// restart" watcher cannot see a change that is not there, which is
    /// how a reconnect could sit on a dead stream. A `jti` makes every
    /// authorization distinguishable in a log, too.
    #[serde(default)]
    pub jti: String,
}

fn load_or_create_jwt_key() -> Result<Vec<u8>, ApiError> {
    #[cfg(not(target_os = "android"))]
    {
        let entry = keyring::Entry::new(JWT_KEY_SLOT, "hmac")
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        match entry.get_password() {
            Ok(hex) => {
                let bytes = hex::decode(hex.trim())
                    .map_err(|e| ApiError::Internal(format!("hex: {e}")))?;
                if bytes.len() != 32 {
                    return Err(ApiError::Internal("jwt key wrong length".into()));
                }
                Ok(bytes)
            }
            Err(keyring::Error::NoEntry) => {
                let mut bytes = vec![0u8; 32];
                rand::thread_rng().fill_bytes(&mut bytes);
                entry
                    .set_password(&hex::encode(&bytes))
                    .map_err(|e| ApiError::Internal(e.to_string()))?;
                Ok(bytes)
            }
            Err(e) => Err(ApiError::Internal(e.to_string())),
        }
    }
    #[cfg(target_os = "android")]
    {
        use std::io::{Read, Write};
        let base = std::env::var("RIVULET_APP_CACHE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        std::fs::create_dir_all(&base).ok();
        let path = base.join("rivulet-api-jwt.bin");
        if path.exists() {
            let mut file = std::fs::File::open(&path)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            if bytes.len() == 32 {
                return Ok(bytes);
            }
        }
        let mut bytes = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            file.write_all(&bytes)
                .map_err(|e| ApiError::Internal(e.to_string()))?;
        }
        Ok(bytes)
    }
}

/// The HMAC key, read from the keychain once per process.
///
/// Every request that carries a bearer token verifies it, and every
/// `/play` mints a redirector token — so without this, a page of sixty
/// channels is sixty-one keychain round trips. On Linux that is sixty-one
/// D-Bus calls to a Secret Service that may be locked; on macOS it is
/// sixty-one Keychain Access lookups. The key is a process-lifetime
/// constant, so reading it more than once buys nothing.
///
/// Not a `get_or_init` closure: the load is fallible and a failure must
/// not be cached. A locked keychain that the user then unlocks has to
/// work on the next request, which it cannot if the first failure was
/// stored as the answer.
static JWT_KEY: OnceLock<Vec<u8>> = OnceLock::new();

fn jwt_key() -> Result<&'static [u8], ApiError> {
    if let Some(key) = JWT_KEY.get() {
        return Ok(key.as_slice());
    }
    let key = load_or_create_jwt_key()?;
    // A racing thread may have won; then this returns theirs and drops
    // ours. Both are the same 32 bytes out of the same slot, so which
    // one wins does not matter.
    Ok(JWT_KEY.get_or_init(|| key).as_slice())
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn mint_api_token() -> Result<(String, i64), ApiError> {
    let key = jwt_key()?;
    let now = now_secs();
    let exp = now + DEFAULT_TTL_SECS;
    let claims = ApiClaims {
        sub: "premium-api".to_string(),
        exp,
        iat: now,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(key),
    )
    .map_err(|e| ApiError::Internal(format!("encode: {e}")))?;
    Ok((token, exp))
}

pub fn verify_api_token(token: &str) -> Result<ApiClaims, ApiError> {
    let key = jwt_key()?;
    let mut validation = Validation::default();
    validation.set_required_spec_claims(&["exp", "sub"]);
    let data = decode::<ApiClaims>(token, &DecodingKey::from_secret(key), &validation)
        .map_err(|e| ApiError::Unauthorized(format!("{e}")))?;
    Ok(data.claims)
}

/// Mint a per-channel redirector token. The body names the
/// connection and channel; `exp` is a deadline in seconds-since-epoch.
/// Signed with the same key the API token uses — the secret is in
/// the OS keychain and the signer/verifier share the same vault.
pub fn mint_stream_token(
    _vault: &CredentialVault,
    connection_id: &str,
    channel_id: &str,
    expires_at_ms: i64,
) -> Result<String, ApiError> {
    let key = jwt_key()?;
    let now = now_secs();
    let exp = (expires_at_ms / 1000).max(now + 1);
    let mut nonce = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut nonce);
    let claims = StreamClaims {
        sub: "premium-stream".to_string(),
        exp,
        iat: now,
        connection_id: connection_id.to_string(),
        channel_id: channel_id.to_string(),
        jti: hex::encode(nonce),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(key),
    )
    .map_err(|e| ApiError::Internal(format!("encode: {e}")))
}

pub fn verify_stream_token(
    _vault: &CredentialVault,
    token: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let key = jwt_key()?;
    let mut validation = Validation::default();
    validation.set_required_spec_claims(&["exp"]);
    let data = decode::<StreamClaims>(token, &DecodingKey::from_secret(key), &validation)
        .map_err(|_| ApiError::Unauthorized("invalid token".into()))?;
    Ok(Some((data.claims.connection_id, data.claims.channel_id)))
}
