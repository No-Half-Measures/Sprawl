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
