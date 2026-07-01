use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

pub fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value?;
    let text = value.trim();
    if text.is_empty() {
        return None;
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }

    let normalized = if let Some(stripped) = text.strip_suffix('Z') {
        format!("{stripped}+0000")
    } else {
        normalize_offset(text)
    };

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%z",
    ] {
        if let Ok(parsed) = DateTime::parse_from_str(&normalized, format) {
            return Some(parsed.with_timezone(&Utc));
        }
    }

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(text, format) {
            return Some(Utc.from_utc_datetime(&parsed));
        }
    }

    None
}

pub fn parse_filter_time(value: Option<&str>, end: bool) -> Result<Option<DateTime<Utc>>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() == 10 {
        if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            let parsed = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap());
            return Ok(Some(if end {
                parsed + chrono::Duration::days(1)
            } else {
                parsed
            }));
        }
    }
    parse_timestamp(Some(value))
        .map(Some)
        .ok_or_else(|| format!("Invalid time value: {value}"))
}

fn normalize_offset(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() >= 6 {
        let sign_at = bytes.len() - 6;
        if (bytes[sign_at] == b'+' || bytes[sign_at] == b'-')
            && bytes[sign_at + 3] == b':'
            && bytes[sign_at + 1].is_ascii_digit()
            && bytes[sign_at + 2].is_ascii_digit()
            && bytes[sign_at + 4].is_ascii_digit()
            && bytes[sign_at + 5].is_ascii_digit()
        {
            let mut normalized = String::with_capacity(text.len() - 1);
            normalized.push_str(&text[..sign_at + 3]);
            normalized.push_str(&text[sign_at + 4..]);
            return normalized;
        }
    }
    text.to_string()
}
