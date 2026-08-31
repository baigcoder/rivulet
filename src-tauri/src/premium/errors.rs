use std::fmt;

/// Every error the Premium TV module can produce.
///
/// **Display impls never include the password or the credential-bearing
/// parts of any URL.** This is the single rule the rest of the module
/// relies on: a log line, a frontend error message, or a serialized
/// response built from one of these variants must not leak secrets.
#[derive(Debug)]
pub enum PremiumError {
    /// Username / password / server URL didn't authenticate.
    AuthFailed,
    /// 404 or "channel not found" from the provider.
    NotFound,
    /// 429 from the provider; the caller may retry after a backoff.
    RateLimited,
    /// Anything 5xx from the provider that we couldn't recover.
    ServerError(String),
    /// Provider returned non-JSON, or a field the schema requires was
    /// missing or null. The inner string names the offending field so
    /// a test can pin it.
    MalformedResponse(String),
    /// reqwest / network / DNS / TLS error not covered by the
    /// variants above.
    Network(String),
    /// Connect or read deadline tripped.
    Timeout,
    /// SQLite error.
    Database(String),
    /// OS keychain or AES-GCM error.
    CredentialError(String),
    /// The provider returned 401 mid-session — token expired.
    SessionExpired,
    /// The premium API was hit but the local subscription is not
    /// `tier === 'premium'` or `expiresAt <= now`.
    PremiumRequired,
    /// Premium is enabled but no provider is connected.
    ProviderNotConnected,
    /// User cancelled an in-flight operation (e.g. M3U import).
    Cancelled,
    /// A catalog import is already running. Not a failure — the answer
    /// to an impatient second `/refresh`, which must not start a
    /// competing download of the same several hundred megabytes.
    AlreadySyncing,
}

impl fmt::Display for PremiumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PremiumError::AuthFailed => write!(
                f,
                "Could not sign in. Check the server URL, username and password."
            ),
            PremiumError::NotFound => write!(f, "Not found."),
            PremiumError::RateLimited => write!(
                f,
                "The provider is rate-limiting us. Try again in a moment."
            ),
            // An upstream error string can contain the request URL. That URL
            // carries Xtream credentials, so it must never cross this boundary.
            PremiumError::ServerError(_) => write!(
                f,
                "The provider returned an error. Check that the account is active and try again."
            ),
            PremiumError::MalformedResponse(field) => write!(
                f,
                "Unexpected response from provider: missing or invalid '{field}'."
            ),
            PremiumError::Network(_) => write!(
                f,
                "Could not reach the provider. Check the server address and your network connection."
            ),
            PremiumError::Timeout => write!(
                f,
                "Connection timed out. The server may be unavailable."
            ),
            PremiumError::Database(m) => write!(f, "Database error: {m}"),
            PremiumError::CredentialError(m) => write!(
                f,
                "Credential storage error: {m}"
            ),
            PremiumError::SessionExpired => write!(
                f,
                "Your session has expired. Please reconnect."
            ),
            PremiumError::PremiumRequired => write!(
                f,
                "Premium TV is not available on this install."
            ),
            PremiumError::ProviderNotConnected => write!(
                f,
                "No IPTV provider is connected."
            ),
            PremiumError::Cancelled => write!(f, "The operation was cancelled."),
            PremiumError::AlreadySyncing => write!(
                f,
                "A channel import is already running. Give it a moment."
            ),
        }
    }
}

impl std::error::Error for PremiumError {}

impl From<reqwest::Error> for PremiumError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            PremiumError::Timeout
        }
        else if err.is_connect() {
            PremiumError::Network(err.to_string())
        }
        else if err.is_status() {
            let status = err.status();
            match status {
                Some(s) if s.as_u16() == 401 || s.as_u16() == 403 => PremiumError::AuthFailed,
                Some(s) if s.as_u16() == 404 => PremiumError::NotFound,
                Some(s) if s.as_u16() == 429 => PremiumError::RateLimited,
                Some(s) if s.is_server_error() => PremiumError::ServerError(s.to_string()),
                _ => PremiumError::Network(err.to_string()),
            }
        }
        else {
            PremiumError::Network(err.to_string())
        }
    }
}

impl From<rusqlite::Error> for PremiumError {
    fn from(err: rusqlite::Error) -> Self {
        PremiumError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for PremiumError {
    fn from(err: serde_json::Error) -> Self {
        PremiumError::MalformedResponse(err.to_string())
    }
}

impl From<PremiumError> for String {
    fn from(err: PremiumError) -> Self {
        err.to_string()
    }
}
