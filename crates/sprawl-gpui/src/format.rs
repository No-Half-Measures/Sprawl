fn number(value: f64, decimals: usize) -> String {
    let text = format!(
        "{:.*}",
        decimals,
        (value * 10_f64.powi(decimals as i32)).round() / 10_f64.powi(decimals as i32)
    );
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let fraction = fraction.trim_end_matches('0');
    if !fraction.is_empty() {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    grouped
}

pub fn format_size(bytes: f64) -> String {
    if !bytes.is_finite() || bytes <= 0. {
        return "0 B".into();
    }
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let index = (bytes.log(1024.).floor() as usize).min(units.len() - 1);
    let value = bytes / 1024_f64.powi(index as i32);
    format!(
        "{} {}",
        number(value, usize::from(index != 0 && value < 100.)),
        units[index]
    )
}

pub fn percent(value: f64, total: f64) -> String {
    if !value.is_finite() || !total.is_finite() || value <= 0. || total <= 0. {
        return "0%".into();
    }
    let share = value / total * 100.;
    if share < 0.1 {
        "<0.1%".into()
    } else {
        format!("{}%", number(share, 1))
    }
}

pub fn count(n: u64, noun: &str) -> String {
    format!(
        "{} {noun}{}",
        number(n as f64, 0),
        if n == 1 { "" } else { "s" }
    )
}

pub fn middle_path(path: &str, limit: usize) -> String {
    let units: Vec<_> = path.encode_utf16().collect();
    if units.len() <= limit {
        return path.into();
    }
    let tail = (limit as f64 * 0.6).floor() as usize;
    let start = if tail == 0 { 0 } else { units.len() - tail };
    format!(
        "{}…{}",
        String::from_utf16_lossy(&units[..(limit as f64 * 0.4).floor() as usize]),
        String::from_utf16_lossy(&units[start..])
    )
}

pub fn age(ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    format_age(ms, now)
}

pub fn format_age(ms: u64, now: u64) -> String {
    let minutes = now.saturating_sub(ms) / 60000;
    match minutes {
        0 => "just now".into(),
        1..60 => format!("{} ago", count(minutes, "minute")),
        60..1440 => format!("{} ago", count(minutes / 60, "hour")),
        1440..2880 => "yesterday".into(),
        2880..10080 => format!("{} ago", count(minutes / 1440, "day")),
        10080..43200 => format!("{} ago", count(minutes / 10080, "week")),
        43200..525600 => format!("{} ago", count(minutes / 43200, "month")),
        _ => format!("{} ago", count(minutes / 525600, "year")),
    }
}

pub fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
pub fn format_delta(bytes: f64) -> String {
    format!(
        "{}{}",
        if bytes > 0. {
            "+"
        } else if bytes < 0. {
            "−"
        } else {
            ""
        },
        format_size(bytes.abs())
    )
}
pub fn format_date(ms: u64) -> String {
    if ms == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| d.format("%-d %b %Y").to_string())
        .unwrap_or_default()
}
pub fn format_scan_date(ms: u64) -> String {
    let offset = chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|d| *d.with_timezone(&chrono::Local).offset())
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
    format_scan_date_at(ms, now(), offset)
}
pub const AGE_BUCKETS: [(&str, &str); 7] = [
    ("<1 month", "#3fb68b"),
    ("<6 months", "#8cc152"),
    ("<1 year", "#e6c34a"),
    ("<2 years", "#f08c3c"),
    ("<5 years", "#e0533d"),
    ("5+ years", "#a33a6b"),
    ("Unknown", "#94a3b5"),
];
pub fn age_color(modified: u64, now: u64) -> &'static str {
    if modified == 0 {
        return AGE_BUCKETS[6].1;
    }
    let age = now as i128 - modified as i128;
    let index = [30, 180, 365, 730, 1825]
        .iter()
        .position(|days| age < days * 86400000)
        .unwrap_or(5);
    AGE_BUCKETS[index].1
}
pub fn not_found_by_scan(total: u64, free: u64, scanned: u64) -> Option<u64> {
    let gap = total.saturating_sub(free).saturating_sub(scanned);
    (gap as f64 >= (1024_f64.powi(3)).max(total as f64 * 0.01)).then_some(gap)
}
pub fn is_drive_root(path: &str, mount: &str) -> bool {
    path.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(mount.trim_end_matches(['\\', '/']))
}
pub fn staleness(
    saved: &sprawl_core::cache::SavedScan,
    drive: Option<&sprawl_core::backend::Drive>,
    now: u64,
) -> (bool, Option<i128>) {
    let delta = drive.zip(saved.drive_free).and_then(|(d, free)| {
        let delta = d.free as i128 - free as i128;
        ((delta.abs() as f64) >= (1024_f64.powi(3)).max(d.total as f64 * 0.01)).then_some(delta)
    });
    (now as i128 - saved.scanned_at as i128 > 7 * 86400000, delta)
}

pub fn format_scan_date_at(ms: u64, now: u64, offset: chrono::FixedOffset) -> String {
    use chrono::Datelike;
    let Some(date) = chrono::DateTime::from_timestamp_millis(ms as i64) else {
        return String::new();
    };
    let Some(current) = chrono::DateTime::from_timestamp_millis(now as i64) else {
        return String::new();
    };
    let date = date.with_timezone(&offset);
    date.format(if date.year() == current.with_timezone(&offset).year() {
        "%-d %b, %H:%M"
    } else {
        "%-d %b %Y, %H:%M"
    })
    .to_string()
}
