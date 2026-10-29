//! When to switch to a stronger access point, scanning continuously.

use crate::mac::Mac;
use std::time::Duration;

/// Below this, the board scans for a stronger access point, before Wi-Fi
/// starts to drop at about −75 dBm.
pub const THRESHOLD: i8 = -70;

/// How much stronger another access point must be, so that the board
/// doesn't flip between two about equal ones.
const MARGIN: i8 = 8;

/// While the link stays weak and a scan found no stronger access point,
/// the next scan waits this long. A stronger one may come back, such as
/// after its own reboot.
pub const RESCAN: Duration = Duration::from_secs(10 * 60);

/// An access point of the network, as a scan found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ap {
    pub bssid: Mac,
    pub channel: u8,
    pub rssi: i8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Disconnect, and reconnect to this one.
    Switch(Ap),
    /// `best` is the strongest other access point, for the log.
    Stay { best: Option<i8> },
}

/// Whether to leave `current`, at `rssi`, for one of the `scanned` access
/// points of the same network.
#[must_use]
pub fn decide(current: Mac, rssi: i8, scanned: &[Ap]) -> Decision {
    let best = scanned
        .iter()
        .filter(|ap| ap.bssid != current)
        .max_by_key(|ap| ap.rssi);
    match best {
        Some(&ap) if ap.rssi >= rssi.saturating_add(MARGIN) => Decision::Switch(ap),
        _ => Decision::Stay {
            best: best.map(|ap| ap.rssi),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALL: Mac = Mac([0x02, 0, 0, 0, 0, 0x01]);
    const GARAGE: Mac = Mac([0x02, 0, 0, 0, 0, 0x02]);
    const SAUNA: Mac = Mac([0x02, 0, 0, 0, 0, 0x03]);

    fn ap(bssid: Mac, rssi: i8) -> Ap {
        Ap {
            bssid,
            channel: 1,
            rssi,
        }
    }

    #[test]
    fn switches_to_a_clearly_stronger_one() {
        assert_eq!(
            decide(HALL, -74, &[ap(HALL, -73), ap(GARAGE, -66), ap(SAUNA, -60)]),
            Decision::Switch(ap(SAUNA, -60))
        );
    }

    #[test]
    fn stays_when_none_is_clearly_stronger() {
        assert_eq!(
            decide(HALL, -74, &[ap(HALL, -60), ap(GARAGE, -67)]),
            Decision::Stay { best: Some(-67) }
        );
        assert_eq!(decide(HALL, -72, &[]), Decision::Stay { best: None });
    }
}
