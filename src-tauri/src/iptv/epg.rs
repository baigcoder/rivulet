//! iptv-org EPG (Electronic Program Guide) for free TV channels.
//!
//! The pipeline is two steps:
//!
//! 1. Fetch `https://iptv-org.github.io/api/epg/channels.json` once and
//!    cache it for 7 days. The file maps an iptv-org `tvg-id`
//!    (e.g. `"BBCNews.uk"`) to an XMLTV guide id
//!    (e.g. `"BBC1.uk"`). Cached on disk; the file is small.
//!
//! 2. When the player asks for a channel's EPG, look up the XMLTV
//!    guide id, fetch `https://iptv-org.github.io/api/epg/guides/{id}.xml`
//!    (gzip-compressed XMLTV), decompress, parse, and return the
//!    programs for the next 24 hours. The result is cached in memory
//!    for 1 hour — guides are large and change on the order of days,
//!    not seconds, so an in-memory cache is plenty.

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

const CHANNELS_URL: &str = "https://iptv-org.github.io/api/epg/channels.json";
const CHANNELS_CACHE: &str = "epg_channels.json";
const CHANNELS_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const GUIDE_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Deserialize)]
struct WireEpgChannel {
    id: String,
    #[serde(default)]
    xmltv_id: Option<String>,
}

/// Map of `tvg-id` -> XMLTV guide id, with the file cached on disk.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_channel_mapping(
    app: &tauri::AppHandle,
) -> Result<HashMap<String, String>, IptvError> {
    let path = cache_path(app, CHANNELS_CACHE);
    if let Some(hit) = read_mapping_cache(&path) {
        return Ok(hit);
    }

    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!(
            "Rivulet/",
            env!("CARGO_PKG_VERSION"),
            " (iptv EPG data)"
        ))
        .build()?
        .get(CHANNELS_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let wire: Vec<WireEpgChannel> = serde_json::from_str(&body)?;
    let map: HashMap<String, String> = wire
        .into_iter()
        .filter_map(|w| w.xmltv_id.map(|x| (w.id, x)))
        .collect();
    write_mapping_cache(&path, &map)?;
    Ok(map)
}

fn read_mapping_cache(path: &PathBuf) -> Option<HashMap<String, String>> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > CHANNELS_TTL {
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

/// Look up the XMLTV guide id for a channel's `tvg-id`, then fetch and
/// parse its guide. Returns programs for the next 24 hours, or an empty
/// list if the channel has no guide or the network is unreachable.
#[allow(dead_code)] // Tauri command system — only invoked from JS.
pub async fn fetch_guide(
    app: &tauri::AppHandle,
    tvg_id: &str,
) -> Result<Vec<EpgProgram>, IptvError> {
    let mapping = fetch_channel_mapping(app).await?;
    let Some(xmltv_id) = mapping.get(tvg_id) else {
        return Ok(Vec::new());
    };

    if let Some(hit) = read_guide_cache(xmltv_id) {
        return Ok(hit);
    }

    let programs = download_and_parse_guide(xmltv_id).await?;
    write_guide_cache(xmltv_id, &programs);
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

#[allow(dead_code)]
async fn download_and_parse_guide(xmltv_id: &str) -> Result<Vec<EpgProgram>, IptvError> {
    let url = format!("https://iptv-org.github.io/api/epg/guides/{}.xml", xmltv_id);

    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Rivulet/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(&url)
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
    super::xmltv::parse_programs(&xml, xmltv_id)
}
