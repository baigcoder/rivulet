//! iptv-org EPG (Electronic Program Guide) for free TV channels.
//!
//! iptv-org used to host a small `epg/channels.json` map plus one XMLTV
//! file per channel under `epg/guides/{id}.xml`. Both URLs 404 now. The
//! replacement is `guides.json`: each row names an iptv-org channel id
//! (the same `tvg-id` the playlist uses) and, when someone is actually
//! publishing a guide, a `sources` list with an XML or gzip URL.
//!
//! Most rows have no source — iptv-org no longer hosts the XML itself.
//! A miss is an empty guide, not a connection error.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use flate2::read::GzDecoder;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tauri::Manager;

use super::errors::IptvError;
use super::models::EpgProgram;

const GUIDES_URL: &str = "https://iptv-org.github.io/api/guides.json";
const GUIDES_CACHE: &str = "epg_guide_urls.json";
const GUIDES_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const GUIDE_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Deserialize)]
struct WireSource {
    #[serde(default)]
    url: String,
    #[serde(default)]
    format: String,
}

#[derive(Debug, Deserialize)]
struct WireGuide {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    sources: Vec<WireSource>,
}

/// Prefer a gzip body (we already sniff it) then plain XML.
fn pick_source_url(sources: &[WireSource]) -> Option<String> {
    let usable = |want: &str| {
        sources
            .iter()
            .find(|s| !s.url.is_empty() && s.format.eq_ignore_ascii_case(want))
    };
    usable("GZIP")
        .or_else(|| usable("XML"))
        .or_else(|| sources.iter().find(|s| !s.url.is_empty()))
        .map(|s| s.url.clone())
}

/// Map of `tvg-id` -> guide URL, with the compact map cached on disk.
/// The upstream JSON is tens of megabytes; we only keep rows that name
/// a real source, so a cache hit is a small file and a miss is one
/// download every seven days.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_channel_mapping(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, String>, IptvError> {
    let path = cache_path(app, GUIDES_CACHE);
    if let Some(hit) = read_mapping_cache(&path) {
        return Ok(hit);
    }

    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!(
            "Rivulet/",
            env!("CARGO_PKG_VERSION"),
            " (iptv EPG data)"
        ))
        .build()?
        .get(GUIDES_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let wire: Vec<WireGuide> = serde_json::from_str(&body)?;
    let mut map = HashMap::new();
    for g in wire {
        let Some(id) = g.channel.filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(url) = pick_source_url(&g.sources) else {
            continue;
        };
        map.entry(id).or_insert(url);
    }
    write_mapping_cache(&path, &map)?;
    Ok(map)
}

fn read_mapping_cache(path: &PathBuf) -> Option<HashMap<String, String>> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > GUIDES_TTL {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_mapping_cache(path: &PathBuf, data: &HashMap<String, String>) -> Result<(), IptvError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IptvError::CacheError(e.to_string()))?;
    }
    let json = serde_json::to_string(data).map_err(|e| IptvError::CacheError(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| IptvError::CacheError(e.to_string()))
}

fn cache_path(app: &tauri::AppHandle, name: &str) -> PathBuf {
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("iptv")
        .join(name)
}

// ── In-memory guide cache ─────────────────────────────────────────────

type GuideCache = HashMap<String, (Vec<EpgProgram>, std::time::Instant)>;

static GUIDE_CACHE: Lazy<Mutex<GuideCache>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Look up a published XMLTV URL for this `tvg-id` and parse the next
/// 24 hours. No row, or a dead URL, is an empty list — never a playback
/// error. The playlist still plays without a guide.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_guide(
    app: &tauri::AppHandle,
    tvg_id: &str,
) -> Result<Vec<EpgProgram>, IptvError> {
    let mapping = match fetch_channel_mapping(app).await {
        Ok(m) => m,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(url) = mapping.get(tvg_id) else {
        return Ok(Vec::new());
    };

    if let Some(hit) = read_guide_cache(tvg_id) {
        return Ok(hit);
    }

    let programs = match download_and_parse_guide(url, tvg_id).await {
        Ok(p) => p,
        Err(_) => Vec::new(),
    };
    write_guide_cache(tvg_id, &programs);
    Ok(programs)
}

fn read_guide_cache(key: &str) -> Option<Vec<EpgProgram>> {
    let cache = GUIDE_CACHE.lock().ok()?;
    let (programs, saved) = cache.get(key)?;
    if saved.elapsed() < GUIDE_TTL {
        return Some(programs.clone());
    }
    None
}

fn write_guide_cache(key: &str, programs: &[EpgProgram]) {
    if let Ok(mut cache) = GUIDE_CACHE.lock() {
        cache.insert(
            key.to_string(),
            (programs.to_vec(), std::time::Instant::now()),
        );
    }
}

async fn download_and_parse_guide(url: &str, tvg_id: &str) -> Result<Vec<EpgProgram>, IptvError> {
    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Rivulet/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // The API may serve gzipped or plain XML; sniff the first two bytes
    // (gzip magic is 0x1f 0x8b). When gzipped, pipe through flate2.
    let xml_bytes: Vec<u8> = if body.len() >= 2 && body[0] == 0x1f && body[1] == 0x8b {
        let mut decoder = GzDecoder::new(&body[..]);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| IptvError::ParseError(e.to_string()))?;
        out
    } else {
        body.to_vec()
    };

    let xml = String::from_utf8(xml_bytes).map_err(|e| IptvError::ParseError(e.to_string()))?;
    super::xmltv::parse_programs(&xml, tvg_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_beats_xml_and_empty_urls_are_skipped() {
        let sources = vec![
            WireSource {
                url: String::new(),
                format: "XML".into(),
            },
            WireSource {
                url: "https://example/guide.xml".into(),
                format: "XML".into(),
            },
            WireSource {
                url: "https://example/guide.xml.gz".into(),
                format: "GZIP".into(),
            },
        ];
        assert_eq!(
            pick_source_url(&sources).as_deref(),
            Some("https://example/guide.xml.gz")
        );
    }

    #[test]
    fn no_source_is_no_guide() {
        assert_eq!(pick_source_url(&[]), None);
    }
}
