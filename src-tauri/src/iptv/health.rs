//! Free TV channel health, found out in the background.
//!
//! A public playlist is other people's servers. Measured from one home
//! connection, a third of the curated list and about half of iptv-org's
//! South Asian channels did not answer at all — and the app only found that
//! out one click at a time, a black screen and an auto-skip per dead channel.
//! So after an import, and again once the last check has gone stale, every
//! channel is asked once, here in Rust where it costs the page nothing, and
//! the ones that fail twice drop out of the list (`build_query_filter`).
//!
//! Stored rather than kept per session, but never final: every sweep asks the
//! dead ones again, so a channel that was down this morning is back tonight.
//!
//! Opening Free TV asks too, when the last sweep is more than half an hour
//! old, and verdicts land in batches as they come — written to the table and
//! sent to the page together — so the grid marks a dead channel within
//! seconds of opening instead of after the whole list has been asked.
//!
//! The question is "does it answer", not "does it play": a manifest and the
//! first thing it points at. That is what separated the channels mpv could
//! open from the ones it could not, and it costs a few kilobytes a channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::commands::IptvState;
use super::db::{self, HealthTarget};
use super::sources::FREE_TV_SOURCE_ID;

/// In flight at once. Each is a few KB; the bound is other people's servers.
const CONCURRENCY: usize = 24;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A launch within this long of the last sweep reuses its verdicts.
pub const STALE_AFTER_SECS: i64 = 6 * 60 * 60;
/// Opening Free TV asks again once the last sweep is older than this.
pub const OPEN_STALE_SECS: i64 = 30 * 60;
/// Verdicts are written and sent to the page this many at a time.
const BATCH: usize = 50;
/// A playlist bigger than this is not a live channel's manifest.
const MANIFEST_CAP: usize = 256 * 1024;
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

static RUNNING: AtomicBool = AtomicBool::new(false);
/// An import landed while a sweep was already running: its new rows are
/// unchecked, so go round once more rather than leave them until next launch.
static AGAIN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthProgress {
    pub checked: usize,
    pub total: usize,
    pub offline: usize,
    pub done: bool,
    /// Channels found answering / dead since the last event, for the grid
    /// to mark without a reload.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alive: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dead: Vec<String>,
}

/// Check every Free TV channel in the background. `force` ignores a recent
/// sweep — an import has just replaced the rows it judged.
pub fn spawn_sweep(app: AppHandle, force: bool) {
    spawn_if_older(app, if force { 0 } else { STALE_AFTER_SECS });
}

/// Free TV was opened: check unless a sweep finished within `OPEN_STALE_SECS`.
pub fn check_on_open(app: AppHandle) {
    spawn_if_older(app, OPEN_STALE_SECS);
}

fn spawn_if_older(app: AppHandle, max_age_secs: i64) {
    let force = max_age_secs == 0;
    if RUNNING.swap(true, Ordering::SeqCst) {
        if force {
            AGAIN.store(true, Ordering::SeqCst);
        }
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut max_age = max_age_secs;
        loop {
            AGAIN.store(false, Ordering::SeqCst);
            if let Err(e) = sweep(&app, max_age).await {
                eprintln!("[iptv] health sweep failed: {e}");
            }
            if !AGAIN.load(Ordering::SeqCst) {
                break;
            }
            max_age = 0;
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn sweep(app: &AppHandle, max_age_secs: i64) -> Result<(), String> {
    let targets = {
        let state = app.try_state::<IptvState>().ok_or("no IPTV state")?;
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        let fresh = db::health_swept_at(&conn, FREE_TV_SOURCE_ID)
            .is_some_and(|at| unix_now() - at < max_age_secs);
        if fresh {
            return Ok(());
        }
        db::health_targets(&conn, FREE_TV_SOURCE_ID).map_err(|e| e.to_string())?
    };
    if targets.is_empty() {
        return Ok(());
    }

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(BROWSER_UA)
        .build()
        .map_err(|e| e.to_string())?;
    let total = targets.len();
    // Write a batch of verdicts and tell the page which channels they were.
    let record = |results: &[(String, bool)], checked: usize, offline: usize, done: bool| -> Result<(), String> {
        if !results.is_empty() {
            let state = app.try_state::<IptvState>().ok_or("no IPTV state")?;
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            db::set_health_batch(&conn, FREE_TV_SOURCE_ID, results).map_err(|e| e.to_string())?;
        }
        let (alive, dead): (Vec<_>, Vec<_>) = results.iter().partition(|(_, ok)| *ok);
        let _ = app.emit(
            "live_health",
            HealthProgress {
                checked,
                total,
                offline,
                done,
                alive: alive.into_iter().map(|(id, _)| id.clone()).collect(),
                dead: dead.into_iter().map(|(id, _)| id.clone()).collect(),
            },
        );
        Ok(())
    };
    record(&[], 0, 0, false)?;

    // First pass: an answer is final and lands at once; a failure waits for
    // the second ask below, so a busy minute does not mark a channel dead.
    let mut failed: Vec<HealthTarget> = Vec::new();
    let mut checked = 0;
    {
        let mut batch: Vec<(String, bool)> = Vec::with_capacity(BATCH);
        let mut first = stream::iter(targets)
            .map(|t| {
                let client = client.clone();
                async move {
                    let ok = answers(&client, &t).await;
                    (t, ok)
                }
            })
            .buffer_unordered(CONCURRENCY);
        while let Some((t, ok)) = first.next().await {
            checked += 1;
            if ok {
                batch.push((t.id, true));
            } else {
                failed.push(t);
            }
            if checked % BATCH == 0 {
                record(&batch, checked, 0, false)?;
                batch.clear();
            }
        }
        record(&batch, checked, 0, false)?;
    }

    // A timeout in a busy minute is not a dead channel: ask the failures once
    // more before hiding any of them.
    let mut offline = 0;
    {
        let mut batch: Vec<(String, bool)> = Vec::with_capacity(BATCH);
        let mut again = stream::iter(failed)
            .map(|t| {
                let client = client.clone();
                async move {
                    let ok = answers(&client, &t).await;
                    (t.id, ok)
                }
            })
            .buffer_unordered(CONCURRENCY);
        while let Some((id, ok)) = again.next().await {
            if !ok {
                offline += 1;
            }
            batch.push((id, ok));
            if batch.len() == BATCH {
                record(&batch, checked, offline, false)?;
                batch.clear();
            }
        }
        record(&batch, checked, offline, false)?;
    }

    {
        let state = app.try_state::<IptvState>().ok_or("no IPTV state")?;
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        db::stamp_health_sweep(&conn, FREE_TV_SOURCE_ID, unix_now()).map_err(|e| e.to_string())?;
    }
    eprintln!("[iptv] health sweep: {offline} of {total} channels offline");
    record(&[], total, offline, true)
}

/// Does this channel answer: a 2xx, and for a playlist, a 2xx from the first
/// thing it lists too. A URL that isn't HTTP can't be asked from here, so it
/// stays listed rather than being called dead for our lack of a client.
async fn answers(client: &Client, t: &HealthTarget) -> bool {
    let url = t.url.as_str();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return true;
    }
    let get = |u: &str| {
        let mut req = client.get(u);
        if let Some(ua) = t.user_agent.as_deref().filter(|s| !s.is_empty()) {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(rf) = t.referer.as_deref().filter(|s| !s.is_empty()) {
            req = req.header(reqwest::header::REFERER, rf);
        }
        req
    };
    let Ok(mut resp) = get(url).send().await else {
        return false;
    };
    if !resp.status().is_success() {
        return false;
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    // A web page where a stream should be — a login wall, a parked domain.
    if content_type.starts_with("text/html") {
        return false;
    }
    let base = resp.url().clone();
    let path = base.path().to_ascii_lowercase();
    let playlist =
        content_type.contains("mpegurl") || path.ends_with(".m3u8") || path.ends_with(".m3u");
    if !playlist {
        // Raw MPEG-TS or a file: the status line is the whole answer, and
        // reading the body would download the channel.
        return true;
    }
    let mut body = Vec::new();
    while body.len() < MANIFEST_CAP {
        match resp.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(_) => return false,
        }
    }
    let Some(next) = playlist_entry(&String::from_utf8_lossy(&body), &base) else {
        return false;
    };
    matches!(get(next.as_str()).send().await, Ok(r) if r.status().is_success())
}

/// The first thing a playlist points at — a variant or a segment — resolved
/// against where it was actually served from. `None` for a body that is not
/// a playlist at all, which is a dead channel however it was labelled.
fn playlist_entry(body: &str, base: &url::Url) -> Option<url::Url> {
    let body = body.trim_start_matches('\u{feff}').trim_start();
    if !body.starts_with("#EXTM3U") {
        return None;
    }
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))?;
    base.join(line).ok()
}

#[cfg(test)]
mod tests {
    use super::playlist_entry;

    #[test]
    fn a_playlist_points_at_its_first_entry_resolved_against_its_own_url() {
        let base = url::Url::parse("https://cdn.example/live/ch/master.m3u8?token=1").unwrap();
        let body = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n../720/index.m3u8\n";
        assert_eq!(
            playlist_entry(body, &base).unwrap().as_str(),
            "https://cdn.example/live/720/index.m3u8"
        );
    }

    #[test]
    fn a_byte_order_mark_is_still_a_playlist() {
        let base = url::Url::parse("http://host/a.m3u8").unwrap();
        assert!(playlist_entry("\u{feff}#EXTM3U\nseg.ts\n", &base).is_some());
    }

    #[test]
    fn a_page_or_an_empty_playlist_is_no_entry() {
        let base = url::Url::parse("http://host/a.m3u8").unwrap();
        assert!(playlist_entry("<!DOCTYPE html><html>", &base).is_none());
        assert!(playlist_entry("#EXTM3U\n#EXT-X-ENDLIST\n", &base).is_none());
    }
}
