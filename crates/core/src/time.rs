//! The two forms of "now" the data files use: JS `Date.now()` and
//! `new Date().toISOString()`.
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `2026-10-13T09:30:00.000Z`, like `Date.prototype.toISOString`.
pub fn iso_string(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

pub fn now_iso() -> String {
    iso_string(now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_string_matches_js() {
        assert_eq!(iso_string(0), "1970-01-01T00:00:00.000Z");
        // Values from node: new Date(ms).toISOString()
        assert_eq!(iso_string(1_791_883_800_123), JS_2026);
        assert_eq!(iso_string(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    const JS_2026: &str = "2026-10-13T09:30:00.123Z";
}
