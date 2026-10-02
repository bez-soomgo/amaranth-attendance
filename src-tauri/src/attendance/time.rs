pub fn parse_hm(value: &str) -> Option<i32> {
    let value = value.trim();
    let parts = value.split(':').collect::<Vec<_>>();
    if !(parts.len() == 2 || parts.len() == 3) {
        return None;
    }
    let hour = parts[0].parse::<i32>().ok()?;
    let minute = parts[1].parse::<i32>().ok()?;
    let second = if parts.len() == 3 {
        parts[2].parse::<i32>().ok()?
    } else {
        0
    };
    if hour < 0 || !(0..60).contains(&minute) || !(0..60).contains(&second) {
        return None;
    }
    Some(hour * 60 + minute)
}

pub fn fmt_hm(minutes: i32) -> String {
    let minutes = i64::from(minutes);
    let sign = if minutes < 0 { "-" } else { "" };
    let absolute = minutes.abs();
    format!("{sign}{}:{:02}", absolute / 60, absolute % 60)
}

pub fn fmt_clock(minutes: i32) -> String {
    let clock = minutes.rem_euclid(1440);
    let prefix = if minutes >= 1440 { "익일 " } else { "" };
    format!("{prefix}{:02}:{:02}", clock / 60, clock % 60)
}

pub fn fmt_korean(minutes: i32) -> String {
    let minutes = i64::from(minutes);
    let sign = if minutes < 0 { "-" } else { "" };
    let absolute = minutes.abs();
    format!("{sign}{}시간 {}분", absolute / 60, absolute % 60)
}

#[cfg(test)]
mod tests {
    use super::{fmt_clock, fmt_hm, fmt_korean, parse_hm};

    #[test]
    fn parses_supported_time_strings_and_trims_whitespace() {
        assert_eq!(parse_hm("8:05"), Some(485));
        assert_eq!(parse_hm(" 08:05 "), Some(485));
        assert_eq!(parse_hm("08:05:59"), Some(485));
        assert_eq!(parse_hm("8:60"), None);
        assert_eq!(parse_hm("08:05:60"), None);
    }

    #[test]
    fn formats_durations_and_overnight_clock_values() {
        assert_eq!(fmt_hm(485), "8:05");
        assert_eq!(fmt_hm(-45), "-0:45");
        assert_eq!(fmt_clock(972), "16:12");
        assert_eq!(fmt_clock(1500), "익일 01:00");
        assert_eq!(fmt_korean(9_678), "161시간 18분");
    }
}
