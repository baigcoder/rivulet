use quick_xml::events::Event;
use quick_xml::Reader;

use super::errors::IptvError;
use super::models::EpgProgram;

/// Parse XMLTV text into EPG programmes.
#[allow(dead_code)]
pub fn parse_xmltv(text: &str) -> Result<Vec<EpgProgram>, IptvError> {
    let mut reader = Reader::from_str(text);
    let mut programs = Vec::new();
    let mut buf = Vec::new();

    let mut current_channel_id = String::new();
    let mut current_title = String::new();
    let mut current_description = String::new();
    let mut current_start = String::new();
    let mut current_stop = String::new();
    let mut in_programme = false;
    let mut in_title = false;
    let mut in_desc = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "programme" => {
                        in_programme = true;
                        current_title.clear();
                        current_description.clear();
                        current_start.clear();
                        current_stop.clear();
                        current_channel_id.clear();

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val = String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "channel" => current_channel_id = val,
                                "start" => current_start = val,
                                "stop" => current_stop = val,
                                _ => {}
                            }
                        }
                    }
                    "title" if in_programme => in_title = true,
                    "desc" if in_programme => in_desc = true,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if in_title {
                    current_title.push_str(&text);
                } else if in_desc {
                    current_description.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "title" => in_title = false,
                    "desc" => in_desc = false,
                    "programme" => {
                        if !current_title.is_empty()
                            && !current_channel_id.is_empty()
                            && !current_start.is_empty()
                        {
                            programs.push(EpgProgram {
                                channel_id: current_channel_id.clone(),
                                title: current_title.clone(),
                                description: if current_description.is_empty() {
                                    None
                                } else {
                                    Some(current_description.clone())
                                },
                                start: current_start.clone(),
                                stop: if current_stop.is_empty() {
                                    None
                                } else {
                                    Some(current_stop.clone())
                                },
                            });
                        }
                        in_programme = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(IptvError::ParseError(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(programs)
}

/// Parse XMLTV and return only the programs whose `channel` attribute
/// matches `tvg_id` AND whose start time is within the next 24 hours.
/// Called by the iptv-org EPG pipeline; the full guide for a channel can
/// run into the thousands of programs, and we only want the next 24 hours.
#[allow(dead_code)]
pub fn parse_programs(text: &str, tvg_id: &str) -> Result<Vec<EpgProgram>, IptvError> {
    let all = parse_xmltv(text)?;
    let now = chrono::Utc::now().timestamp();
    let horizon = now + 24 * 3600;
    let programs = all
        .into_iter()
        .filter(|p| p.channel_id == tvg_id)
        .filter_map(|p| {
            // XMLTV stamps look like "20240101120000 +0000". Parse the
            // YYYYMMDDHHMMSS prefix and treat as UTC.
            let start_ts = parse_xmltv_ts(&p.start)?;
            let stop_ts = p
                .stop
                .as_deref()
                .and_then(parse_xmltv_ts)
                .unwrap_or(start_ts + 3600);
            // Skip programs that ended before now or start after the 24h window.
            if stop_ts < now || start_ts > horizon {
                return None;
            }
            Some(p)
        })
        .collect();
    Ok(programs)
}

/// Parse an XMLTV timestamp ("20240101120000 +0000") into a Unix epoch
/// second. Returns `None` on malformed input.
fn parse_xmltv_ts(s: &str) -> Option<i64> {
    // Strip the timezone suffix and any whitespace, then take the first
    // 14 digits (YYYYMMDDHHMMSS).
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 14 {
        return None;
    }
    let y: i32 = digits[0..4].parse().ok()?;
    let mo: u32 = digits[4..6].parse().ok()?;
    let d: u32 = digits[6..8].parse().ok()?;
    let h: u32 = digits[8..10].parse().ok()?;
    let mi: u32 = digits[10..12].parse().ok()?;
    let s: u32 = digits[12..14].parse().ok()?;
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    let time = NaiveTime::from_hms_opt(h, mi, s)?;
    let dt = NaiveDateTime::new(date, time);
    let dt_utc: DateTime<Utc> = DateTime::from_naive_utc_and_offset(dt, Utc);
    Some(dt_utc.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xmltv() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="bbc.one">
    <display-name>BBC One</display-name>
  </channel>
  <programme start="20240101120000 +0000" stop="20240101130000 +0000" channel="bbc.one">
    <title>News at Six</title>
    <desc>Evening news bulletin</desc>
  </programme>
  <programme start="20240101130000 +0000" stop="20240101140000 +0000" channel="bbc.one">
    <title>Weather</title>
  </programme>
</tv>"#;
        let programs = parse_xmltv(xml).unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].title, "News at Six");
        assert_eq!(programs[0].channel_id, "bbc.one");
        assert_eq!(
            programs[0].description,
            Some("Evening news bulletin".to_string())
        );
        assert!(programs[1].description.is_none());
    }

    #[test]
    fn test_parse_xmltv_malformed() {
        let xml = r#"<?xml version="1.0"?>
<tv>
  <programme start="20240101120000" channel="test">
    <title>Good</title>
  </programme>
  <programme start="20240101130000" channel="">
    <title>Empty Channel</title>
  </programme>
  <programme channel="test">
    <title>No Start</title>
  </programme>
</tv>"#;
        let programs = parse_xmltv(xml).unwrap();
        // Should only include the first one (second has empty channel, third has no start).
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].title, "Good");
    }
}
