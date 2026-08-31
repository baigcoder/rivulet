//! M3U / M3U+ adapter.
//!
//! The M3U format is a line-oriented text file: a header line
//! `#EXTM3U`, then a sequence of `#EXTINF` records that name a
//! channel and a line that is the channel's URL. M3U+ adds
//! `tvg-id`, `tvg-name`, `tvg-logo`, `group-title`, and
//! `#EXTVLCOPT` directives for `http-user-agent` and
//! `http-referrer`.
//!
//! The body is streamed line by line — a 500K-channel playlist
//! is hundreds of MB and a `String::from_utf8` of the whole body
//! would run a phone out of memory. The free-TV `streaming_m3u`
//! importer does the same thing with the iptv-org M3U; the same
//! parser is what this module uses so the wire-level rules
//! (line ending, BOM, comment lines) match.
//!
//! `resolve_stream_url` returns `None` here: for an M3U the URL is
//! the channel's own line, already stored in the catalog by the
//! import, so the redirector reads it out of SQLite. Re-fetching a
//! several-hundred-megabyte playlist to answer one zap is not an
//! option. The playlist URL itself stays in the vault — it is the
//! account credential, and it is not what plays.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use super::errors::PremiumError;
use super::models::{
    EpgProgram, IPTVCategory, IPTVChannel, PremiumAccount,
};
use super::names;
use super::provider::{Catalog, IPTVProvider};
use super::storage::{self, PremiumState};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

pub struct M3uAdapter {
    pub state: Arc<PremiumState>,
    pub connection_id: String,
}

impl M3uAdapter {
    pub fn new(state: Arc<PremiumState>, connection_id: String) -> Self {
        Self { state, connection_id }
    }

    fn client(&self) -> Result<Client, PremiumError> {
        Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent("Rivulet/0.5 (M3U)")
            .build()
            .map_err(|e| PremiumError::Network(e.to_string()))
    }

    /// Read the URL out of the encrypted config.
    async fn url(&self) -> Result<String, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let blob = storage::get_secret(&conn, &self.connection_id)?
            .ok_or(PremiumError::ProviderNotConnected)?;
        match storage::ProviderConfig::decrypt(&blob, &self.state.vault)? {
            storage::ProviderConfig::M3u { url } => Ok(url),
            storage::ProviderConfig::Xtream { .. } => {
                Err(PremiumError::ServerError(
                    "connection is Xtream, not M3U".into(),
                ))
            }
        }
    }
}

// ── M3U line parser ───────────────────────────────────────────────

/// In-flight M3U record being built up as we read the file. One
/// `#EXTINF` line sets the metadata, the next non-comment
/// non-empty line is the URL.
#[derive(Default, Debug, Clone)]
struct PendingEntry {
    name: Option<String>,
    logo: Option<String>,
    group: Option<String>,
    tvg_id: Option<String>,
    tvg_name: Option<String>,
    user_agent: Option<String>,
    referer: Option<String>,
    url: Option<String>,
}

impl PendingEntry {
    fn into_channel(self, seen: &mut std::collections::HashSet<String>) -> Option<IPTVChannel> {
        let url = self.url?;
        // Furniture (section dividers, escaped names) is dropped here
        // rather than drawn — see `premium::names`. The last resort is
        // the URL's *host*, never the URL: an Xtream-flavoured playlist
        // carries the account's username and password in the path, and
        // a name is written to the database and rendered on a card.
        let name = self
            .name
            .as_deref()
            .and_then(names::clean_channel_name)
            .or_else(|| self.tvg_name.as_deref().and_then(names::clean_channel_name))
            .or_else(|| host_of(&url))?;
        // M3U gives no stable channel id, so the id is derived from the
        // URL — stable across re-imports (so favourites survive a
        // refresh) and unique per stream. `tvg-id` would be the nicer
        // key but it is optional, frequently blank, and frequently
        // shared by every HD/SD variant of one channel.
        let id = format!("m3u:{:016x}", hash_str(&url));
        if !seen.insert(id.clone()) {
            // The same stream listed twice. Keep the first.
            return None;
        }
        Some(IPTVChannel {
            id,
            name,
            logo_url: self.logo.filter(|s| !s.is_empty()),
            category_id: self.group.clone(),
            category_name: self.group,
            country: None,
            language: None,
            epg_id: self.tvg_id.filter(|s| !s.is_empty()),
            stream_type: Some("live".to_string()),
            user_agent: self.user_agent,
            referer: self.referer,
            stream_url: Some(url),
            is_favorite: false,
        })
    }
}

/// The host of a URL, for a channel with no name of its own. Deliberately
/// not the whole URL: an Xtream playlist puts the account's username and
/// password in the path.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    // Strip any `user:pass@` a URL is allowed to carry, and the port.
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// A tiny non-cryptographic hash for synthetic ids. Not used for
/// security — just enough entropy that two M3U URLs rarely collide.
fn hash_str(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Parse a single `#EXTINF` line. The grammar (informal) is:
///
/// ```text
/// #EXTINF:<duration> [key="value" ...] ,<display name>
/// ```
///
/// The attributes are space-separated, each `key="value"`. The
/// `,` separates the metadata from the display name. Quotes are
/// part of the format.
fn parse_extinf(line: &str) -> PendingEntry {
    let mut entry = PendingEntry::default();
    // Strip the leading `#EXTINF:` and the duration.
    let after = match line.strip_prefix("#EXTINF:") {
        Some(s) => s,
        None => return entry,
    };
    // The first comma separates the attribute block from the
    // display name. There may be a comma inside a quoted value
    // (group-title="Foo, Bar"), so find the LAST unquoted comma.
    let comma_at = last_unquoted_comma(after);
    let (attrs, name) = match comma_at {
        Some(i) => (&after[..i], after[i + 1..].trim()),
        None => (after, ""),
    };
    if !name.is_empty() {
        entry.name = Some(name.to_string());
    }
    // Walk the attribute block token by token. A `key="value"`
    // takes the whole quoted string; a bareword key is skipped
    // (most are the duration which we already threw away).
    let mut chars = attrs.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        // Read the key.
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        // Optional `=`.
        if chars.peek() == Some(&'=') {
            chars.next();
        }
        // Optional quoted value.
        if chars.peek() == Some(&'"') {
            chars.next();
            let mut value = String::new();
            while let Some(c) = chars.next() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
            apply_attr(&mut entry, &key, &value);
        }
        else {
            // Bareword value; consume until whitespace.
            let mut value = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
            if !value.is_empty() {
                apply_attr(&mut entry, &key, &value);
            }
        }
    }
    entry
}

fn apply_attr(entry: &mut PendingEntry, key: &str, value: &str) {
    match key {
        "tvg-id" => entry.tvg_id = Some(value.to_string()),
        "tvg-name" => entry.tvg_name = Some(value.to_string()),
        "tvg-logo" => entry.logo = Some(value.to_string()),
        "group-title" => entry.group = Some(value.to_string()),
        _ => {}
    }
}

fn last_unquoted_comma(s: &str) -> Option<usize> {
    let mut in_quote = false;
    let mut last = None;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            ',' if !in_quote => last = Some(i),
            _ => {}
        }
    }
    last
}

/// Parse a `#EXTVLCOPT:` line.
fn parse_vlcopt(line: &str) -> Option<(String, String)> {
    let body = line.strip_prefix("#EXTVLCOPT:")?;
    let (k, v) = body.split_once('=')?;
    Some((k.trim().to_string(), v.trim().to_string()))
}

/// The XMLTV URL a playlist advertises, from its `#EXTM3U` line.
///
/// This is where the guide URL actually lives in practice. The HTTP
/// `x-tvg-url` response header exists and some panels send it, but the
/// attribute on the first line is the convention every playlist
/// generator writes — so a reader that only looked at the header found
/// no guide on most real M3Us. Both spellings are in the wild
/// (`x-tvg-url` and `url-tvg`), and a value may be a comma-separated
/// list of mirrors, of which we take the first.
fn tvg_url_from_header_line(body: &str) -> Option<String> {
    let line = body.lines().find(|l| l.trim_start().starts_with("#EXTM3U"))?;
    for key in ["x-tvg-url", "url-tvg"] {
        // `key="…"`, then the unquoted form some generators emit.
        let quoted = format!("{key}=\"");
        let raw = if let Some(i) = line.find(&quoted) {
            let rest = &line[i + quoted.len()..];
            rest.split('"').next().unwrap_or("")
        }
        else if let Some(i) = line.find(&format!("{key}=")) {
            let rest = &line[i + key.len() + 1..];
            rest.split_whitespace().next().unwrap_or("")
        }
        else {
            continue;
        };
        let first = raw.split(',').next().unwrap_or("").trim();
        if first.starts_with("http") {
            return Some(first.to_string());
        }
    }
    None
}

/// Stream the body of an M3U response. Yields one channel per
/// `#EXTINF` + URL pair. `seen` dedupes by URL so a malformed
/// playlist with the same stream listed twice doesn't bloat the
/// database.
///
/// `MAX_CHANNELS` is a ceiling, not a target: a hostile or broken URL
/// can serve an endless body, and this parser holds its output in
/// memory before the single `replace_channels` transaction. 200K rows
/// is well past any real provider and still bounded.
const MAX_CHANNELS: usize = 200_000;

async fn stream_channels(
    resp: reqwest::Response,
) -> Result<(Vec<IPTVChannel>, Vec<IPTVCategory>), PremiumError> {
    let stream = resp
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let reader = BufReader::new(StreamReader::new(stream));
    let mut lines = reader.lines();
    let mut channels: Vec<IPTVChannel> = Vec::new();
    let mut pending = PendingEntry::default();
    let mut seen: std::collections::HashSet<String> = Default::default();
    let mut groups: std::collections::BTreeMap<String, ()> = Default::default();
    let mut truncated = false;

    while let Some(raw) = lines
        .next_line()
        .await
        .map_err(|e| PremiumError::Network(e.to_string()))?
    {
        // A UTF-8 BOM sits in front of the `#EXTM3U` on the first line
        // of a surprising number of playlists, and `\r` survives on
        // every line of a CRLF one.
        let line = raw.trim_start_matches('\u{feff}').trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#EXTINF:") {
            pending = parse_extinf(line);
        }
        else if line.starts_with("#EXTVLCOPT:") {
            if let Some((k, v)) = parse_vlcopt(line) {
                match k.as_str() {
                    "http-user-agent" => pending.user_agent = Some(v),
                    "http-referrer" => pending.referer = Some(v),
                    _ => {}
                }
            }
        }
        else if line.starts_with('#') {
            // Any other directive — `#EXTM3U`, `#EXTGRP`, `#KODIPROP`,
            // a comment. Not a URL, and not the end of the record.
            continue;
        }
        else {
            // The first non-comment line after an `#EXTINF` is the
            // channel's URL, whatever scheme it uses: providers ship
            // `rtmp://`, `rtsp://` and `udp://` alongside HTTP, and the
            // old check for an `http` prefix silently dropped all of
            // them.
            // A bare URL with no `#EXTINF` in front of it keeps no name
            // of its own; `into_channel` falls back to the host, which
            // is the part of a URL that is safe to show.
            pending.url = Some(line.to_string());
            if let Some(ch) = std::mem::take(&mut pending).into_channel(&mut seen) {
                if let Some(g) = &ch.category_name {
                    groups.entry(g.clone()).or_insert(());
                }
                channels.push(ch);
                if channels.len() >= MAX_CHANNELS {
                    truncated = true;
                    break;
                }
            }
        }
    }
    if truncated {
        eprintln!(
            "[premium] M3U playlist exceeded {MAX_CHANNELS} channels; import truncated"
        );
    }
    // The category id *is* the group name: an M3U has no separate
    // category identifier, and `group-title` is what a channel row's
    // `category_id` was set to above. Anything else and the two would
    // not join.
    let categories: Vec<IPTVCategory> = groups
        .into_keys()
        .filter_map(|name| {
            // The id stays the raw `group-title` — it is what a
            // channel's `category_id` was set to and the two have to
            // join — while the label is cleaned for display.
            let label = names::clean_channel_name(&name)?;
            Some(IPTVCategory {
                id: name,
                name: label,
                country: None,
                group: None,
            })
        })
        .collect();
    Ok((channels, categories))
}

// ── Provider impl ────────────────────────────────────────────────

#[async_trait]
impl IPTVProvider for M3uAdapter {
    async fn authenticate(&self) -> Result<PremiumAccount, PremiumError> {
        let url = self.url().await?;
        let client = self.client()?;
        // A HEAD would be cheaper, but many M3U hosts reject
        // HEAD. A GET of just the first few bytes is enough to
        // confirm the URL is reachable; we then throw the body
        // away and let `get_channels` do the real work.
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(PremiumError::from)?;
        if !resp.status().is_success() {
            return Err(PremiumError::ServerError(resp.status().to_string()));
        }
        let display = url
            .split('/')
            .next_back()
            .unwrap_or("M3U")
            .split('?')
            .next()
            .unwrap_or("M3U")
            .to_string();
        let account = PremiumAccount {
            provider_type: "m3u".to_string(),
            server_url: url.clone(),
            username: String::new(),
            status: "connected".to_string(),
            account_name: Some(display),
            expires_at: None,
            is_trial: None,
            active_connections: None,
            max_connections: None,
        };
        if let Ok(conn) = self.state.db.lock() {
            let _ = storage::update_account(
                &conn,
                &self.connection_id,
                account.account_name.as_deref(),
                None,
                None,
                None,
                None,
            );
        }
        Ok(account)
    }

    async fn get_categories(&self) -> Result<Vec<IPTVCategory>, PremiumError> {
        let (_channels, categories) = self.get_channels_with_categories().await?;
        Ok(categories)
    }

    async fn get_channels(&self) -> Result<Vec<IPTVChannel>, PremiumError> {
        let (channels, _categories) = self.get_channels_with_categories().await?;
        Ok(channels)
    }

    /// Overridden, and this is the whole reason `get_catalog` exists on
    /// the trait: an M3U's categories and channels come from one
    /// document, so the default implementation would download a
    /// several-hundred-megabyte playlist twice.
    async fn get_catalog(&self) -> Result<Catalog, PremiumError> {
        let (channels, categories) = self.get_channels_with_categories().await?;
        Ok(Catalog { categories, channels })
    }

    async fn get_epg(
        &self,
        _channel_id: &str,
        _limit: usize,
    ) -> Result<Vec<EpgProgram>, PremiumError> {
        // M3U has no per-channel EPG endpoint. The plan's EPG
        // strategy says: bulk XMLTV if the playlist carries an
        // `x-tvg-url` header, else empty. We read that header
        // here and dispatch to the bulk path on `get_bulk_epg`,
        // which the repository parses once and caches.
        Ok(Vec::new())
    }

    async fn get_bulk_epg(&self) -> Result<Option<Vec<u8>>, PremiumError> {
        // A playlist advertises its guide in one of two places: the
        // `x-tvg-url` attribute on the `#EXTM3U` line, or an HTTP
        // header of the same name. Neither is guaranteed, and a
        // playlist with no guide is normal — `None` sends the caller to
        // the per-channel fallback rather than reporting a failure.
        //
        // The `Range` header is why this is cheap enough to do on every
        // sync: the first line is all that is read, not the
        // several-hundred-megabyte body. A server that ignores `Range`
        // answers 200 with the whole playlist, so the response is read
        // as a stream and dropped after the first chunk.
        let url = self.url().await?;
        let client = self.client()?;
        let mut resp = client
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=0-65535")
            .send()
            .await
            .map_err(PremiumError::from)?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let header_url = resp
            .headers()
            .get("x-tvg-url")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut head = Vec::new();
        while head.len() < 65_536 {
            match resp.chunk().await.map_err(PremiumError::from)? {
                Some(bytes) => head.extend_from_slice(&bytes),
                None => break,
            }
        }
        drop(resp);
        let tvg_url = header_url
            .or_else(|| tvg_url_from_header_line(&String::from_utf8_lossy(&head)));
        let Some(tvg_url) = tvg_url else {
            return Ok(None);
        };
        let resp = client
            .get(&tvg_url)
            .send()
            .await
            .map_err(PremiumError::from)?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| PremiumError::Network(e.to_string()))?
            .to_vec();
        if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut out = Vec::new();
            use std::io::Read;
            decoder
                .read_to_end(&mut out)
                .map_err(|e| PremiumError::MalformedResponse(format!("gunzip: {e}")))?;
            Ok(Some(out))
        }
        else {
            Ok(Some(bytes))
        }
    }

    /// Nothing to resolve. An M3U line *is* the stream URL, and the
    /// import stored it in the channel's `stream_url` column, so the
    /// redirector reads it from SQLite instead of re-downloading the
    /// playlist on every zap.
    async fn resolve_stream_url(
        &self,
        _channel_id: &str,
    ) -> Result<Option<String>, PremiumError> {
        Ok(None)
    }
}

impl M3uAdapter {
    /// Internal helper: fetch + parse the body once, returning
    /// both the channel list and the deduped group list. The
    /// `get_categories` and `get_channels` trait methods both
    /// call this rather than hitting the network twice.
    async fn get_channels_with_categories(
        &self,
    ) -> Result<(Vec<IPTVChannel>, Vec<IPTVCategory>), PremiumError> {
        let url = self.url().await?;
        let client = self.client()?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(PremiumError::from)?;
        if !resp.status().is_success() {
            return Err(PremiumError::ServerError(resp.status().to_string()));
        }
        stream_channels(resp).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extinf_basic() {
        let line = r#"#EXTINF:-1 tvg-id="bbc.uk" tvg-name="BBC One" tvg-logo="https://example.com/bbc.png" group-title="UK",BBC One HD"#;
        let entry = parse_extinf(line);
        assert_eq!(entry.tvg_id.as_deref(), Some("bbc.uk"));
        assert_eq!(entry.tvg_name.as_deref(), Some("BBC One"));
        assert_eq!(entry.logo.as_deref(), Some("https://example.com/bbc.png"));
        assert_eq!(entry.group.as_deref(), Some("UK"));
        assert_eq!(entry.name.as_deref(), Some("BBC One HD"));
    }

    #[test]
    fn parse_extinf_handles_comma_in_group() {
        let line = r#"#EXTINF:-1 group-title="Foo, Bar",Channel"#;
        let entry = parse_extinf(line);
        assert_eq!(entry.group.as_deref(), Some("Foo, Bar"));
        assert_eq!(entry.name.as_deref(), Some("Channel"));
    }

    #[test]
    fn parse_vlcopt_user_agent() {
        let (k, v) = parse_vlcopt("#EXTVLCOPT:http-user-agent=VLC/3").unwrap();
        assert_eq!(k, "http-user-agent");
        assert_eq!(v, "VLC/3");
    }

    #[test]
    fn parse_vlcopt_referrer() {
        let (k, v) = parse_vlcopt("#EXTVLCOPT:http-referrer=https://referer/").unwrap();
        assert_eq!(k, "http-referrer");
        assert_eq!(v, "https://referer/");
    }

    #[test]
    fn tvg_url_from_extm3u_line() {
        let body = "#EXTM3U x-tvg-url=\"https://guide.example.com/xmltv.xml.gz\"\n#EXTINF:-1,A\nhttp://a\n";
        assert_eq!(
            tvg_url_from_header_line(body).as_deref(),
            Some("https://guide.example.com/xmltv.xml.gz")
        );
    }

    #[test]
    fn tvg_url_takes_first_mirror_and_alternate_spelling() {
        let body = "#EXTM3U url-tvg=\"http://one/g.xml,http://two/g.xml\"\n";
        assert_eq!(
            tvg_url_from_header_line(body).as_deref(),
            Some("http://one/g.xml")
        );
    }

    #[test]
    fn tvg_url_absent_is_none() {
        assert!(tvg_url_from_header_line("#EXTM3U\n#EXTINF:-1,A\nhttp://a\n").is_none());
        // A non-http value is a relative path or a placeholder; there is
        // nothing to fetch, and guessing a base URL would be wrong.
        assert!(tvg_url_from_header_line("#EXTM3U x-tvg-url=\"none\"\n").is_none());
    }

    fn channel_of(entry: PendingEntry) -> Option<IPTVChannel> {
        let mut seen = std::collections::HashSet::new();
        entry.into_channel(&mut seen)
    }

    /// An entry with no name of its own must never be named after its
    /// URL: an Xtream-flavoured playlist carries the account's username
    /// and password in the path, and a channel name is written to the
    /// database and drawn on a card.
    #[test]
    fn a_nameless_entry_is_named_after_its_host_only() {
        let url = "http://panel.example.com:8080/live/someuser/somepass/1.ts";
        let ch = channel_of(PendingEntry {
            url: Some(url.to_string()),
            ..Default::default()
        })
        .expect("a nameless entry is still a channel");
        assert_eq!(ch.name, "panel.example.com");
        assert!(!ch.name.contains("somepass"));
        assert!(!ch.name.contains("someuser"));
    }

    /// A playlist's section headings are rows like any other.
    #[test]
    fn divider_entries_lose_their_decoration() {
        let ch = channel_of(PendingEntry {
            name: Some("##########   Germany   ##########".to_string()),
            url: Some("http://panel.example.com/live/u/p/2.ts".to_string()),
            ..Default::default()
        })
        .expect("the row still resolves to a host");
        // The divider text is gone, and what replaced it is not a credential.
        assert_eq!(ch.name, "panel.example.com");
    }

    #[test]
    fn entry_names_are_html_decoded() {
        let ch = channel_of(PendingEntry {
            name: Some("QVC Beauty &amp; Style DE".to_string()),
            url: Some("http://example.com/a.ts".to_string()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(ch.name, "QVC Beauty & Style DE");
    }

    #[test]
    fn host_of_ignores_url_embedded_credentials() {
        assert_eq!(
            host_of("http://u:p@host.example.com:8080/x").as_deref(),
            Some("host.example.com")
        );
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn synthetic_id_stable() {
        let a = format!("m3u:{:016x}", hash_str("https://example.com/1"));
        let b = format!("m3u:{:016x}", hash_str("https://example.com/1"));
        assert_eq!(a, b);
    }
}
