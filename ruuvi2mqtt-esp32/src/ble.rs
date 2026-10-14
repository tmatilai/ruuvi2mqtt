use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use esp32_nimble::{BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::{debug, warn};
use ruuvi2mqtt_esp32::mac::Mac;
use ruuvi2mqtt_esp32::payload;
use ruuvi2mqtt_esp32::scan::ScanStats;
use ruuvi_sensor_protocol::SensorValues;

use crate::config;

/// Decoded Ruuvi sensor reading ready for publishing.
#[derive(Debug, Clone)]
pub struct RuuviReading {
    pub mac: Mac,
    /// JSON payload matching the `homeassistant::SensorData` field names used
    /// by the Linux ruuvi2mqtt app.
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

/// Set once Wi-Fi has connected, to split the scan statistics.
static WIFI_CONNECTED: AtomicBool = AtomicBool::new(false);

/// Tells a running scan that Wi-Fi has connected.
pub fn wifi_connected() {
    WIFI_CONNECTED.store(true, Ordering::Relaxed);
}

#[derive(Default)]
struct State {
    readings: HashMap<Mac, RuuviReading>,
    stats: ScanStats,
}

/// Run one BLE scan pass and return decoded Ruuvi readings.
///
/// Each MAC is reported at most once per scan (last advertisement wins).
pub fn scan(start: Instant) -> anyhow::Result<Scan> {
    let ble_device = BLEDevice::take();
    let mut ble_scan = BLEScan::new();
    // Passive scan with 100% duty cycle — the BLE radio power savings from
    // duty cycling are negligible compared to WiFi and CPU, so maximize the
    // chance of catching every tag (~2.5s advertisement interval). Without
    // the duplicate filter, every advertisement reaches the callback, so the
    // statistics count them all.
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
                config::BLE_SCAN_DURATION * 1000,
                |device, data| -> Option<()> {
                    let at_ms = crate::ms_since(start);
                    let wifi_connected = WIFI_CONNECTED.load(Ordering::Relaxed);
                    let mut state = state.lock().unwrap();

                    let Some(mfr_data) = data
                        .manufacture_data()
                        .filter(|m| m.company_identifier == config::RUUVI_MANUFACTURER_ID)
                    else {
                        state.stats.other(wifi_connected);
                        return None;
                    };

                    let addr = device.addr().as_le_bytes();
                    let mac = Mac([addr[5], addr[4], addr[3], addr[2], addr[1], addr[0]]);
                    state.stats.ruuvi(mac, device.rssi(), at_ms, wifi_connected);

                    // Parse with the ruuvi-sensor-protocol crate.
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

                    let payload = payload::encode(&values, config::MQTT_CLIENT_ID);
                    debug!("Ruuvi data: [{mac}] {} dBm -> {payload}", device.rssi());

                    // Deduplicate by MAC: last advertisement wins.
                    state.readings.insert(mac, RuuviReading { mac, payload });
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
