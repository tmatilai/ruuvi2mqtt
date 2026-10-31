use std::iter;
use std::ops::Sub;
use std::time::Duration;

/// The longest duration that [`parse`] takes. Longer ones would overflow the
/// checker's arithmetic, and no watch or gap is that long.
const MAX: u64 = 1000 * 3600;

/// Parses `90`, `90s`, `15m` or `1h`.
pub fn parse(s: &str) -> Result<Duration, String> {
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (number, unit) = s.split_at(split);
    let n: u64 = number
        .parse()
        .map_err(|_| format!("invalid duration '{s}'"))?;
    let unit = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(format!("invalid duration '{s}': use s, m or h")),
    };
    match n.checked_mul(unit) {
        Some(secs) if secs <= MAX => Ok(Duration::from_secs(secs)),
        _ => Err(format!("invalid duration '{s}': at most 1000h")),
    }
}

/// The longest time without an event from `start` to `end`. The times must be
/// in order.
pub fn max_gap<T>(times: &[T], start: T, end: T) -> T
where
    T: Copy + Default + Ord + Sub<Output = T>,
{
    let mut gap = T::default();
    let mut previous = start;
    for &at in times.iter().chain(iter::once(&end)) {
        if at > previous {
            gap = gap.max(at - previous);
            previous = at;
        }
    }
    gap
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
        assert!(parse("1001h").is_err());
        assert!(parse("18446744073709551615h").is_err());
        assert_eq!(parse("1000h"), Ok(Duration::from_hours(1000)));
    }

    #[test]
    fn gaps_count_from_the_start_to_the_end() {
        assert_eq!(max_gap(&[], 100, 400), 300);
        assert_eq!(max_gap(&[150, 200], 100, 400), 200);
        assert_eq!(max_gap(&[350], 100, 400), 250);
        assert_eq!(max_gap(&[100, 150], 100, 160), 50);
        let secs = Duration::from_secs;
        assert_eq!(max_gap(&[secs(30)], Duration::ZERO, secs(50)), secs(30));
    }

    #[test]
    fn formats() {
        assert_eq!(format(Duration::from_secs(5)), "0:05");
        assert_eq!(format(Duration::from_secs(754)), "12:34");
        assert_eq!(format(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format(Duration::from_secs(3725)), "1:02:05");
    }
}
