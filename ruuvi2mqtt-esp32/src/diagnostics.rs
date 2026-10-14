use serde::Serialize;

use crate::scan::ScanStats;

/// The message on `<base topic>/diagnostics/<hostname>`, published every
/// cycle. The times are in ms from the start of the cycle.
#[derive(Debug, Serialize)]
pub struct Diagnostics<'a> {
    /// The package version and the git commit.
    pub firmware: &'a str,
    /// The `publisher` in the readings.
    pub client_id: &'a str,
    pub chip: &'a str,
    /// Cycles since a cold boot or a crash, this one included.
    pub cycle: u32,
    pub settings: Settings<'a>,
    /// The access point's signal, in dBm.
    pub wifi_rssi: Option<i8>,
    pub wifi_channel: Option<u8>,
    pub wifi_cache: CacheUse,
    pub free_heap: u32,
    pub scan_start_ms: u32,
    pub scan_end_ms: u32,
    /// When Wi-Fi connected.
    pub wifi_ms: u32,
    /// When the broker accepted the connection.
    pub mqtt_ms: u32,
    /// When this message was built, just before it is published.
    pub cycle_ms: u32,
    #[serde(flatten)]
    pub scan: ScanStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Diagnostics<'_> {
    /// # Panics
    ///
    /// Never: the diagnostics are plain data, which always serializes.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

/// The build-time settings that the test bed compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Settings<'a> {
    /// `parallel` or `sequential`.
    pub scan_mode: &'a str,
    /// Seconds.
    pub scan_duration: u32,
    /// `internal` or `external`. Left out when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub antenna: Option<&'a str>,
}

/// How Wi-Fi used the channel and BSSID cached in NVS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheUse {
    /// Connected with the cached channel and BSSID, without a full scan.
    Hit,
    /// Nothing cached yet, so a full scan.
    Miss,
    /// The cached values failed, so a full scan.
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mac::Mac;

    const TAG: Mac = Mac([0xF8, 0x17, 0x62, 0x4E, 0x4C, 0x0B]);

    fn diagnostics(scan: ScanStats) -> Diagnostics<'static> {
        Diagnostics {
            firmware: "ruuvi2mqtt-esp32 1.5.2-dev (c43f162)",
            client_id: "esp32-test",
            chip: "esp32c6",
            cycle: 42,
            settings: Settings {
                scan_mode: "parallel",
                scan_duration: 5,
                antenna: None,
            },
            wifi_rssi: Some(-61),
            wifi_channel: Some(6),
            wifi_cache: CacheUse::Hit,
            free_heap: 151_000,
            scan_start_ms: 180,
            scan_end_ms: 5190,
            wifi_ms: 1450,
            mqtt_ms: 2210,
            cycle_ms: 5230,
            scan,
            error: None,
        }
    }

    #[test]
    fn json() {
        let mut scan = ScanStats::default();
        scan.ruuvi(TAG, -78, 1210, false);
        scan.ruuvi(TAG, -74, 3800, true);
        scan.other(false);
        assert_eq!(
            diagnostics(scan).to_json(),
            concat!(
                r#"{"firmware":"ruuvi2mqtt-esp32 1.5.2-dev (c43f162)","client_id":"esp32-test","#,
                r#""chip":"esp32c6","cycle":42,"settings":{"scan_mode":"parallel","scan_duration":5},"#,
                r#""wifi_rssi":-61,"wifi_channel":6,"#,
                r#""wifi_cache":"hit","free_heap":151000,"scan_start_ms":180,"scan_end_ms":5190,"#,
                r#""wifi_ms":1450,"mqtt_ms":2210,"cycle_ms":5230,"#,
                r#""tags":{"f817624e4c0b":{"rssi":-74,"adverts":2,"first_ms":1210}},"#,
                r#""adverts_before_wifi":1,"adverts_after_wifi":1,"#,
                r#""other_adverts_before_wifi":1,"other_adverts_after_wifi":0}"#
            )
        );
    }

    #[test]
    fn json_with_an_error_and_all_settings() {
        let diagnostics = Diagnostics {
            settings: Settings {
                scan_mode: "sequential",
                scan_duration: 8,
                antenna: Some("external"),
            },
            wifi_rssi: None,
            wifi_channel: None,
            wifi_cache: CacheUse::Stale,
            error: Some("BLE scan thread panicked".into()),
            ..diagnostics(ScanStats::default())
        };
        let json = diagnostics.to_json();
        assert!(json.contains(
            r#""settings":{"scan_mode":"sequential","scan_duration":8,"antenna":"external"},"#
        ));
        assert!(json.contains(r#""wifi_rssi":null,"wifi_channel":null,"wifi_cache":"stale","#));
        assert!(json.contains(r#""tags":{},"#));
        assert!(json.ends_with(r#","error":"BLE scan thread panicked"}"#));
    }
}
