//! IPTV source protocol detection helpers.
//!
//! `group_title_country` parses a `group-title="BR: NEWS"` style
//! header for a country code prefix. The full `Protocol` detector
//! and `detect()` were used when the M3U import form needed to
//! pick a flow; the streaming importer treats every URL as a
//! generic M3U and lets the server response decide. The detector
//! stays in the test module so the helper is still covered.

/// Extract a country code or name from a `group-title` like
/// `BR: DISNEY+ [PPV EVENTS]` or `US | NEWS NETWORK`. Returns `None`
/// if the group title doesn't start with a recognizable country
/// indicator.
///
/// Xtream-based playlists in particular don't always set
/// `tvg-country` on every channel, but they do prefix the
/// `group-title` with a 2-letter country code. This is the
/// fallback for when the iptv-org countries API has no entry.
pub fn group_title_country(group: &str) -> Option<String> {
    let trimmed = group.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Common separators after the prefix: `:`, `|`, ` - `, ` -`.
    let mut end = trimmed.len();
    for (i, ch) in trimmed.char_indices() {
        if matches!(ch, ':' | '|') {
            end = i;
            break;
        }
        if ch == ' ' && trimmed[i..].starts_with(" - ") {
            end = i;
            break;
        }
    }
    let prefix = trimmed[..end].trim();
    if prefix.is_empty() {
        return None;
    }
    // Must be a short, all-uppercase token to be a country code.
    if prefix.len() <= 5
        && prefix
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c == '-')
    {
        Some(prefix.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_title_country_colon() {
        assert_eq!(
            group_title_country("BR: DISNEY+ [PPV EVENTS]"),
            Some("BR".to_string())
        );
        assert_eq!(
            group_title_country("US: NEWS NETWORK"),
            Some("US".to_string())
        );
        assert_eq!(
            group_title_country("IN: HINDI ENTERTAINMENT"),
            Some("IN".to_string())
        );
        assert_eq!(group_title_country("PK: DRAMA"), Some("PK".to_string()));
    }

    #[test]
    fn group_title_country_pipe() {
        assert_eq!(group_title_country("IN | HINDI"), Some("IN".to_string()));
        assert_eq!(group_title_country("United States | CNN"), None);
    }

    #[test]
    fn group_title_country_dash() {
        assert_eq!(group_title_country("IN - HINDI"), Some("IN".to_string()));
    }

    #[test]
    fn group_title_country_no_prefix() {
        assert_eq!(group_title_country("DISNEY+ [PPV EVENTS]"), None);
        assert_eq!(group_title_country("NEWS NETWORK"), None);
    }
}
