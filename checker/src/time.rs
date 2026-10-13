use std::time::Duration;

/// Parses `90`, `90s`, `15m` or `1h`.
pub fn parse(s: &str) -> Result<Duration, String> {
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("invalid duration '{s}'"))?;
    match unit {
        "" | "s" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_mins(n)),
        "h" => Ok(Duration::from_hours(n)),
        _ => Err(format!("invalid duration '{s}': use s, m or h")),
    }
}

/// `m:ss`, or `h:mm:ss` from an hour up.
pub fn format(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse("90"), Ok(Duration::from_secs(90)));
        assert_eq!(parse("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse("15m"), Ok(Duration::from_mins(15)));
        assert_eq!(parse("1h"), Ok(Duration::from_hours(1)));
    }

    #[test]
    fn rejects_bad_durations() {
        assert!(parse("").is_err());
        assert!(parse("m").is_err());
        assert!(parse("1d").is_err());
        assert!(parse("1.5h").is_err());
        assert!(parse("-1s").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(format(Duration::from_secs(5)), "0:05");
        assert_eq!(format(Duration::from_secs(754)), "12:34");
        assert_eq!(format(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format(Duration::from_secs(3725)), "1:02:05");
    }
}
