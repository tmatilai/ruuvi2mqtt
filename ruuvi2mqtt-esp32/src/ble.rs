use std::collections::HashMap;

use esp32_nimble::{BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::{debug, warn};
use ruuvi2mqtt_esp32::mac::Mac;
use ruuvi2mqtt_esp32::payload;
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

/// Run one BLE scan pass and return decoded Ruuvi readings.
///
/// Each MAC is reported at most once per scan (last advertisement wins).
pub fn scan_once() -> anyhow::Result<Vec<RuuviReading>> {
    let ble_device = BLEDevice::take();
    let mut ble_scan = BLEScan::new();
    // Passive scan with 100% duty cycle — the BLE radio power savings from
    // duty cycling are negligible compared to WiFi and CPU, so maximize the
    // chance of catching every tag (~2.5s advertisement interval).
    ble_scan.active_scan(false).interval(100).window(100);

    let results: std::sync::Arc<std::sync::Mutex<HashMap<Mac, RuuviReading>>> =
        std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
    let results_clone = results.clone();

    debug!("Starting BLE scan...");

    block_on(async {
        ble_scan
            .start(
                ble_device,
                config::BLE_SCAN_DURATION * 1000,
                move |device, data| -> Option<()> {
                    let mfr_data = data.manufacture_data()?;

                    if mfr_data.company_identifier != config::RUUVI_MANUFACTURER_ID {
                        return None;
                    }

                    let addr = device.addr().as_le_bytes();
                    let mac = Mac([addr[5], addr[4], addr[3], addr[2], addr[1], addr[0]]);

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
                    debug!("Ruuvi data: [{mac}] -> {payload}");

                    // Deduplicate by MAC: last advertisement wins.
                    results_clone
                        .lock()
                        .unwrap()
                        .insert(mac, RuuviReading { mac, payload });
                    None
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))
            .map(|_| ())
    })?;

    debug!("BLE scan complete");
    let readings = results.lock().unwrap().drain().map(|(_, v)| v).collect();
    Ok(readings)
}
