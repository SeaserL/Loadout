// SPDX-License-Identifier: Apache-2.0

//! Minimal, dependency-free time parsing for usage drivers.

use serde_json::Value;

/// Parses RFC 3339 / ISO 8601 date-times (`2026-10-14T01:44:00Z`,
/// `2026-10-14T01:44:00.123+08:00`, `2026-10-14 01:44:00`) into Unix milliseconds.
/// A bare date (`2026-10-14`) is midnight UTC.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, m, d) * 86_400_000;
    if bytes.len() == 10 {
        return Some(ms);
    }
    if !matches!(bytes[10], b'T' | b't' | b' ') || bytes.len() < 19 {
        return None;
    }
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    ms += (hh * 3600 + mm * 60 + ss) * 1000;
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        let padded: String = format!("{digits:0<3}").chars().take(3).collect();
        ms += padded.parse::<i64>().ok()?;
        rest = &frac[digits.len()..];
    }
    match rest {
        "" | "Z" | "z" => Some(ms),
        tz if tz.len() == 6 && (tz.starts_with('+') || tz.starts_with('-')) => {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            let oh: i64 = tz.get(1..3)?.parse().ok()?;
            let om: i64 = tz.get(4..6)?.parse().ok()?;
            Some(ms - sign * (oh * 3600 + om * 60) * 1000)
        }
        _ => None,
    }
}

/// Howard Hinnant's days-from-civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A number that may be sent as a JSON number or a numeric string.
pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// First present numeric field among `keys`.
pub fn number_field(v: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|k| v.get(*k).and_then(number))
}

/// A timestamp given as RFC 3339 text or as Unix seconds / milliseconds.
pub fn timestamp_ms(v: &Value) -> Option<i64> {
    match v {
        Value::String(s) => {
            parse_rfc3339(s).or_else(|| s.trim().parse::<f64>().ok().map(epoch_to_ms))
        }
        Value::Number(n) => n.as_f64().map(epoch_to_ms),
        _ => None,
    }
}

/// Unix seconds or milliseconds (values beyond year 2286 in seconds are taken as ms).
pub fn epoch_to_ms(v: f64) -> i64 {
    if v > 1e11 {
        v as i64
    } else {
        (v * 1000.0) as i64
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_rfc3339_variants() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-14"), Some(1_791_936_000_000));
        assert_eq!(
            parse_rfc3339("2026-10-14T01:44:00Z"),
            Some(1_791_936_000_000 + 6_240_000)
        );
        assert_eq!(
            parse_rfc3339("2026-10-14T09:44:00+08:00"),
            Some(1_791_936_000_000 + 6_240_000)
        );
        assert_eq!(
            parse_rfc3339("2026-10-14T01:44:00.5Z"),
            Some(1_791_936_000_000 + 6_240_500)
        );
        assert_eq!(
            parse_rfc3339("2026-10-14T01:44:00.123456789Z"),
            Some(1_791_936_000_000 + 6_240_123)
        );
        assert_eq!(
            parse_rfc3339("2026-10-14 01:44:00"),
            Some(1_791_936_000_000 + 6_240_000)
        );
        for bad in [
            "",
            "yesterday",
            "2026-13-01",
            "2026-10-14T25:00:00Z",
            "2026-10-14T01:44:00+8",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn tolerant_numbers_and_timestamps() {
        assert_eq!(number(&json!("38.27")), Some(38.27));
        assert_eq!(number(&json!(7)), Some(7.0));
        assert_eq!(number(&json!(null)), None);
        assert_eq!(number_field(&json!({"b": "2"}), &["a", "b"]), Some(2.0));
        assert_eq!(timestamp_ms(&json!(1_788_265_323)), Some(1_788_265_323_000));
        assert_eq!(
            timestamp_ms(&json!(1_788_265_323_000_i64)),
            Some(1_788_265_323_000)
        );
        assert_eq!(timestamp_ms(&json!("1970-01-01T00:00:01Z")), Some(1_000));
    }
}
