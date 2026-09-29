// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;

use crate::Failure;

pub fn json<T: Serialize>(value: &T) -> Result<(), Failure> {
    let text = serde_json::to_string_pretty(value).map_err(|e| Failure::new(e.to_string()))?;
    println!("{text}");
    Ok(())
}

/// Formats a Unix-epoch millisecond timestamp as `YYYY-MM-DD HH:MM:SS UTC`.
pub fn timestamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// `YYYY-MM-DD` (UTC).
pub fn date(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(1000).div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parses `YYYY-MM-DD` as midnight UTC in Unix milliseconds.
pub fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.trim().splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = days * 86_400_000;
    (date(ms) == s.trim()).then_some(ms)
}

/// Compact human duration: `45s`, `12m`, `1h 44m`, `3d 2h`.
pub fn duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86_400, s % 86_400 / 3600),
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Renders rows as left-aligned columns.
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let parts: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect();
        println!("{}", parts.join("  ").trim_end());
    };
    line(headers.to_vec());
    for row in rows {
        line(row.iter().map(String::as_str).collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_timestamps() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(timestamp(1_759_104_000_000), "2025-09-29 00:00:00 UTC");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29 00:00:00 UTC");
    }

    #[test]
    fn parses_and_formats_dates() {
        assert_eq!(parse_date("2026-10-14"), Some(1_791_936_000_000));
        assert_eq!(date(1_791_936_000_000), "2026-10-14");
        assert_eq!(parse_date("2000-02-29"), Some(951_782_400_000));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("tomorrow"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(duration(45_000), "45s");
        assert_eq!(duration(6_240_000), "1h 44m");
        assert_eq!(duration(266_400_000), "3d 2h");
    }
}
