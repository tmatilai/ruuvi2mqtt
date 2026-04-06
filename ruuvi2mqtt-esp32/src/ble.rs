use std::collections::HashMap;

use esp32_nimble::{BLEDevice, BLEScan};
use esp_idf_svc::hal::task::block_on;
use log::{debug, warn};
use ruuvi_sensor_protocol::{
    BatteryPotential, Humidity, Pressure, SensorValues, Temperature, TransmitterPower,
};
use serde::Serialize;

use crate::config;
use crate::mac::Mac;

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

                    let payload = encode_payload(&values);
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

/// JSON-serialisable sensor reading.
///
/// Field names match `homeassistant::SensorData` in the Linux crate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct SensorPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    humidity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pressure: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    battery: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    battery_low: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_power: Option<i8>,
}

/// Build the JSON payload from parsed sensor values.
#[allow(clippy::cast_precision_loss)] // sensor value ranges are well within f32 precision
fn encode_payload(v: &SensorValues) -> String {
    let temperature = v.temperature_as_millicelsius().map(|t| t as f32 / 1000.0);
    let humidity = v.humidity_as_ppm().map(|h| h as f32 / 10_000.0);
    let pressure = v.pressure_as_pascals().map(|p| p as f32 / 100.0);
    let battery = v
        .battery_potential_as_millivolts()
        .map(|mv| f32::from(mv) / 1000.0);

    // Temperature-aware battery_low logic, matching the Linux app
    // (ported from https://github.com/ruuvi/com.ruuvi.station/).
    let battery_low = match (battery, temperature) {
        (Some(volts), Some(temp)) => Some(
            (temp <= -20.0 && volts < 2.0)
                || (temp > -20.0 && temp < 0.0 && volts < 2.3)
                || (temp >= 0.0 && volts < 2.5),
        ),
        _ => None,
    };

    let tx_power = v.tx_power_as_dbm();

    let payload = SensorPayload {
        temperature,
        humidity,
        pressure,
        battery,
        battery_low,
        tx_power,
    };

    serde_json::to_string(&payload).unwrap()
}
