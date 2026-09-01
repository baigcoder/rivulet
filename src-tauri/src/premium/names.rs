//! Channel and category name hygiene, applied once at ingest.
//!
//! A provider's lineup is not a list of channel names — it is a list of
//! *rows in a panel*, and some of those rows are furniture. Measured
//! against one real 7,052-channel Xtream lineup: 58 rows are section
//! dividers (`##########   Germany   ##########`,
//! `========== Srbija ==========`) that exist to draw a separator in a
//! set-top box's flat list, and 6 names arrive HTML-escaped
//! (`QVC Beauty &amp; Style DE`) because the panel rendered them for a
//! web page on the way out.
//!
//! Cleaning belongs here and not in a component. The name is what the
//! grid draws, what search matches and what a duplicate check compares,
//! so a name cleaned in one of those places is dirty in the other two —
//! and the store already holds the rows a divider would occupy.
//!
//! What this deliberately does *not* touch: quality and package tokens
//! (`HD`, `FHD`, `UHD`, `4K`, `HEVC`, `VIP`), country suffixes and
//! non-ASCII scripts. `beIN SPORTS 1 FHD` and `beIN SPORTS 1 HD` are two
//! streams at two bitrates, not a duplicate, and `Пе́рвый канал` is a
//! channel name and not corruption.

/// The characters a panel draws separators out of. A run of three or
/// more of any one of them is decoration; one or two is punctuation
/// inside a real name (`E!`, `Sky Sports 1/2`, `TV-3`).
const DECOR: [char; 7] = ['#', '=', '*', '_', '-', '~', '.'];

/// Named entities a panel actually emits. Not a general HTML parser:
/// numeric escapes and the five predefined names cover every one of
/// the six occurrences in the measured lineup, and an unknown entity is
/// left alone rather than mangled.
fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        // An entity is short; anything longer is a stray ampersand.
        let end = tail[1..].find(';').map(|i| i + 1).filter(|i| *i <= 10);
        let Some(end) = end else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let body = &tail[1..end];
        let decoded = match body.to_ascii_lowercase().as_str() {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => body
                .strip_prefix('#')
                .and_then(|n| {
                    n.strip_prefix('x')
                        .or_else(|| n.strip_prefix('X'))
                        .map(|h| u32::from_str_radix(h, 16))
                        .unwrap_or_else(|| n.parse::<u32>())
                        .ok()
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// True when `s` is a run of three or more of the same decorative
/// character.
fn is_decor_run(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    DECOR.contains(&first) && s.chars().count() >= 3 && chars.all(|c| c == first)
}

/// Clean one provider-supplied name.
///
/// `None` means the row is not a channel: a divider, or a name with
/// nothing legible left in it. A caller drops the row rather than
/// rendering `##########` or an empty card.
pub fn clean_channel_name(raw: &str) -> Option<String> {
    let decoded = decode_entities(raw);
    // Control characters and U+FFFD (what a mis-decoded byte becomes)
    // carry no information and are what renders as `?????`. Collapse
    // the whitespace runs a stripped character leaves behind.
    let mut words: Vec<&str> = Vec::new();
    let cleaned: String = decoded
        .chars()
        .map(|c| {
            if c == '\u{fffd}' || (c.is_control() && !c.is_whitespace()) {
                ' '
            } else if c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect();
    words.extend(cleaned.split(' ').filter(|w| !w.is_empty()));

    // A divider is decoration *around* a label. Losing the decoration
    // from one end only would leave a heading masquerading as a
    // channel, so both ends have to be furniture for the row to go.
    let decorated_start = words.first().is_some_and(|w| is_decor_run(w));
    let decorated_end = words.last().is_some_and(|w| is_decor_run(w));
    if decorated_start && decorated_end {
        return None;
    }
    // A single token that is nothing but decoration is not a name
    // either, whichever end it sits at.
    let kept: Vec<&str> = words
        .into_iter()
        .filter(|w| !is_decor_run(w))
        .collect();
    let name = kept.join(" ");
    let trimmed = name.trim_matches(|c: char| c.is_whitespace() || c == '|');
    let out = trimmed.trim();
    // Nothing legible left: a name of pure punctuation is furniture too.
    if out.is_empty() || !out.chars().any(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(out.to_string())
}

/// Detect the quality label from a channel name.
///
/// Returns `None` when no recognisable quality token is present. The
/// returned value is always uppercase and uses a standard form: "4K UHD"
/// for UHD/2160p, "4K" for bare 4K, "FHD" for 1080p, "HD" for 720p,
/// and "SD" for standard definition.
pub fn detect_quality(raw: &str) -> Option<String> {
    let upper = raw.to_uppercase();
    // Order matters: "4K UHD" before "4K", "FHD" before "HD".
    if upper.contains("4K") && (upper.contains("UHD") || upper.contains("2160")) {
        Some("4K UHD".to_string())
    } else if upper.contains("4K") {
        Some("4K".to_string())
    } else if upper.contains("2160P") {
        Some("4K".to_string())
    } else if upper.contains("FHD") || upper.contains("1080P") {
        Some("FHD".to_string())
    } else if upper.contains("HD") || upper.contains("720P") {
        Some("HD".to_string())
    } else if upper.contains("SD") || upper.contains("480P") {
        Some("SD".to_string())
    } else if upper.contains("HEVC") || upper.contains("H265") || upper.contains("H.265") {
        Some("HEVC".to_string())
    } else {
        None
    }
}

/// Detect whether a category name indicates adult content.
///
/// Matches common patterns found across Xtream panels: "Adult (18+)",
/// "XXX Channels", "Porn movies", "Erotic", etc. The match is
/// case-insensitive and requires the *word* to stand on its own —
/// "Adult Swim Kids" must not trigger.
pub fn is_adult_category(name: &str) -> bool {
    let lower = name.to_lowercase();
    let tokens = ["18+", "xxx", "adult", "porn", "erotic", "nsfw"];
    for token in &tokens {
        if lower.contains(token) {
            // "Adult Swim Kids" or "Adult Swim" are not adult content.
            // The token must not be followed by common non-adult words.
            if *token == "adult" {
                let rest = lower.split("adult").nth(1).unwrap_or("");
                let rest = rest.trim_start();
                if rest.starts_with("swim") || rest.starts_with("cartoon") {
                    continue;
                }
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_plain_name_unchanged() {
        assert_eq!(clean_channel_name("BBC One HD UK").as_deref(), Some("BBC One HD UK"));
    }

    /// The 58 rows a real lineup uses to draw section headings. They are
    /// the reason the grid showed `##########` cards.
    #[test]
    fn drops_divider_rows() {
        for divider in [
            "##########   Germany   ##########",
            "========== Srbija ==========",
            "========== BIH Kanali ==========",
            "**** SPORTS ****",
            "---------- VOD ----------",
            "~~~ Kids ~~~",
        ] {
            assert_eq!(clean_channel_name(divider), None, "{divider}");
        }
    }

    #[test]
    fn drops_names_with_nothing_legible() {
        assert_eq!(clean_channel_name(""), None);
        assert_eq!(clean_channel_name("   "), None);
        assert_eq!(clean_channel_name("###"), None);
        assert_eq!(clean_channel_name("---"), None);
        assert_eq!(clean_channel_name("\u{fffd}\u{fffd}\u{fffd}"), None);
        assert_eq!(clean_channel_name("|"), None);
    }

    /// Panels emit names they escaped for a web page.
    #[test]
    fn decodes_html_entities() {
        assert_eq!(
            clean_channel_name("QVC Beauty &amp; Style DE").as_deref(),
            Some("QVC Beauty & Style DE"),
        );
        assert_eq!(clean_channel_name("A &lt;B&gt; C").as_deref(), Some("A <B> C"));
        assert_eq!(clean_channel_name("It&#39;s TV").as_deref(), Some("It's TV"));
        assert_eq!(clean_channel_name("Ampersand &amp;amp; twice").as_deref(), Some("Ampersand &amp; twice"));
    }

    /// An unknown or malformed entity is a literal, not something to
    /// guess at: a name is the user's to read, not ours to invent.
    #[test]
    fn leaves_unknown_entities_alone() {
        assert_eq!(clean_channel_name("Rock & Roll TV").as_deref(), Some("Rock & Roll TV"));
        assert_eq!(clean_channel_name("Sport &unknown; 1").as_deref(), Some("Sport &unknown; 1"));
        assert_eq!(clean_channel_name("Cats & Dogs&").as_deref(), Some("Cats & Dogs&"));
    }

    /// Quality and package tokens separate two real streams. Stripping
    /// them here is what would turn a lineup into "duplicates".
    #[test]
    fn preserves_quality_and_package_tokens() {
        for name in [
            "beIN SPORTS 1 FHD",
            "beIN SPORTS 1 HD",
            "Sky Sport UHD DE",
            "DAZN 1 4K",
            "RTL HEVC DE",
            "BBC One East E VIP UK",
            "MTV SD",
        ] {
            assert_eq!(clean_channel_name(name).as_deref(), Some(name), "{name}");
        }
    }

    /// Punctuation inside a name is not decoration.
    #[test]
    fn preserves_punctuation_inside_names() {
        for name in ["E! Entertainment", "TV-3 Sport", "Sky Sports F1", "A&E HD", "Kanal 5 - Plus"] {
            assert_eq!(clean_channel_name(name).as_deref(), Some(name), "{name}");
        }
    }

    /// A lineup is mostly not English. Nothing here may touch a script
    /// it does not recognise.
    #[test]
    fn preserves_international_names() {
        for name in [
            "Первый канал",
            "قناة الجزيرة",
            "中央电视台 1",
            "NRK1 Nordnytt",
            "Télé-Québec",
            "ΕΡΤ1 HD",
            "日本テレビ",
        ] {
            assert_eq!(clean_channel_name(name).as_deref(), Some(name), "{name}");
        }
    }

    /// A decorated *prefix* alone is a channel with an ugly name, not a
    /// heading — the label is kept and the furniture goes.
    #[test]
    fn strips_one_sided_decoration_but_keeps_the_channel() {
        assert_eq!(clean_channel_name("### Sky Cinema").as_deref(), Some("Sky Cinema"));
        assert_eq!(clean_channel_name("Sky Cinema ***").as_deref(), Some("Sky Cinema"));
    }

    #[test]
    fn collapses_whitespace_and_control_characters() {
        assert_eq!(clean_channel_name("  Sky   Sports \t 1 \n").as_deref(), Some("Sky Sports 1"));
        assert_eq!(clean_channel_name("Sky\u{0000}Sports").as_deref(), Some("Sky Sports"));
        assert_eq!(clean_channel_name("Sky \u{fffd} Sports").as_deref(), Some("Sky Sports"));
    }

    #[test]
    fn detect_quality_labels() {
        assert_eq!(detect_quality("Sky Sport 4K UHD").as_deref(), Some("4K UHD"));
        assert_eq!(detect_quality("DAZN 1 4K").as_deref(), Some("4K"));
        assert_eq!(detect_quality("Channel 2160p").as_deref(), Some("4K"));
        assert_eq!(detect_quality("beIN SPORTS 1 FHD").as_deref(), Some("FHD"));
        assert_eq!(detect_quality("Channel 1080p").as_deref(), Some("FHD"));
        assert_eq!(detect_quality("BBC One HD").as_deref(), Some("HD"));
        assert_eq!(detect_quality("Channel 720p").as_deref(), Some("HD"));
        assert_eq!(detect_quality("MTV SD").as_deref(), Some("SD"));
        assert_eq!(detect_quality("Channel 480p").as_deref(), Some("SD"));
        assert_eq!(detect_quality("RTL HEVC DE").as_deref(), Some("HEVC"));
        assert_eq!(detect_quality("BBC One"), None);
        assert_eq!(detect_quality("Sky Sports 1"), None);
    }

    #[test]
    fn detect_adult_categories() {
        assert!(is_adult_category("Adult (18+)"));
        assert!(is_adult_category("XXX Channels"));
        assert!(is_adult_category("Porn movies"));
        assert!(is_adult_category("Erotic HD"));
        assert!(is_adult_category("Adult VOD"));
        assert!(is_adult_category("ALL ADULT"));
        assert!(is_adult_category("18+"));
        assert!(!is_adult_category("Kids"));
        assert!(!is_adult_category("Sports"));
        assert!(!is_adult_category("News 24"));
        assert!(!is_adult_category("Adult Swim Kids"));
    }
}
