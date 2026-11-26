//! The BLE scan, through esp32-nimble.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use esp32_nimble::{BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::{debug, warn};
use ruuvi2mqtt_esp32::config;
use ruuvi2mqtt_esp32::mac::Mac;
use ruuvi2mqtt_esp32::payload;
use ruuvi2mqtt_esp32::scan::ScanStats;
use ruuvi_sensor_protocol::SensorValues;

/// Ruuvi manufacturer-specific data company identifier (little-endian 0x0499).
const RUUVI_MANUFACTURER_ID: u16 = 0x0499;

/// A decoded reading, ready to publish.
#[derive(Debug, Clone)]
pub struct RuuviReading {
    pub mac: Mac,
    /// The data format, the payload's first byte.
    pub format: u8,
    /// JSON payload matching the `homeassistant::SensorData` field names used
    /// by the ruuvi2mqtt app.
    pub payload: String,
}

/// The result of one scan. The times are in ms from the start of the cycle.
#[derive(Debug, Default)]
pub struct Scan {
    pub readings: Vec<RuuviReading>,
    pub stats: ScanStats,
    pub started_ms: u32,
    pub finished_ms: u32,
}

/// Set while Wi-Fi is connected, to split the scan statistics.
static WIFI_CONNECTED: AtomicBool = AtomicBool::new(false);

pub fn set_wifi_connected(connected: bool) {
    WIFI_CONNECTED.store(connected, Ordering::Relaxed);
}

#[derive(Default)]
struct State {
    readings: HashMap<Mac, RuuviReading>,
    stats: ScanStats,
}

/// Runs one BLE scan and returns the decoded readings.
///
/// Each MAC is reported at most once per scan. The last advertisement wins,
/// except that an Air's format 6 doesn't replace its E1.
pub fn scan(start: Instant) -> anyhow::Result<Scan> {
    let ble_device = BLEDevice::take();
    let mut ble_scan = BLEScan::new();
    // A passive scan at a 100 % duty cycle. Duty cycling the BLE radio would
    // save little next to Wi-Fi and the CPU, and this way the scan catches
    // the most advertisements. Without the duplicate filter,
    // every advertisement reaches the callback, so the statistics count them
    // all.
    ble_scan
        .active_scan(false)
        .filter_duplicates(false)
        .interval(100)
        .window(100);

    let state = Mutex::new(State::default());

    debug!("Starting BLE scan...");
    let started_ms = crate::ms_since(start);

    block_on(async {
        ble_scan
            .start(
                ble_device,
                config::get().ble_scan_duration * 1000,
                |device, data| -> Option<()> {
                    let at_ms = crate::ms_since(start);
                    let wifi_connected = WIFI_CONNECTED.load(Ordering::Relaxed);
                    let mut state = state.lock().unwrap();

                    let Some(mfr_data) = data
                        .manufacture_data()
                        .filter(|m| m.company_identifier == RUUVI_MANUFACTURER_ID)
                    else {
                        state.stats.other(wifi_connected);
                        return None;
                    };

                    let addr = device.addr().as_le_bytes();
                    let mac = Mac([addr[5], addr[4], addr[3], addr[2], addr[1], addr[0]]);
                    state.stats.ruuvi(mac, device.rssi(), at_ms, wifi_connected);
                    if config::get().ble_scan_continuous {
                        crate::web::record(mac, device.rssi());
                    }

                    let format = mfr_data.payload.first().copied()?;
                    let values = match SensorValues::from_manufacturer_specific_data(
                        mfr_data.company_identifier,
                        mfr_data.payload,
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Failed to parse Ruuvi data [{mac}]: {e:?}");
                            return None;
                        }
                    };

                    let payload = payload::encode(&values, config::get().mqtt_client_id.as_str());
                    debug!("Ruuvi data: [{mac}] {} dBm -> {payload}", device.rssi());

                    if state
                        .readings
                        .get(&mac)
                        .is_none_or(|r| payload::replaces(format, r.format))
                    {
                        let reading = RuuviReading {
                            mac,
                            format,
                            payload,
                        };
                        state.readings.insert(mac, reading);
                    }
                    None
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))
            .map(|_| ())
    })?;

    debug!("BLE scan complete");
    let state = state.into_inner().unwrap();
    Ok(Scan {
        readings: state.readings.into_values().collect(),
        stats: state.stats,
        started_ms,
        finished_ms: crate::ms_since(start),
    })
}
