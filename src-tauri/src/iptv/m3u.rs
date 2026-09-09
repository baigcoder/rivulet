//! M3U parser and helpers shared with the streaming importer.
//!
//! `streaming_m3u::stream_into_source` does the actual line-by-line
//! import work; this file owns the regex shapes the parser reuses, the
//! public `ExtinfData` struct that the importer holds between lines,
//! and the parser tests. The previous single-shot `fetch_m3u_playlist`
//! that buffered the entire body into a `String` is gone — that was
//! the source of the 1 GB string the new architecture replaces.

use regex::Regex;
#[cfg(test)]
use std::collections::HashSet;

#[cfg(test)]
use super::countries::Country;
#[cfg(test)]
use super::errors::IptvError;
#[cfg(test)]
use super::models::LiveChannel;

/// The bundled free-TV playlist: Free-TV/IPTV, a hand-curated public
/// list that drops a channel when it stops answering. iptv-org's
/// `index.m3u` was here before and is six times the size, which is the
/// problem — it is a directory of every stream anyone has ever seen, so
/// a third of it is dead on any given day and the app carried the
/// difference. Nothing else was ever read from the list this replaced:
/// only index 0 had a caller, so the "fallback chain" fell back nowhere.
///
/// Curation is not health, so it is only half the answer — the other
/// half is `app/utils/livehealth.ts`, which probes what is on screen and
/// zaps past a channel that fails to open.
const FREE_TV_PLAYLIST: &str = "https://raw.githubusercontent.com/Free-TV/IPTV/master/playlist.m3u8";

/// Per-country playlists imported alongside the curated list, because the
/// curated list simply has no channels for these countries — there is no
/// Pakistan group in it at all, and no Pakistani broadcaster under any other
/// group either. The rest of South Asia is the same gap. iptv-org's published
/// per-country file is the free source that does carry them, with logos and
/// real category names.
///
/// The `countries/` file rather than `streams/`: same channels, but the
/// published one adds `tvg-logo` and a `group-title`, and a card with no
/// logo and no category is a worse channel than a missing one.
///
/// What it does *not* carry is a country on each line — a per-country
/// playlist knows its country and doesn't repeat it — hence the code here,
/// which the importer applies as its last fallback.
const COUNTRY_SUPPLEMENTS: &[(&str, &str)] = &[
    ("PK", "https://iptv-org.github.io/iptv/countries/pk.m3u"),
    ("IN", "https://iptv-org.github.io/iptv/countries/in.m3u"),
    ("BD", "https://iptv-org.github.io/iptv/countries/bd.m3u"),
    ("LK", "https://iptv-org.github.io/iptv/countries/lk.m3u"),
    ("NP", "https://iptv-org.github.io/iptv/countries/np.m3u"),
    ("AF", "https://iptv-org.github.io/iptv/countries/af.m3u"),
];

/// Every playlist that makes up Free TV, as `(country code, url)`. The
/// curated worldwide list comes first: it is the big one, and whichever
/// import runs first is the one that clears the old channels out.
pub fn free_playlists() -> Vec<(Option<&'static str>, &'static str)> {
    let mut all = vec![(None, FREE_TV_PLAYLIST)];
    all.extend(COUNTRY_SUPPLEMENTS.iter().map(|(cc, url)| (Some(*cc), *url)));
    all
}

/// Bump when the importer changes what it *stores* for the same playlist, so
/// that an install already holding the old rows re-imports once instead of
/// keeping them until someone finds the Refresh button. `r3` reads the M3U
/// comma title — the only name iptv-org's per-country lists carry — and files
/// a supplement's channels under their country.
const IMPORT_REVISION: &str = "r3";

/// Identifies the *set* of playlists on disk and how they were parsed, so that
/// adding, removing or re-reading one re-imports once. Space-separated because
/// this is stored inside a JSON string and compared with a substring test — a
/// newline would be escaped on the way in and never match again (see
/// `sources::free_playlist_changed`).
pub fn free_playlist_key() -> String {
    let urls: Vec<&str> = free_playlists().iter().map(|(_, url)| *url).collect();
    format!("{IMPORT_REVISION} {}", urls.join(" "))
}

pub struct ExtinfData {
    pub tvg_id: Option<String>,
    /// The display name after the attributes — `#EXTINF:-1 attrs,Aaj News`.
    /// The M3U spec's own channel title, and the only name a playlist that
    /// writes no `tvg-name` gives you: without it those channels were named
    /// from their URL, which is how a news channel ended up called
    /// "113328724".
    pub title: Option<String>,
    pub tvg_name: Option<String>,
    pub tvg_logo: Option<String>,
    pub group: Option<String>,
    pub country: Option<String>,
    pub language: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
}

impl Default for ExtinfData {
    fn default() -> Self {
        Self {
            tvg_id: None,
            title: None,
            tvg_name: None,
            tvg_logo: None,
            group: None,
            country: None,
            language: None,
            user_agent: None,
            referer: None,
        }
    }
}

/// The display name of an `#EXTINF` line: everything after the first comma
/// that is not inside a quoted attribute value.
///
/// The first comma outside quotes rather than the last comma in the line,
/// because both halves can contain one — `tvg-id="A,B"` on the left and
/// "Aaj News, Karachi" on the right — and splitting on the wrong one either
/// swallows the name or cuts it in half.
pub fn extinf_title(line: &str) -> Option<String> {
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                let title = line[i + 1..].trim();
                return (!title.is_empty()).then(|| title.to_string());
            }
            _ => {}
        }
    }
    None
}

/// Public version of `parse_extinf` so the streaming importer can reuse
/// the same regex shapes. The fields are identical to the private
/// function below.
#[allow(clippy::too_many_arguments)]
pub fn parse_extinf_pub(
    line: &str,
    extinf_re: Option<&Regex>,
    tvg_name_re: Option<&Regex>,
    tvg_logo_re: Option<&Regex>,
    group_re: Option<&Regex>,
    country_re: Option<&Regex>,
    language_re: Option<&Regex>,
) -> ExtinfData {
    fn extract(re: Option<&Regex>, line: &str) -> Option<String> {
        re.and_then(|r| r.captures(line))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
    }
    ExtinfData {
        tvg_id: extract(extinf_re, line),
        title: extinf_title(line),
        tvg_name: extract(tvg_name_re, line),
        tvg_logo: extract(tvg_logo_re, line),
        group: extract(group_re, line),
        country: extract(country_re, line),
        language: extract(language_re, line),
        user_agent: None,
        referer: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn parse_m3u_simple(m3u: &str) -> Result<Vec<LiveChannel>, IptvError> {
        let mut channels: Vec<LiveChannel> = Vec::new();
        let mut seen_urls: HashSet<String> = HashSet::new();
        let extinf_re = Regex::new(r#"tvg-id="([^"]*)""#).ok();
        let tvg_name_re = Regex::new(r#"tvg-name="([^"]*)""#).ok();
        let tvg_logo_re = Regex::new(r#"tvg-logo="([^"]*)""#).ok();
        let group_re = Regex::new(r#"group-title="([^"]*)""#).ok();
        let country_re = Regex::new(r#"tvg-country="([^"]*)""#).ok();
        let language_re = Regex::new(r#"tvg-language="([^"]*)""#).ok();

        let mut pending: Option<ExtinfData> = None;
        for line in m3u.lines() {
            let line = line.trim();
            if line.starts_with("#EXTINF:") {
                pending = Some(parse_extinf_pub(
                    line,
                    extinf_re.as_ref(),
                    tvg_name_re.as_ref(),
                    tvg_logo_re.as_ref(),
                    group_re.as_ref(),
                    country_re.as_ref(),
                    language_re.as_ref(),
                ));
            } else if line.starts_with("#EXTVLCOPT:") {
                if let Some(extinf) = pending.as_mut() {
                    let directive = line.trim_start_matches("#EXTVLCOPT:").trim();
                    if let Some(value) = directive.strip_prefix("http-user-agent=") {
                        extinf.user_agent = Some(value.to_string());
                    } else if let Some(value) = directive.strip_prefix("http-referrer=") {
                        extinf.referer = Some(value.to_string());
                    }
                }
            } else if !line.is_empty() && !line.starts_with('#') {
                let stream_url = line.to_string();
                if stream_url.starts_with("http://") || stream_url.starts_with("https://") {
                    if seen_urls.insert(stream_url.clone()) {
                        let extinf = pending.take().unwrap_or_default();
                        let name = extinf
                            .tvg_name
                            .clone()
                            .unwrap_or_else(|| stream_url.clone());
                        channels.push(LiveChannel {
                            id: extinf
                                .tvg_id
                                .clone()
                                .unwrap_or_else(|| format!("free-{}", channels.len())),
                            name,
                            logo_url: extinf.tvg_logo,
                            stream_url: Some(stream_url),
                            category_id: None,
                            category_name: extinf.group,
                            country: None,
                            country_code: None,
                            country_flag: None,
                            language: extinf.language,
                            epg_id: extinf.tvg_id,
                            stream_type: None,
                            user_agent: extinf.user_agent,
                            referer: extinf.referer,
                        });
                    }
                } else {
                    pending = None;
                }
            }
        }
        Ok(channels)
    }

    /// A playlist with no `tvg-name` still has a name, and it is on the same
    /// line — see `extinf_title`.
    #[test]
    fn title_is_read_from_after_the_attributes() {
        assert_eq!(
            extinf_title(r#"#EXTINF:-1 tvg-id="8XM.pk@SD",8XM (576p)"#).as_deref(),
            Some("8XM (576p)"),
        );
        // A comma inside an attribute value is not the separator.
        assert_eq!(
            extinf_title(r#"#EXTINF:-1 tvg-id="A,B" group-title="News",Aaj News, Karachi"#)
                .as_deref(),
            Some("Aaj News, Karachi"),
        );
        // Nothing after the comma, and no comma at all, are both "no title".
        assert_eq!(extinf_title("#EXTINF:-1 tvg-id=\"x\",   "), None);
        assert_eq!(extinf_title("#EXTINF:-1"), None);
    }

    #[test]
    fn test_parse_m3u_simple() {
        let m3u = r#"#EXTM3U
#EXTINF:-1 tvg-id="geo.us" tvg-name="Geo News" tvg-logo="https://example.com/logo.png" group-title="News",Geo News
https://example.com/stream1.m3u8
#EXTINF:-1 tvg-id="bbc.uk" tvg-name="BBC News" group-title="News",BBC News
https://example.com/stream2.m3u8
"#;
        let channels = parse_m3u_simple(m3u).unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Geo News");
        assert_eq!(channels[0].epg_id, Some("geo.us".to_string()));
        assert_eq!(channels[0].category_name, Some("News".to_string()));
        assert_eq!(
            channels[0].logo_url,
            Some("https://example.com/logo.png".to_string())
        );
        assert_eq!(channels[1].name, "BBC News");
    }

    #[test]
    fn test_parse_m3u_dedup() {
        let m3u = r#"#EXTM3U
#EXTINF:-1 tvg-name="Channel A",Channel A
https://example.com/stream.m3u8
#EXTINF:-1 tvg-name="Channel A Duplicate",Channel A
https://example.com/stream.m3u8
"#;
        let channels = parse_m3u_simple(m3u).unwrap();
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn test_parse_m3u_skip_invalid() {
        let m3u = r#"#EXTM3U
#EXTINF:-1 tvg-name="Good Channel",Good Channel
https://example.com/stream.m3u8
#EXTINF:-1 tvg-name="Bad Channel",Bad Channel
ftp://invalid-url
#EXTINF:-1 tvg-name="Another Good",Another Good
https://example.com/stream2.m3u8
"#;
        let channels = parse_m3u_simple(m3u).unwrap();
        assert_eq!(channels.len(), 2);
    }

    #[test]
    fn test_parse_m3u_extvlcopt() {
        let m3u = r#"#EXTM3U
#EXTINF:-1 tvg-id="Geo.us" tvg-name="Geo News" group-title="News",Geo News
#EXTVLCOPT:http-user-agent=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36
#EXTVLCOPT:http-referrer=https://geo.tv/
https://geo.tv/stream.m3u8
"#;
        let channels = parse_m3u_simple(m3u).unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(
            channels[0].user_agent.as_deref(),
            Some("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        );
        assert_eq!(channels[0].referer.as_deref(), Some("https://geo.tv/"));
    }

    #[test]
    fn test_parse_m3u_country_lookup() {
        let mut countries = HashMap::new();
        countries.insert(
            "PK".to_string(),
            Country {
                name: "Pakistan".to_string(),
                code: "PK".to_string(),
                languages: vec!["urd".to_string()],
                flag: "🇵🇰".to_string(),
            },
        );
        let m3u = r#"#EXTM3U
#EXTINF:-1 tvg-id="GeoNews.pk" tvg-name="Geo News" tvg-country="PK" group-title="News",Geo News
https://example.com/stream.m3u8
"#;
        let channels = parse_m3u_simple(m3u).unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Geo News");
    }
}
