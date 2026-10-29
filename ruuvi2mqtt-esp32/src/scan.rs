use std::collections::BTreeMap;

use serde::Serialize;

use crate::mac::Mac;

/// One tag's advertisements in a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TagStats {
    /// The strongest RSSI, in dBm.
    pub rssi: i8,
    /// Ruuvi advertisements received.
    pub adverts: u32,
    /// When the first one arrived, in ms from the start of the cycle.
    pub first_ms: u32,
}

/// The advertisements received in a scan.
///
/// The counters are split by whether Wi-Fi was up. In the deep-sleep mode,
/// that is before and after it connected.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ScanStats {
    pub tags: BTreeMap<Mac, TagStats>,
    /// Ruuvi advertisements before Wi-Fi connected.
    pub adverts_before_wifi: u32,
    /// Ruuvi advertisements after Wi-Fi connected.
    pub adverts_after_wifi: u32,
    /// Other advertisements, a measure of the scan's airtime.
    pub other_adverts_before_wifi: u32,
    pub other_adverts_after_wifi: u32,
}

impl ScanStats {
    /// Records a Ruuvi advertisement that arrived `at_ms` from the start of
    /// the cycle.
    pub fn ruuvi(&mut self, mac: Mac, rssi: i8, at_ms: u32, wifi_connected: bool) {
        self.tags
            .entry(mac)
            .and_modify(|tag| {
                tag.rssi = tag.rssi.max(rssi);
                tag.adverts += 1;
            })
            .or_insert(TagStats {
                rssi,
                adverts: 1,
                first_ms: at_ms,
            });
        if wifi_connected {
            self.adverts_after_wifi += 1;
        } else {
            self.adverts_before_wifi += 1;
        }
    }

    /// Records any other advertisement.
    pub fn other(&mut self, wifi_connected: bool) {
        if wifi_connected {
            self.other_adverts_after_wifi += 1;
        } else {
            self.other_adverts_before_wifi += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
    const OTHER_TAG: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02]);

    #[test]
    fn keeps_the_strongest_rssi_and_the_first_arrival() {
        let mut stats = ScanStats::default();
        stats.ruuvi(TAG, -80, 400, false);
        stats.ruuvi(TAG, -72, 1700, false);
        stats.ruuvi(TAG, -90, 3000, true);
        assert_eq!(
            stats.tags[&TAG],
            TagStats {
                rssi: -72,
                adverts: 3,
                first_ms: 400
            }
        );
    }

    #[test]
    fn splits_the_counts_when_wifi_connects() {
        let mut stats = ScanStats::default();
        stats.ruuvi(TAG, -80, 400, false);
        stats.ruuvi(OTHER_TAG, -60, 900, false);
        stats.ruuvi(TAG, -80, 2500, true);
        stats.other(false);
        stats.other(true);
        stats.other(true);
        assert_eq!(stats.tags.len(), 2);
        assert_eq!(
            (stats.adverts_before_wifi, stats.adverts_after_wifi),
            (2, 1)
        );
        assert_eq!(
            (
                stats.other_adverts_before_wifi,
                stats.other_adverts_after_wifi
            ),
            (1, 2)
        );
    }
}
