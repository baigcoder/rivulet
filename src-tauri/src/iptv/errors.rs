use std::fmt;

#[derive(Debug)]
pub enum IptvError {
    Network(String),
    InvalidCredentials,
    Expired,
    Disabled,
    Timeout,
    TlsError,
    InvalidServer,
    InvalidResponse(String),
    UnsupportedProvider,
    CacheError(String),
    CredentialError(String),
    ParseError(String),
    Database(String),
    Cancelled,
}

impl fmt::Display for IptvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IptvError::Network(msg) => write!(f, "Network error: {msg}"),
            IptvError::InvalidCredentials => write!(
                f,
                "Invalid credentials. Check your Server URL, username and password."
            ),
            IptvError::Expired => write!(f, "Your IPTV subscription has expired."),
            IptvError::Disabled => write!(f, "Your IPTV account has been disabled."),
            IptvError::Timeout => write!(f, "Connection timed out. The server may be unavailable."),
            IptvError::TlsError => write!(f, "TLS/SSL connection failed. Check the Server URL."),
            IptvError::InvalidServer => {
                write!(f, "Invalid server URL. Check the Server URL format.")
            }
            IptvError::InvalidResponse(msg) => {
                write!(f, "Unexpected response from provider: {msg}")
            }
            IptvError::UnsupportedProvider => write!(f, "This provider is not supported."),
            IptvError::CacheError(msg) => write!(f, "Cache error: {msg}"),
            IptvError::CredentialError(msg) => write!(f, "Credential storage error: {msg}"),
            IptvError::ParseError(msg) => write!(f, "Failed to parse data: {msg}"),
            IptvError::Database(msg) => write!(f, "Database error: {msg}"),
            IptvError::Cancelled => write!(f, "The import was cancelled."),
        }
    }
}

impl std::error::Error for IptvError {}

impl From<reqwest::Error> for IptvError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            IptvError::Timeout
        } else if err.is_connect() {
            let msg = err.to_string();
            if msg.contains("tls") || msg.contains("certificate") || msg.contains("ssl") {
                IptvError::TlsError
            } else {
                IptvError::Network(msg)
            }
        } else {
            IptvError::Network(err.to_string())
        }
    }
}

impl From<quick_xml::Error> for IptvError {
    fn from(err: quick_xml::Error) -> Self {
        IptvError::ParseError(err.to_string())
    }
}

impl From<serde_json::Error> for IptvError {
    fn from(err: serde_json::Error) -> Self {
        IptvError::ParseError(err.to_string())
    }
}

impl From<rusqlite::Error> for IptvError {
    fn from(err: rusqlite::Error) -> Self {
        IptvError::Database(err.to_string())
    }
}

impl From<IptvError> for String {
    fn from(err: IptvError) -> Self {
        err.to_string()
    }
}
