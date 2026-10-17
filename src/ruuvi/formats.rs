use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::ruuvi::{BDAddr, SensorData};

/// Data format 6, an Air's legacy advertisement
const FORMAT_6: u8 = 6;
/// Data format E1, an Air's extended advertisement
const FORMAT_E1: u8 = 0xE1;

/// How long an E1 reading holds off format 6. E1 arrives about every
/// 300 ms while it's received at all.
const E1_WINDOW: Duration = Duration::from_mins(1);

/// An Air sends data formats 6 and E1 from one address. Only E1 has PM1, PM4
/// and PM10, so Ruuvi says to drop format 6 when both arrive. Otherwise the
/// published reading comes from whichever wins the throttle, and those
/// values come and go. Format 6 still serves when E1 isn't received, e.g.
/// with a Bluetooth 4 adapter.
#[derive(Default)]
pub struct FormatFilter {
    last_e1: HashMap<BDAddr, Instant>,
}

impl FormatFilter {
    pub fn keep(&mut self, data: &SensorData) -> bool {
        self.keep_at(data.bdaddr, data.format, Instant::now())
    }

    fn keep_at(&mut self, bdaddr: BDAddr, format: u8, now: Instant) -> bool {
        match format {
            FORMAT_E1 => {
                self.last_e1.insert(bdaddr, now);
                true
            }
            FORMAT_6 => self
                .last_e1
                .get(&bdaddr)
                .is_none_or(|last| now.duration_since(*last) >= E1_WINDOW),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn air() -> BDAddr {
        BDAddr::from([1, 2, 3, 4, 5, 6])
    }

    #[test]
    fn keeps_format_6_without_e1() {
        let mut filter = FormatFilter::default();
        assert!(filter.keep_at(air(), FORMAT_6, Instant::now()));
    }

    #[test]
    fn drops_format_6_after_e1() {
        let mut filter = FormatFilter::default();
        let start = Instant::now();
        assert!(filter.keep_at(air(), FORMAT_E1, start));
        assert!(!filter.keep_at(air(), FORMAT_6, start + Duration::from_secs(59)));
        assert!(filter.keep_at(air(), FORMAT_E1, start + Duration::from_secs(59)));
    }

    #[test]
    fn keeps_format_6_when_e1_stops() {
        let mut filter = FormatFilter::default();
        let start = Instant::now();
        filter.keep_at(air(), FORMAT_E1, start);
        assert!(filter.keep_at(air(), FORMAT_6, start + E1_WINDOW));
    }

    #[test]
    fn e1_holds_off_only_its_device() {
        let mut filter = FormatFilter::default();
        let start = Instant::now();
        filter.keep_at(air(), FORMAT_E1, start);
        assert!(filter.keep_at(BDAddr::from([1, 2, 3, 4, 5, 7]), FORMAT_6, start));
    }

    #[test]
    fn keeps_other_formats() {
        let mut filter = FormatFilter::default();
        let start = Instant::now();
        filter.keep_at(air(), FORMAT_E1, start);
        assert!(filter.keep_at(air(), 5, start));
    }
}
