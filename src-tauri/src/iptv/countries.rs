//! iptv-org country data. The API serves 247 countries with their ISO
//! code, language list, and a flag emoji — used to render the browse
//! rows ("🇮🇹 Italy (296)") and to give the parser a real name when the
//! M3U hands us a `tvg-country` code instead of a full country name.
//!
//! The data is small (~50 KB) and rarely changes, so a 24h disk cache
//! is plenty. On the first call after install the request races against
//! the browse page; on every subsequent call the cache hits and the
//! store can render rows immediately.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::errors::IptvError;

const COUNTRIES_URL: &str = "https://iptv-org.github.io/api/countries.json";
const CACHE_FILE: &str = "countries.json";
const TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub name: String,
    pub code: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub flag: String,
}

#[derive(Deserialize)]
struct WireCountry {
    name: String,
    code: String,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    flag: String,
}

impl From<WireCountry> for Country {
    fn from(w: WireCountry) -> Self {
        Self {
            name: w.name,
            code: w.code,
            languages: w.languages,
            flag: w.flag,
        }
    }
}

/// Fetch the full countries list, keyed by ISO 3166-1 alpha-2 code. Uses
/// a 24h disk cache so the first call after install is the only one
/// that hits the network.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_countries(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, Country>, IptvError> {
    let path = cache_path(app);
    if let Some(hit) = read_cache(&path) {
        return Ok(hit);
    }

    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!(
            "Rivulet/",
            env!("CARGO_PKG_VERSION"),
            " (iptv country data)"
        ))
        .build()?
        .get(COUNTRIES_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let wire: Vec<WireCountry> = serde_json::from_str(&body)?;
    let map: HashMap<String, Country> = wire
        .into_iter()
        .map(|w| (w.code.clone(), Country::from(w)))
        .collect();
    write_cache(&path, &map)?;
    Ok(map)
}

fn cache_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("iptv")
        .join(CACHE_FILE)
}

fn read_cache(path: &PathBuf) -> Option<HashMap<String, Country>> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > TTL {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_cache(path: &PathBuf, data: &HashMap<String, Country>) -> Result<(), IptvError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IptvError::CacheError(e.to_string()))?;
    }
    let json = serde_json::to_string(data).map_err(|e| IptvError::CacheError(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| IptvError::CacheError(e.to_string()))
}
