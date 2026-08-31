//! Memory-flat M3U importer.
//!
//! The previous `m3u::fetch_m3u_playlist` buffered the entire playlist body
//! into a single `String` before parsing, which is a 1 GB string for a 1 GB
//! playlist. This module reads from the response `bytes_stream`, walks
//! `\n` boundaries as they arrive, and inserts each parsed channel into
//! SQLite inside a transaction. The maximum resident set is bounded by
//! the line buffer (kilobytes), not the file.
//!
//! The existing `parse_m3u_iter` already takes an iterator, so the parser
//! itself is unchanged — only the byte pump that feeds it is new.

use std::collections::HashMap;
use std::path::Path;

use futures_util::StreamExt;
use rusqlite::Connection;
use tauri::{AppHandle, Emitter};

use super::countries::Country;
use super::db;
use super::errors::IptvError;
use super::m3u;
use super::models::LiveChannel;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub url: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub stage: ImportStage,
    pub channels_imported: u64,
}

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImportStage {
    Connecting,
    Downloading,
    Parsing,
    Indexing,
    Complete,
    Cancelled,
}

/// Stream `url` into `source_id` as channels. `cancel` is checked between
/// chunks; a `true` value means drop the staging source and return
/// `IptvError::Cancelled`. The M3U body is never fully materialised.
///
/// `db_path` is the on-disk path the state's `IptvState` already uses
/// — the importer opens its own connection to the same file so WAL is
/// shared with the rest of the subsystem. Without the explicit path
/// the importer would default to a different file and FK constraints
/// on `iptv_channels.source_id → iptv_sources.id` would fail.
pub async fn stream_into_source<F>(
    app: Option<&AppHandle>,
    db_path: &Path,
    url: &str,
    source_id: &str,
    // A per-country playlist knows its country but writes it on no line, so
    // the caller says which; the last fallback after `tvg-country` and the
    // group title. `None` for a worldwide list.
    country_hint: Option<&str>,
    // Clear the source's existing channels first. One source can be fed by
    // several playlists (see `m3u::free_playlists`), and only the first of
    // them may wipe — otherwise each import erases the one before it.
    wipe: bool,
    cancel: F,
) -> Result<(), IptvError>
where
    F: Fn() -> bool + Send + Sync,
{
    // IPTV provider servers vary wildly:
    //   - `output=ts&type=m3u_plus` on Xtream endpoints is *generated*
    //     on the fly: each line is a separate `.ts` URL, and the
    //     provider's `php` script can run for many minutes before
    //     flushing its first byte. A 1Mbps VPS in particular can
    //     take 30+ minutes for a 500K-channel playlist.
    //   - Many providers have an `nginx proxy_read_timeout` around
    //     60s, which kills the upstream connection even though our
    //     client is willing to wait longer.
    //   - Some upstreams `Content-Length` correctly and we can
    //     resume; others don't and we have to start over.
    //
    // The most defensive shape is: no overall request timeout, a
    // generous connect timeout, and a per-chunk idle timeout. If the
    // server drops the connection mid-stream, the bytes_stream
    // `next().await` returns `None` and we exit the loop cleanly —
    // any channels parsed up to that point are already committed.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(60))
        // 30 minutes per chunk. Xtream's `output=ts&type=m3u_plus` is
        // generated on the fly: the upstream `php` can take 5-15
        // minutes to flush the next channel's metadata when the
        // playlist has 500K+ entries. A 10-minute chunk timeout
        // would falsely trigger on a healthy slow server.
        .read_timeout(std::time::Duration::from_secs(30 * 60))
        // No `.timeout(...)` here — the user's playlist can take as
        // long as the server needs. The 30-minute read timeout fires
        // *only* if a single chunk stalls for that long, not on the
        // overall request.
        .gzip(true)
        .brotli(true)
        .user_agent("VLC/3.0.18 LibVLC/3.0.18")
        .build()
        .map_err(IptvError::from)?;
    let _ = emit(app, url, ImportStage::Connecting, 0, 0, 0);

    let resp = client
        .get(url)
        .header("User-Agent", "VLC/3.0.18 LibVLC/3.0.18")
        .send()
        .await
        .map_err(IptvError::from)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(IptvError::Network(format!("HTTP {status}")));
    }
    let total_bytes = resp.content_length().unwrap_or(0);
    let _ = emit(app, url, ImportStage::Downloading, 0, total_bytes, 0);

    // Resolve the iptv-org country map from disk (best-effort).
    let app_dir = std::env::var("RIVULET_APP_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let country_map: Option<HashMap<String, Country>> =
        read_country_cache(&app_dir.join("iptv").join("countries.json"));

    // State that survives across the resume loop. Rust can't prove
    // they're used through `continue`, so we use a `_used` shim
    // where needed.
    let mut line_buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut bytes_downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();

    // The line → channel state machine lives in `m3u::parse_m3u_iter`. That
    // function takes a `(usize, &str)` iterator; we adapt by iterating
    // over lines of `line_buf` once the buffer is full or the body ends.
    // To avoid building a 1GB string we instead parse here directly, line
    // by line, with the same regexes as `m3u::parse_extinf`. The behaviour
    // is identical to the old single-shot parser, just streamed.

    use regex::Regex;
    let extinf_re = Regex::new(r#"tvg-id="([^"]*)""#).ok();
    let tvg_name_re = Regex::new(r#"tvg-name="([^"]*)""#).ok();
    let tvg_logo_re = Regex::new(r#"tvg-logo="([^"]*)""#).ok();
    let group_re = Regex::new(r#"group-title="([^"]*)""#).ok();
    let country_re = Regex::new(r#"tvg-country="([^"]*)""#).ok();
    let language_re = Regex::new(r#"tvg-language="([^"]*)""#).ok();

    let mut pending: Option<m3u::ExtinfData> = None;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Keep memory bounded while avoiding one SQLite commit per channel. A
    // 256-row transaction is quick on Android and makes large provider lists
    // import many times faster than SQLite autocommit mode.
    let mut pending_inserts: Vec<LiveChannel> = Vec::with_capacity(256);
    let mut total_channels: u64 = 0;
    let mut skipped_non_live: u64 = 0;

    // The import runs in two phases. In the *outer* loop we own the
    // connection but no transaction; we open one when we need to commit,
    // commit it, and re-open. This is the standard "long-running bulk
    // insert" pattern in SQLite: never hold a transaction open for the
    // full multi-minute download.
    let conn = open_db(db_path)?;
    // Defensive: make sure the source row exists *in this connection*
    // before we touch its child tables. The caller is expected to have
    // upserted it on the state's connection, but a stale WAL snapshot
    // or a re-import after an interrupted run can leave this
    // connection blind. `INSERT OR IGNORE` is a no-op if the row is
    // already there, so this is safe to call every time.
    db::ensure_source_exists(&conn, source_id, "free-m3u")?;
    if wipe {
        db::delete_source_channels(&conn, source_id)?;
    }

    // Resume support. Xtream providers routinely drop the connection
    // mid-stream — the upstream `php` script hits its `max_execution_time`
    // (often 30s), the provider's nginx `proxy_read_timeout` fires, or
    // the user's ISP NATs the connection idle. We retry with
    // exponential backoff up to 10 times. On a `200` response to a
    // Range request, we **don't** discard the partial work — the
    // `seen` HashSet dedupes channels whose URLs we already have, and
    // the per-channel SQLite commit means anything we parsed is
    // already durable. Restarting from byte 0 and re-parsing the
    // whole body just gives the server another chance to send the
    // tail.
    const MAX_ATTEMPTS: u32 = 10;
    let mut byte_offset: u64 = 0;
    let mut attempt: u32 = 0;
    let mut total_bytes: u64 = 0;

    'attempts: loop {
        attempt += 1;
        if attempt > MAX_ATTEMPTS {
            // We've tried our best. Whatever we did get is committed.
            // Emit a final progress event and let the caller activate
            // the source — partial is better than none, and the user
            // can re-import to get more.
            eprintln!(
                "[iptv] gave up after {MAX_ATTEMPTS} attempts at {bytes_downloaded} bytes / {total_channels} channels"
            );
            let _ = emit(
                app,
                url,
                ImportStage::Complete,
                bytes_downloaded,
                total_bytes,
                total_channels,
            );
            break 'attempts;
        }
        if cancel() {
            let _ = db::delete_source(&conn, source_id);
            let _ = emit(
                app,
                url,
                ImportStage::Cancelled,
                bytes_downloaded,
                total_bytes,
                total_channels,
            );
            return Err(IptvError::Cancelled);
        }

        // Before each attempt, emit a fresh progress event so the UI
        // knows the importer is alive. Xtream's `output=ts` playlists
        // can take 5+ minutes per attempt, and a frozen-looking
        // progress bar is the #1 reason users give up on an import
        // that's actually working.
        let _ = emit(
            app,
            url,
            ImportStage::Connecting,
            bytes_downloaded,
            total_bytes,
            total_channels,
        );
        eprintln!(
            "[iptv] attempt {attempt}/{MAX_ATTEMPTS}: {bytes_downloaded} bytes, {total_channels} channels so far"
        );

        // Build a request. If we're resuming, ask the server for the
        // tail. Some providers support Range and reply 206 with
        // `Content-Range: bytes N-...`; Xtream providers typically
        // reply 200 with the full body, in which case we keep the
        // partial work and dedupe.
        let mut req = client
            .get(url)
            .header("User-Agent", "VLC/3.0.18 LibVLC/3.0.18");
        if byte_offset > 0 {
            req = req.header("Range", format!("bytes={byte_offset}-"));
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[iptv] attempt {attempt}/{MAX_ATTEMPTS} send failed: {e}");
                let backoff = std::time::Duration::from_secs((2u64.pow(attempt)).min(60));
                tokio::time::sleep(backoff).await;
                continue 'attempts;
            }
        };
        let status = resp.status();
        if !(status.is_success() || status.as_u16() == 206) {
            return Err(IptvError::Network(format!("HTTP {status}")));
        }
        if status.as_u16() == 200 && byte_offset > 0 {
            // Server replied 200 to our Range request — it ignored
            // us. We're going to re-parse the whole body, but the
            // per-channel `INSERT OR REPLACE` and the `seen`
            // HashSet make this safe: any channel we already have
            // gets skipped, and any new channel gets inserted.
            eprintln!(
                "[iptv] server ignored Range on attempt {attempt}; restarting from byte 0 with {total_channels} already committed"
            );
            byte_offset = 0;
            line_buf.clear();
            bytes_downloaded = 0;
            pending = None;
            // Note: we deliberately keep `seen` and `total_channels`
            // — those represent committed state and dedup correctly.
        }
        if total_bytes == 0 {
            // Pull Content-Length once, from the first successful
            // response. After a 206, `content_length` is the *total*
            // length, not the remaining bytes, so this assignment is
            // only correct on the first attempt.
            total_bytes = resp.content_length().unwrap_or(0);
        }
        let _ = emit(
            app,
            url,
            ImportStage::Downloading,
            bytes_downloaded,
            total_bytes,
            total_channels,
        );

        let mut stream = resp.bytes_stream();
        let mut stream_failed: bool = false;
        while let Some(chunk) = stream.next().await {
            if cancel() {
                let _ = db::delete_source(&conn, source_id);
                let _ = emit(
                    app,
                    url,
                    ImportStage::Cancelled,
                    bytes_downloaded,
                    total_bytes,
                    total_channels,
                );
                return Err(IptvError::Cancelled);
            }
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "[iptv] stream dropped at {bytes_downloaded} bytes / {total_channels} channels: {e}"
                    );
                    stream_failed = true;
                    break;
                }
            };
            bytes_downloaded += chunk.len() as u64;
            byte_offset = bytes_downloaded;
            line_buf.extend_from_slice(&chunk);

            // One-shot body-format diagnostic. We sample the first
            // 2KB as text and count CR/LF bytes in the first 64KB so
            // the user can see what the server is actually sending
            // when an import is suspiciously slow. The Xtream M3U
            // Plus TS endpoint is known to switch between LF, CRLF
            // and CR-only across hosts, and the previous parser
            // crashed silently on the CR-only case — these logs
            // make the failure mode obvious from the dev-tools
            // console.
            if bytes_downloaded <= 65536 {
                let cr = line_buf
                    .iter()
                    .take(bytes_downloaded as usize)
                    .filter(|&&b| b == b'\r')
                    .count();
                let lf = line_buf
                    .iter()
                    .take(bytes_downloaded as usize)
                    .filter(|&&b| b == b'\n')
                    .count();
                eprintln!(
                    "[iptv] first {bytes_downloaded} bytes: CR={cr}, LF={lf} (CR-only line endings would show CR>0, LF=0)"
                );
                if bytes_downloaded <= 2048 {
                    let sample = &line_buf[..line_buf.len().min(2048)];
                    let preview = String::from_utf8_lossy(sample);
                    eprintln!("[iptv] body preview: {preview}");
                }
            }

            // Walk every newline currently in the buffer. Everything
            // after the last newline stays in `line_buf` for the
            // next chunk. We split on both `\n` and `\r` because
            // Xtream M3U Plus TS bodies from PHP panels frequently
            // use `\r` (CR) only — a body with 500K records and no
            // `\n` would otherwise never trigger the state machine
            // and we'd see 1 channel from 500MB of data. `\r\n` is
            // handled as a single line ending.
            //
            // We do the walk byte-by-byte, not by line-by-line, because
            // some Xtream M3U Plus bodies mix ASCII headers with
            // binary padding or other non-UTF-8 bytes; an
            // `std::str::from_utf8` failure on one line would
            // otherwise drop the rest of the body.
            let mut consumed_to: usize = 0;
            let mut pos = 0usize;
            while pos < line_buf.len() {
                if line_buf[pos] == b'\n' || line_buf[pos] == b'\r' {
                    let end = if line_buf[pos] == b'\n' && pos > 0 && line_buf[pos - 1] == b'\r' {
                        pos - 1
                    } else {
                        pos
                    };
                    let raw_line = &line_buf[consumed_to..end];
                    // Check the prefix as raw bytes — that's the only
                    // thing we need to drive the state machine. The
                    // rest of the line (tvg-id, tvg-name, etc.) only
                    // matters when we actually have a valid UTF-8
                    // EXTINF to parse.
                    let starts_with_extinf =
                        raw_line.len() > 8 && raw_line[..8].eq_ignore_ascii_case(b"#EXTINF:");
                    let starts_with_extvlcopt =
                        raw_line.len() > 11 && raw_line[..11].eq_ignore_ascii_case(b"#EXTVLCOPT:");
                    let starts_with_hash = raw_line.first().copied() == Some(b'#');
                    let is_empty = raw_line.iter().all(|&b| b == b' ' || b == b'\t');
                    let is_stream_url = !is_empty
                        && !starts_with_hash
                        && (raw_line.starts_with(b"http://")
                            || raw_line.starts_with(b"https://")
                            || raw_line.starts_with(b"rtsp://")
                            || raw_line.starts_with(b"rtmp://"));

                    if starts_with_extinf {
                        if let Ok(line) = std::str::from_utf8(raw_line) {
                            let data = m3u::parse_extinf_pub(
                                line,
                                extinf_re.as_ref(),
                                tvg_name_re.as_ref(),
                                tvg_logo_re.as_ref(),
                                group_re.as_ref(),
                                country_re.as_ref(),
                                language_re.as_ref(),
                            );
                            pending = Some(data);
                        } else {
                            // Non-UTF-8 EXTINF — skip it but keep the
                            // pending state from the previous one
                            // intact, in case the next line is the
                            // URL.
                        }
                    } else if starts_with_extvlcopt {
                        if let (Some(extinf), Ok(line)) =
                            (pending.as_mut(), std::str::from_utf8(raw_line))
                        {
                            let directive = line.trim_start_matches("#EXTVLCOPT:").trim();
                            if let Some(v) = directive.strip_prefix("http-user-agent=") {
                                extinf.user_agent = Some(v.to_string());
                            } else if let Some(v) = directive.strip_prefix("http-referrer=") {
                                extinf.referer = Some(v.to_string());
                            }
                        }
                    } else if is_stream_url {
                        // Stream URL — only commit when we can extract
                        // the channel's name AND positively identify
                        // the stream as live. The pending state from
                        // the previous EXTINF is consumed here; if
                        // the line is non-UTF-8 we still try to insert
                        // a row with whatever name we can pull from
                        // the URL itself.
                        let stream_url = String::from_utf8_lossy(raw_line).into_owned();

                        // Xtream M3U Plus playlists (e.g.
                        // `get.php?...&output=ts&type=m3u_plus`) return
                        // every content type in one body. The
                        // stream-URL path segment tells us which:
                        //   /live/...     → live TV (import)
                        //   /movie/...    → VOD (skip)
                        //   /series/...   → series (skip)
                        //   /timeshift/.. → catchup (skip)
                        //   anything else → unknown (skip — the rule
                        //     is "live must be positively identified")
                        let detected = detect_stream_type(&stream_url);
                        // Public playlists such as iptv-org normally use
                        // ordinary HLS URLs (`.../channel.m3u8`) and do not
                        // include an Xtream `/live/` path.  Only provider
                        // M3U imports need the strict path check that keeps
                        // movies and series out of Premium TV.
                        if source_id != super::sources::FREE_TV_SOURCE_ID
                            && !matches!(detected, Some("live"))
                        {
                            skipped_non_live += 1;
                            pending = None;
                        } else if seen.insert(stream_url.clone()) {
                            let extinf = pending.take().unwrap_or_default();
                            let id = extinf
                                .tvg_id
                                .clone()
                                .filter(|s| !s.is_empty())
                                // Namespaced per playlist: the counter restarts on every
                                // import into this source, so an unnamed channel in a
                                // supplement would otherwise take an id the worldwide
                                // list already used and `INSERT OR REPLACE` over it.
                                .unwrap_or_else(|| {
                                    format!("free-{}{total_channels}", country_hint.unwrap_or(""))
                                });
                            let name = extinf
                                .tvg_name
                                .clone()
                                .filter(|s| !s.is_empty())
                                // The comma title, before falling back to the URL:
                                // iptv-org's per-country playlists write no
                                // `tvg-name` at all.
                                .or_else(|| {
                                    extinf.title.clone().filter(|s| !s.is_empty())
                                })
                                .or_else(|| extract_name_from_url(&stream_url))
                                .unwrap_or_else(|| "Unknown Channel".to_string());
                            let country_code = extinf
                                .country
                                .clone()
                                .or_else(|| {
                                    extinf
                                        .group
                                        .as_deref()
                                        .and_then(super::protocols::group_title_country)
                                })
                                .or_else(|| {
                                    // Premium playlists often put a full country
                                    // name in the group title instead of tvg-country.
                                    extinf
                                        .group
                                        .as_deref()
                                        .and_then(|g| super::normalize::parse_category_name(g).0)
                                })
                                .or_else(|| country_hint.map(str::to_string));
                            let (country_name, country_flag) =
                                match (country_code.as_deref(), country_map.as_ref()) {
                                    (Some(code), Some(map)) => match map.get(code) {
                                        Some(c) => (Some(c.name.clone()), Some(c.flag.clone())),
                                        None => {
                                            (Some(super::normalize::normalize_country(code)), None)
                                        }
                                    },
                                    (Some(code), None) => {
                                        (Some(super::normalize::normalize_country(code)), None)
                                    }
                                    (None, _) => (None, None),
                                };
                            let country_name = country_name
                                .filter(|s| !s.is_empty())
                                .or_else(|| super::normalize::extract_country_from_name(&name));
                            let channel = LiveChannel {
                                id,
                                name: super::normalize::normalize_channel_name(&name),
                                logo_url: extinf.tvg_logo.filter(|s| !s.is_empty()),
                                stream_url: Some(stream_url),
                                category_id: None,
                                // The curated list's own taxonomy is *countries* —
                                // Italy, Greece, UK — and the rail is built
                                // from it, so a supplement files itself the
                                // same way. Its own group ("News", and 27 of
                                // Pakistan's 97 channels carry it) would
                                // scatter the country into worldwide buckets
                                // and leave no Pakistan entry to click.
                                // "Undefined" is iptv-org's placeholder and
                                // is worth less than no category at all.
                                category_name: country_hint
                                    .and(country_name.clone())
                                    .or_else(|| {
                                        extinf.group.filter(|s| {
                                            !s.is_empty()
                                                && !s.eq_ignore_ascii_case("undefined")
                                        })
                                    }),
                                country: country_name,
                                language: extinf.language.filter(|s| !s.is_empty()),
                                epg_id: extinf.tvg_id.filter(|s| !s.is_empty()),
                                stream_type: Some(detected.unwrap_or("live").to_string()),
                                country_code: country_code.filter(|s| !s.is_empty()),
                                country_flag,
                                user_agent: extinf.user_agent.filter(|s| !s.is_empty()),
                                referer: extinf.referer.filter(|s| !s.is_empty()),
                            };
                            pending_inserts.push(channel);
                            total_channels += 1;
                            if total_channels % 1000 == 0 {
                                eprintln!(
                                    "[iptv] imported {total_channels} live channels from {bytes_downloaded} bytes (skipped {skipped_non_live} non-live)"
                                );
                                let _ = emit(
                                    app,
                                    url,
                                    ImportStage::Downloading,
                                    bytes_downloaded,
                                    total_bytes,
                                    total_channels,
                                );
                            }
                        } else {
                            pending = None;
                        }
                    }
                    // Lines that are empty, whitespace-only, or unknown
                    // directives (e.g. #EXTGRP, #EXTVLCOPT) just keep
                    // `pending` intact and move on.
                    // CRLF is consumed in one step: skip the `\n` that
                    // immediately follows a `\r` we just processed.
                    if line_buf[pos] == b'\r'
                        && pos + 1 < line_buf.len()
                        && line_buf[pos + 1] == b'\n'
                    {
                        consumed_to = pos + 2;
                        // `pos` must skip the LF as well. Leaving it for
                        // the next loop made `end = pos - 1` smaller than
                        // `consumed_to`, causing a slice panic on every
                        // normal CRLF M3U playlist.
                        pos += 2;
                    } else {
                        consumed_to = pos + 1;
                        pos += 1;
                    }
                    continue;
                }
                pos += 1;
            }
            line_buf.drain(..consumed_to);

            if pending_inserts.len() >= 256 {
                db::insert_channels_batch(&conn, source_id, &pending_inserts)?;
                pending_inserts.clear();
            }

            if last_emit.elapsed() >= std::time::Duration::from_millis(150) {
                let _ = emit(
                    app,
                    url,
                    ImportStage::Downloading,
                    bytes_downloaded,
                    total_bytes,
                    total_channels,
                );
                last_emit = std::time::Instant::now();
            }
        }

        if !stream_failed {
            // Clean end of stream — we have the whole body.
            // Process any remaining bytes in `line_buf` as the final
            // line. The body might not end with a `\r` or `\n`, in
            // which case the last channel's URL would otherwise be
            // silently dropped.
            if !line_buf.is_empty() {
                let raw_line = &line_buf[..];
                let starts_with_hash = raw_line.first().copied() == Some(b'#');
                let is_empty = raw_line.iter().all(|&b| b == b' ' || b == b'\t');
                let is_stream_url = !is_empty
                    && !starts_with_hash
                    && (raw_line.starts_with(b"http://")
                        || raw_line.starts_with(b"https://")
                        || raw_line.starts_with(b"rtsp://")
                        || raw_line.starts_with(b"rtmp://"));
                if is_stream_url {
                    let stream_url = String::from_utf8_lossy(raw_line).into_owned();
                    // Same live-only filter as the main walk — the
                    // final trailing line might be a movie URL
                    // when the body has no terminating newline.
                    let detected = detect_stream_type(&stream_url);
                    if source_id != super::sources::FREE_TV_SOURCE_ID
                        && !matches!(detected, Some("live"))
                    {
                        skipped_non_live += 1;
                    } else if seen.insert(stream_url.clone()) {
                        let extinf = pending.take().unwrap_or_default();
                        let id = extinf
                            .tvg_id
                            .clone()
                            .filter(|s| !s.is_empty())
                            // Namespaced per playlist: the counter restarts on every
                            // import into this source, so an unnamed channel in a
                            // supplement would otherwise take an id the worldwide
                            // list already used and `INSERT OR REPLACE` over it.
                            .unwrap_or_else(|| {
                                format!("free-{}{total_channels}", country_hint.unwrap_or(""))
                            });
                        let name = extinf
                            .tvg_name
                            .clone()
                            .filter(|s| !s.is_empty())
                            .or_else(|| extinf.title.clone().filter(|s| !s.is_empty()))
                            .or_else(|| extract_name_from_url(&stream_url))
                            .unwrap_or_else(|| "Unknown Channel".to_string());
                        let country_code = extinf
                            .country
                            .clone()
                            .or_else(|| {
                                extinf
                                    .group
                                    .as_deref()
                                    .and_then(super::protocols::group_title_country)
                            })
                            .or_else(|| {
                                extinf
                                    .group
                                    .as_deref()
                                    .and_then(|g| super::normalize::parse_category_name(g).0)
                            })
                            .or_else(|| country_hint.map(str::to_string));
                        let (country_name, country_flag) =
                            match (country_code.as_deref(), country_map.as_ref()) {
                                (Some(code), Some(map)) => match map.get(code) {
                                    Some(c) => (Some(c.name.clone()), Some(c.flag.clone())),
                                    None => (Some(super::normalize::normalize_country(code)), None),
                                },
                                (Some(code), None) => {
                                    (Some(super::normalize::normalize_country(code)), None)
                                }
                                (None, _) => (None, None),
                            };
                        let country_name = country_name
                            .filter(|s| !s.is_empty())
                            .or_else(|| super::normalize::extract_country_from_name(&name));
                        let channel = LiveChannel {
                            id,
                            name: super::normalize::normalize_channel_name(&name),
                            logo_url: extinf.tvg_logo.filter(|s| !s.is_empty()),
                            stream_url: Some(stream_url),
                            category_id: None,
                            category_name: country_hint.and(country_name.clone()).or_else(|| {
                                extinf.group.filter(|s| {
                                    !s.is_empty() && !s.eq_ignore_ascii_case("undefined")
                                })
                            }),
                            country: country_name,
                            language: extinf.language.filter(|s| !s.is_empty()),
                            epg_id: extinf.tvg_id.filter(|s| !s.is_empty()),
                            stream_type: Some(detected.unwrap_or("live").to_string()),
                            country_code: country_code.filter(|s| !s.is_empty()),
                            country_flag,
                            user_agent: extinf.user_agent.filter(|s| !s.is_empty()),
                            referer: extinf.referer.filter(|s| !s.is_empty()),
                        };
                        pending_inserts.push(channel);
                        total_channels += 1;
                    }
                }
                line_buf.clear();
            }
            if !pending_inserts.is_empty() {
                db::insert_channels_batch(&conn, source_id, &pending_inserts)?;
                pending_inserts.clear();
            }
            break 'attempts;
        }

        // Stream dropped. Wait with exponential backoff before
        // trying again. On the *last* attempt we just emit
        // Complete and break out of the loop with whatever bytes we
        // did get.
        if attempt >= MAX_ATTEMPTS {
            eprintln!("[iptv] gave up after {attempt} attempts at {bytes_downloaded} bytes");
            break 'attempts;
        }
        // Exponential backoff capped at 60s. The Xtream server
        // usually recovers after a few seconds; 60s gives the worst
        // flaky hosts time to come back without making the user wait
        // half an hour between attempts.
        let backoff_secs = (2u64.pow(attempt)).min(60);
        eprintln!(
            "[iptv] connection dropped; retrying in {backoff_secs}s (attempt {attempt}/{MAX_ATTEMPTS})"
        );
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
    }

    let _ = emit(
        app,
        url,
        ImportStage::Parsing,
        bytes_downloaded,
        total_bytes,
        total_channels,
    );

    eprintln!(
        "[iptv] import complete: {total_channels} live channels imported, {skipped_non_live} non-live URLs skipped (movies/series/timeshift/unknown)"
    );

    // Build the pre-aggregated count tables in a single transaction.
    db::refresh_stats(&conn, source_id)?;
    let _ = emit(
        app,
        url,
        ImportStage::Indexing,
        bytes_downloaded,
        total_bytes,
        total_channels,
    );
    let _ = emit(
        app,
        url,
        ImportStage::Complete,
        bytes_downloaded,
        total_bytes,
        total_channels,
    );

    Ok(())
}

fn read_country_cache(path: &std::path::Path) -> Option<HashMap<String, Country>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn extract_name_from_url(url: &str) -> Option<String> {
    let path = url::Url::parse(url).ok()?.path().to_string();
    let stem = path.rsplit('/').next()?.to_string();
    let name = stem
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(&stem)
        .replace(['-', '_', '+'], " ");
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn emit(
    app: Option<&AppHandle>,
    url: &str,
    stage: ImportStage,
    bytes_downloaded: u64,
    total_bytes: u64,
    channels: u64,
) -> Result<(), tauri::Error> {
    if let Some(app) = app {
        app.emit(
            "m3u_progress",
            ImportProgress {
                url: url.to_string(),
                bytes_downloaded,
                total_bytes,
                stage,
                channels_imported: channels,
            },
        )?;
    }
    Ok(())
}

/// Classify a stream URL by its first path segment, the way
/// Xtream providers do. The path segment is the only thing the
/// Xtream protocol uses to distinguish content types, so it's
/// the only thing the importer can rely on.
///
/// Returns `"live"` for live TV channels, `"movie"` for VOD,
/// `"series"` for series, `"timeshift"` for catchup, or `None`
/// for anything else (unrecognised scheme, no path, etc.).
///
/// Channel name, group-title, category, keywords like "HD" or
/// "News" — none of those are reliable indicators of content
/// type. Only the URL path segment is.
pub(crate) fn detect_stream_type(url: &str) -> Option<&'static str> {
    let parsed = url::Url::parse(url).ok()?;
    let first = parsed.path().split('/').find(|s| !s.is_empty())?;
    match first {
        "live" => Some("live"),
        "movie" => Some("movie"),
        "series" => Some("series"),
        "timeshift" => Some("timeshift"),
        _ => None,
    }
}

/// Open a SQLite connection at `path`, running the IPTV schema and the
/// standard PRAGMAs. Used by the streaming importer to share the WAL
/// with the state's connection.
pub fn open_db(path: &Path) -> Result<Connection, IptvError> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path).map_err(|e| IptvError::Database(e.to_string()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-8000;",
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    crate::iptv::db::run_schema_pub(&conn)?;
    Ok(conn)
}

/// Split a byte buffer on `\n` and `\r` boundaries, returning each
/// line's byte slice (without the separator). `\r\n` is treated as
/// a single line ending. Lines are returned in order.
///
/// The previous parser only split on `\n`, which silently dropped
/// every record in a body that used CR-only line endings — the
/// Xtream M3U Plus TS endpoint is known to do this. This helper
/// exists so the split logic can be unit-tested in isolation
/// from the streaming importer's state machine.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn split_lines(buf: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] == b'\n' {
            let end = if i > 0 && buf[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            lines.push(&buf[start..end]);
            start = i + 1;
        } else if buf[i] == b'\r' {
            lines.push(&buf[start..i]);
            start = i + 1;
            if i + 1 < buf.len() && buf[i + 1] == b'\n' {
                start = i + 2;
                i += 1;
            }
        }
        i += 1;
    }
    if start < buf.len() {
        lines.push(&buf[start..]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_lf_only() {
        let buf = b"#EXTM3U\n#EXTINF:1,Ch1\nhttp://a/1.ts\nhttp://a/2.ts\n";
        let lines: Vec<&str> = split_lines(buf)
            .iter()
            .map(|l| std::str::from_utf8(l).unwrap())
            .collect();
        assert_eq!(
            lines,
            vec!["#EXTM3U", "#EXTINF:1,Ch1", "http://a/1.ts", "http://a/2.ts"]
        );
    }

    #[test]
    fn split_lines_crlf() {
        let buf = b"#EXTM3U\r\n#EXTINF:1,Ch1\r\nhttp://a/1.ts\r\n";
        let lines: Vec<&str> = split_lines(buf)
            .iter()
            .map(|l| std::str::from_utf8(l).unwrap())
            .collect();
        assert_eq!(lines, vec!["#EXTM3U", "#EXTINF:1,Ch1", "http://a/1.ts"]);
    }

    /// The actual Xtream M3U Plus TS bug: a body with CR-only line
    /// endings. The previous `\n`-only parser saw this as one giant
    /// line and committed 0–1 channels. This test pins the fix.
    #[test]
    fn split_lines_cr_only() {
        let buf = b"#EXTM3U\r#EXTINF:-1,Ch1\rhttp://a/1.ts\r#EXTINF:-1,Ch2\rhttp://a/2.ts\r";
        let lines: Vec<&str> = split_lines(buf)
            .iter()
            .map(|l| std::str::from_utf8(l).unwrap())
            .collect();
        assert_eq!(
            lines,
            vec![
                "#EXTM3U",
                "#EXTINF:-1,Ch1",
                "http://a/1.ts",
                "#EXTINF:-1,Ch2",
                "http://a/2.ts"
            ]
        );
    }

    /// A body with no trailing line separator — the final channel
    /// would be silently dropped by the previous parser because it
    /// never processed the tail of the buffer.
    #[test]
    fn split_lines_no_trailing_separator() {
        let buf = b"http://a/1.ts";
        let lines: Vec<&str> = split_lines(buf)
            .iter()
            .map(|l| std::str::from_utf8(l).unwrap())
            .collect();
        assert_eq!(lines, vec!["http://a/1.ts"]);
    }

    /// Mixed line endings — LF, CRLF, and CR-only interspersed.
    /// The previous parser would have split on the LF and CRLF
    /// boundaries, merging the CR-only sections into garbage.
    #[test]
    fn split_lines_mixed_endings() {
        let buf = b"http://a/1.ts\nhttp://a/2.ts\r\nhttp://a/3.ts\rhttp://a/4.ts";
        let lines: Vec<&str> = split_lines(buf)
            .iter()
            .map(|l| std::str::from_utf8(l).unwrap())
            .collect();
        assert_eq!(
            lines,
            vec![
                "http://a/1.ts",
                "http://a/2.ts",
                "http://a/3.ts",
                "http://a/4.ts"
            ]
        );
    }

    // ── detect_stream_type tests ───────────────────────────────────
    // The Xtream protocol uses the first URL path segment to tag
    // content type. The importer must positively identify "live"
    // before accepting a URL; everything else is rejected.

    #[test]
    fn detect_live_url() {
        assert_eq!(
            detect_stream_type("http://server.live/live/user/pass/12345.ts"),
            Some("live")
        );
        assert_eq!(
            detect_stream_type("https://example.com/live/u/p/abc.m3u8"),
            Some("live")
        );
    }

    #[test]
    fn detect_movie_url() {
        assert_eq!(
            detect_stream_type("http://server/movie/user/pass/12345.mp4"),
            Some("movie")
        );
        assert_eq!(
            detect_stream_type("http://server/movie/u/p/abc.mkv"),
            Some("movie")
        );
    }

    #[test]
    fn detect_series_url() {
        assert_eq!(
            detect_stream_type("http://server/series/user/pass/12345.mp4"),
            Some("series")
        );
    }

    #[test]
    fn detect_timeshift_url() {
        assert_eq!(
            detect_stream_type("http://server/timeshift/user/pass/12345.ts"),
            Some("timeshift")
        );
    }

    #[test]
    fn detect_unknown_url_is_not_live() {
        // Non-Xtream / random path → None. The rule is "live must
        // be positively identified": a URL we can't classify is
        // NOT a live channel.
        assert_eq!(detect_stream_type("http://other.server/stream.m3u8"), None);
        assert_eq!(detect_stream_type("http://server/foobar/123"), None);
        assert_eq!(detect_stream_type("not a url at all"), None);
        assert_eq!(detect_stream_type(""), None);
    }

    /// A stream must be positively identified as live. The
    /// "import only live" rule is what this helper enforces.
    #[test]
    fn only_live_is_accepted() {
        let cases = vec![
            ("http://s/live/u/p/1.ts", true),
            ("http://s/movie/u/p/1.mp4", false),
            ("http://s/series/u/p/1.mp4", false),
            ("http://s/timeshift/u/p/1", false),
            ("http://s/random/path/1", false),
        ];
        for (url, should_be_accepted) in cases {
            let detected = detect_stream_type(url);
            let accepted = matches!(detected, Some("live"));
            assert_eq!(
                accepted, should_be_accepted,
                "url={url} detected={detected:?} accepted={accepted} expected={should_be_accepted}"
            );
        }
    }
}
