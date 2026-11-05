const DAY: u64 = 86_400_000;

pub fn parse(text: &str) -> Option<i64> {
    if text.len() != 10
        || text.as_bytes()[4] != b'-'
        || text.as_bytes()[7] != b'-'
        || !text
            .bytes()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let year: i64 = text[..4].parse().ok()?;
    let month: i64 = text[5..7].parse().ok()?;
    let day: i64 = text[8..].parse().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if day < 1 || day > max {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let days =
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * mp + 2) / 5 + day - 1 - 719468;
    days.checked_mul(DAY as i64)
}

pub fn format(ms: u64) -> String {
    let z = (ms / DAY) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + i64::from(m <= 2);
    let seconds = ms / 1000 % 86400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utc_dates_validate_and_format() {
        assert_eq!(parse("1970-01-01"), Some(0));
        assert_eq!(parse("1969-12-31"), Some(-86_400_000));
        assert_eq!(parse("2025-01-01"), Some(1735689600000));
        for text in ["2000-02-29", "2024-02-29", "2100-03-01", "9999-12-31"] {
            assert_eq!(
                format(parse(text).unwrap() as u64),
                format!("{text}T00:00:00Z")
            );
        }
        for text in [
            "2025-02-29",
            "1900-02-29",
            "2024-13-01",
            "2024-01-00",
            "2024-04-31",
            "2024-1-01",
            "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
        assert_eq!(format(0), "1970-01-01T00:00:00Z");
    }
    #[test]
    fn filetime_clamps_before_epoch_and_truncates_ticks() {
        for ticks in [0, 1, 116_444_735_999_999_999, 116_444_736_000_000_000] {
            assert_eq!(crate::scanner::filetime_ms(ticks), 0);
        }
        assert_eq!(crate::scanner::filetime_ms(116_444_736_000_019_999), 1);
        assert_eq!(
            crate::scanner::filetime_ms(u64::MAX),
            (u64::MAX - 116_444_736_000_000_000) / 10_000
        );
    }
}
