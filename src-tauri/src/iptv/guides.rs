//! Free TV's programme guide, from XMLTV addresses the user adds.
//!
//! The playlist names each channel's guide id (`tvg-id`, stored as
//! `epg_id`), and iptv-org no longer hosts the guides those ids point at —
//! so the guide was empty for every free channel. Rivulet ships no guide of
//! its own, for the same reason it ships no sources: the user adds an XMLTV
//! address they trust, and this module matches its programmes to channels.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::Serialize;

use super::commands::IptvState;
use super::errors::IptvError;
use super::models::EpgProgram;

pub const MAX_GUIDE_URLS: usize = 10;
pub const MAX_GUIDE_BYTES: usize = 256 * 1024 * 1024;
pub const GUIDE_TTL_SECS: i64 = 12 * 3600;

/// What is kept: enough behind for "what just ended", a day and a half ahead.
const KEEP_BEHIND_SECS: i64 = 3 * 3600;
const KEEP_AHEAD_SECS: i64 = 36 * 3600;

const URLS_KEY: &str = "guide_urls";
const IMPORTED_KEY: &str = "guide_imported_at";

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideReport {
    pub programs: usize,
    pub channels: usize,
    pub failed: Vec<String>,
}

fn db_err(e: impl std::fmt::Display) -> IptvError {
    IptvError::Database(e.to_string())
}

fn meta(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM iptv_meta WHERE key = ?1", params![key], |r| r.get(0))
        .ok()
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), IptvError> {
    conn.execute(
        "INSERT OR REPLACE INTO iptv_meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(db_err)?;
    Ok(())
}

pub fn guide_urls(conn: &Connection) -> Result<Vec<String>, IptvError> {
    Ok(meta(conn, URLS_KEY)
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

/// Only `http(s)` addresses, each once, at most `MAX_GUIDE_URLS`. Returns
/// what was kept, so the page shows the list as stored rather than as typed.
pub fn set_guide_urls(conn: &Connection, urls: &[String]) -> Result<Vec<String>, IptvError> {
    let mut seen = HashSet::new();
    let kept: Vec<String> = urls
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| {
            let lower = u.to_ascii_lowercase();
            (lower.starts_with("https://") || lower.starts_with("http://")) && u.len() > 8
        })
        .filter(|u| seen.insert(u.clone()))
        .take(MAX_GUIDE_URLS)
        .collect();
    set_meta(conn, URLS_KEY, &serde_json::to_string(&kept).map_err(db_err)?)?;
    Ok(kept)
}

/// A guide's channel id and a playlist's `tvg-id` name the same channel when
/// they agree ignoring case and the `@SD`/`@HD` feed suffix iptv-org adds.
pub fn feed_key(xmltv_channel: &str) -> String {
    let id = xmltv_channel.trim();
    id.split('@').next().unwrap_or(id).to_ascii_lowercase()
}

/// Plain or gzipped XMLTV, refused past `MAX_GUIDE_BYTES` decompressed — a
/// guide is read whole, and an unbounded one is a memory exhaustion.
pub fn decode_guide(raw: &[u8]) -> Result<String, IptvError> {
    let limit = MAX_GUIDE_BYTES as u64;
    let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw)
            .take(limit + 1)
            .read_to_end(&mut out)
            .map_err(|e| IptvError::ParseError(format!("gzip: {e}")))?;
        out
    } else {
        raw.to_vec()
    };
    if bytes.len() as u64 > limit {
        return Err(IptvError::ParseError("guide is larger than 256 MB".into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// XMLTV's `20240101120000 +0200`, as unix seconds. The offset is honoured —
/// dropping it is how a guide ends up an hour or two off.
fn parse_time(raw: &str) -> Option<i64> {
    let mut parts = raw.split_whitespace();
    let main = parts.next()?;
    let naive = chrono::NaiveDateTime::parse_from_str(main.get(0..14)?, "%Y%m%d%H%M%S").ok()?;
    let offset = parts
        .next()
        .and_then(|o| {
            let sign = match o.get(0..1)? { "+" => 1, "-" => -1, _ => return None };
            let h: i64 = o.get(1..3)?.parse().ok()?;
            let m: i64 = o.get(3..5)?.parse().ok()?;
            Some(sign * (h * 3600 + m * 60))
        })
        .unwrap_or(0);
    Some(naive.and_utc().timestamp() - offset)
}

/// Replace this source's guide with `programs`, matched to its channels.
/// Returns (programmes stored, channels that got at least one).
pub fn store_programs(
    conn: &Connection,
    source_id: &str,
    programs: &[EpgProgram],
    now: i64,
    offset_minutes: i32,
) -> Result<(usize, usize), IptvError> {
    // Several channels can share a guide id (the same feed in two groups).
    let mut by_key: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, epg_id FROM iptv_channels WHERE source_id = ?1 AND epg_id IS NOT NULL AND epg_id != ''")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![source_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?;
        for row in rows {
            let (id, epg) = row.map_err(db_err)?;
            by_key.entry(feed_key(&epg)).or_default().push(id);
        }
    }

    let shift = i64::from(offset_minutes) * 60;
    let (lo, hi) = (now - KEEP_BEHIND_SECS, now + KEEP_AHEAD_SECS);
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    tx.execute("DELETE FROM iptv_epg_programs WHERE source_id = ?1", params![source_id])
        .map_err(db_err)?;
    let mut stored = 0;
    let mut channels = HashSet::new();
    {
        let mut insert = tx
            .prepare(
                "INSERT OR REPLACE INTO iptv_epg_programs
                   (source_id, epg_id, channel_id, title, description, start_ts, end_ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(db_err)?;
        for p in programs {
            let Some(ids) = by_key.get(&feed_key(&p.channel_id)) else { continue };
            let Some(start) = parse_time(&p.start).map(|t| t + shift) else { continue };
            let stop = p.stop.as_deref().and_then(parse_time).map(|t| t + shift);
            if start >= hi || stop.unwrap_or(start) < lo {
                continue;
            }
            for id in ids {
                insert
                    .execute(params![source_id, p.channel_id, id, p.title, p.description, start, stop])
                    .map_err(db_err)?;
                stored += 1;
                channels.insert(id.clone());
            }
        }
    }
    tx.commit().map_err(db_err)?;
    Ok((stored, channels.len()))
}

fn rfc3339(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

/// The stored guide for these channels, in the window the importer keeps, with
/// times as RFC 3339 — the format the page's `new Date()` reads.
pub fn programs_for(
    conn: &Connection,
    source_id: &str,
    channel_ids: &[String],
    now: i64,
) -> Result<Vec<EpgProgram>, IptvError> {
    let mut out = Vec::new();
    // SQLite caps bound parameters; a page of channels is far under this.
    for chunk in channel_ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let sql = format!(
            "SELECT channel_id, title, description, start_ts, end_ts FROM iptv_epg_programs
             WHERE source_id = ? AND channel_id IN ({marks})
               AND (end_ts IS NULL OR end_ts > ?) AND start_ts < ?
             ORDER BY channel_id, start_ts"
        );
        let mut values: Vec<rusqlite::types::Value> = vec![source_id.to_string().into()];
        values.extend(chunk.iter().map(|c| c.clone().into()));
        values.push((now - KEEP_BEHIND_SECS).into());
        values.push((now + KEEP_AHEAD_SECS).into());
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok(EpgProgram {
                    channel_id: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    start: rfc3339(r.get(3)?),
                    stop: r.get::<_, Option<i64>>(4)?.map(rfc3339),
                })
            })
            .map_err(db_err)?;
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
    }
    Ok(out)
}

const OFFSET_KEY: &str = "guide_offset_minutes";
/// Twelve hours either way covers every guide stamped in the wrong zone.
const MAX_OFFSET_MINUTES: i32 = 12 * 60;

/// Minutes added to every guide time: for a guide that runs early or late
/// because its publisher stamped local time as UTC.
pub fn guide_offset(conn: &Connection) -> i32 {
    meta(conn, OFFSET_KEY).and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn set_guide_offset(conn: &Connection, minutes: i32) -> Result<i32, IptvError> {
    let clamped = minutes.clamp(-MAX_OFFSET_MINUTES, MAX_OFFSET_MINUTES);
    set_meta(conn, OFFSET_KEY, &clamped.to_string())?;
    Ok(clamped)
}

pub fn imported_at(conn: &Connection) -> Option<i64> {
    meta(conn, IMPORTED_KEY).and_then(|v| v.parse().ok())
}

async fn fetch(url: &str) -> Result<Vec<u8>, IptvError> {
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent(concat!("Rivulet/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    if resp.content_length().is_some_and(|n| n > MAX_GUIDE_BYTES as u64) {
        return Err(IptvError::ParseError("guide is larger than 256 MB".into()));
    }
    Ok(resp.bytes().await?.to_vec())
}

/// Fetch every guide address, match it to the free channels, and replace the
/// stored guide. One address failing is reported, not fatal to the others.
pub async fn import_guides(state: &IptvState, source_id: &str) -> Result<GuideReport, IptvError> {
    let (urls, offset_minutes) = {
        let conn = state.db.lock().map_err(db_err)?;
        (guide_urls(&conn)?, guide_offset(&conn))
    };
    let mut report = GuideReport::default();
    let mut programs = Vec::new();
    for url in urls {
        let parsed = match fetch(&url).await {
            // Parsing a large guide is CPU work; keep it off the async workers.
            Ok(raw) => tokio::task::spawn_blocking(move || {
                decode_guide(&raw).and_then(|text| super::xmltv::parse_xmltv(&text))
            })
            .await
            .map_err(db_err)
            .and_then(|r| r),
            Err(e) => Err(e),
        };
        match parsed {
            Ok(mut p) => programs.append(&mut p),
            Err(e) => {
                eprintln!("[guides] {url}: {e}");
                report.failed.push(url);
            }
        }
    }
    let now = chrono::Utc::now().timestamp();
    let conn = state.db.lock().map_err(db_err)?;
    // Every address failed: keep yesterday's guide rather than wipe it.
    if programs.is_empty() && !report.failed.is_empty() {
        return Ok(report);
    }
    let (stored, channels) = store_programs(&conn, source_id, &programs, now, offset_minutes)?;
    set_meta(&conn, IMPORTED_KEY, &now.to_string())?;
    report.programs = stored;
    report.channels = channels;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iptv::{db, models::LiveChannel, xmltv};

    const SRC: &str = "free:iptv-org";
    /// 2024-01-01 12:00:00 UTC.
    const NOON: i64 = 1_704_110_400;

    fn conn_with_channel(epg_id: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_schema_pub(&conn).unwrap();
        db::ensure_source_exists(&conn, SRC, "m3u").unwrap();
        let ch = LiveChannel {
            id: "ch1".into(),
            name: "One".into(),
            stream_url: Some("http://s/1.m3u8".into()),
            epg_id: Some(epg_id.into()),
            ..Default::default()
        };
        db::insert_channel(&conn, SRC, &ch).unwrap();
        conn
    }

    fn guide(channel: &str, start: &str, stop: &str) -> String {
        format!(
            r#"<tv><programme channel="{channel}" start="{start}" stop="{stop}"><title>News</title></programme></tv>"#
        )
    }

    #[test]
    fn feed_key_strips_case_and_suffix() {
        assert_eq!(feed_key("1Plus1Ukraina.ua@SD"), "1plus1ukraina.ua");
        assert_eq!(feed_key("  BBC1.uk "), "bbc1.uk");
    }

    #[test]
    fn matches_epg_id_ignoring_case_and_feed_suffix() {
        let conn = conn_with_channel("1Plus1Ukraina.ua");
        let xml = guide("1plus1ukraina.ua@HD", "20240101120000 +0000", "20240101130000 +0000");
        let programs = xmltv::parse_xmltv(&xml).unwrap();
        let (stored, channels) = store_programs(&conn, SRC, &programs, NOON, 0).unwrap();
        assert_eq!((stored, channels), (1, 1));
        let got = programs_for(&conn, SRC, &["ch1".into()], NOON).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].channel_id, "ch1");
        assert!(got[0].start.starts_with("2024-01-01T12:00:00"), "{}", got[0].start);
    }

    #[test]
    fn honours_the_xmltv_offset() {
        let conn = conn_with_channel("x.ua");
        // 14:00 at +0200 is 12:00 UTC.
        let xml = guide("x.ua", "20240101140000 +0200", "20240101150000 +0200");
        store_programs(&conn, SRC, &xmltv::parse_xmltv(&xml).unwrap(), NOON, 0).unwrap();
        let got = programs_for(&conn, SRC, &["ch1".into()], NOON).unwrap();
        assert!(got[0].start.starts_with("2024-01-01T12:00:00"), "{}", got[0].start);
    }

    #[test]
    fn drops_programmes_outside_window() {
        let conn = conn_with_channel("x.ua");
        let xml = format!(
            "<tv>{}{}</tv>",
            r#"<programme channel="x.ua" start="20231230120000 +0000" stop="20231230130000 +0000"><title>Old</title></programme>"#,
            r#"<programme channel="x.ua" start="20240104120000 +0000" stop="20240104130000 +0000"><title>Far</title></programme>"#,
        );
        let (stored, _) = store_programs(&conn, SRC, &xmltv::parse_xmltv(&xml).unwrap(), NOON, 0).unwrap();
        assert_eq!(stored, 0);
    }

    #[test]
    fn rejects_oversized_guide() {
        assert!(decode_guide(&vec![b'a'; MAX_GUIDE_BYTES + 1]).is_err());
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &vec![b'a'; MAX_GUIDE_BYTES + 1]).unwrap();
        assert!(decode_guide(&gz.finish().unwrap()).is_err());
        assert_eq!(decode_guide(b"<tv/>").unwrap(), "<tv/>");
    }

    #[test]
    fn offset_shifts_start_and_stop() {
        let conn = conn_with_channel("x.ua");
        let xml = guide("x.ua", "20240101120000 +0000", "20240101130000 +0000");
        store_programs(&conn, SRC, &xmltv::parse_xmltv(&xml).unwrap(), NOON, 60).unwrap();
        let got = programs_for(&conn, SRC, &["ch1".into()], NOON).unwrap();
        assert!(got[0].start.starts_with("2024-01-01T13:00:00"), "{}", got[0].start);
        assert!(got[0].stop.as_deref().unwrap().starts_with("2024-01-01T14:00:00"));
        assert_eq!(set_guide_offset(&conn, 5000).unwrap(), 720);
        assert_eq!(guide_offset(&conn), 720);
        assert_eq!(set_guide_offset(&conn, -90).unwrap(), -90);
    }

    #[test]
    fn set_guide_urls_keeps_only_http_and_caps() {
        let conn = Connection::open_in_memory().unwrap();
        db::run_schema_pub(&conn).unwrap();
        let kept = set_guide_urls(
            &conn,
            &["ftp://x".into(), " https://a/g.xml ".into(), "https://a/g.xml".into(), "javascript:1".into()],
        )
        .unwrap();
        assert_eq!(kept, vec!["https://a/g.xml".to_string()]);
        assert_eq!(guide_urls(&conn).unwrap(), kept);
        let many: Vec<String> = (0..20).map(|i| format!("https://a/{i}.xml")).collect();
        assert_eq!(set_guide_urls(&conn, &many).unwrap().len(), MAX_GUIDE_URLS);
    }
}
