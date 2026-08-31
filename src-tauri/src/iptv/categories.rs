//! iptv-org category data. The API serves 30 categories with their
//! canonical id, display name, and description. The M3U hands us
//! `group-title` as freeform text (often
//! `"Entertainment;Family;General"`), and mapping that to one of these
//! 30 ids is how the browse "Categories" tab stops looking like a
//! word salad.
//!
//! Same caching strategy as `countries.rs`: 24h disk cache, fetched
//! once, served from disk for a day.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::errors::IptvError;

const CATEGORIES_URL: &str = "https://iptv-org.github.io/api/categories.json";
const CACHE_FILE: &str = "categories.json";
const TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Deserialize)]
struct WireCategory {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
}

impl From<WireCategory> for Category {
    fn from(w: WireCategory) -> Self {
        Self {
            id: w.id,
            name: w.name,
            description: w.description,
        }
    }
}

/// Fetch all categories, keyed by id.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_categories(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, Category>, IptvError> {
    let path = cache_path(app);
    if let Some(hit) = read_cache(&path) {
        return Ok(hit);
    }

    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!(
            "Rivulet/",
            env!("CARGO_PKG_VERSION"),
            " (iptv category data)"
        ))
        .build()?
        .get(CATEGORIES_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let wire: Vec<WireCategory> = serde_json::from_str(&body)?;
    let map: HashMap<String, Category> = wire
        .into_iter()
        .map(|w| (w.id.clone(), Category::from(w)))
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

fn read_cache(path: &PathBuf) -> Option<HashMap<String, Category>> {
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

fn write_cache(path: &PathBuf, data: &HashMap<String, Category>) -> Result<(), IptvError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IptvError::CacheError(e.to_string()))?;
    }
    let json = serde_json::to_string(data).map_err(|e| IptvError::CacheError(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| IptvError::CacheError(e.to_string()))
}
