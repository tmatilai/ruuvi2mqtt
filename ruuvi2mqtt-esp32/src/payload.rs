use ruuvi_sensor_protocol::{
    BatteryPotential, Humidity, Pressure, SensorValues, Temperature, TransmitterPower,
};
use serde::Serialize;

/// JSON-serialisable sensor reading.
///
/// Field names match `homeassistant::SensorData` in the Linux crate.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct SensorPayload<'a> {
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
    publisher: &'a str,
}

/// Build the JSON payload from parsed sensor values. `publisher` is the MQTT
/// client ID.
///
/// # Panics
///
/// Never: the payload is plain data, which always serializes.
#[allow(clippy::cast_precision_loss)] // sensor value ranges are well within f32 precision
#[must_use]
pub fn encode(v: &SensorValues, publisher: &str) -> String {
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
        publisher,
    };

    serde_json::to_string(&payload).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_raw(data: &[u8]) -> String {
        let values = SensorValues::from_manufacturer_specific_data(0x0499, data).unwrap();
        encode(&values, "esp32-test")
    }

    // Test vectors from the Ruuvi data format 5 specification.
    #[test]
    fn format_5_valid_values() {
        let data = [
            0x05, 0x12, 0xFC, 0x53, 0x94, 0xC3, 0x7C, 0x00, 0x04, 0xFF, 0xFC, 0x04, 0x0C, 0xAC,
            0x36, 0x42, 0x00, 0xCD, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ];
        assert_eq!(
            encode_raw(&data),
            r#"{"temperature":24.3,"humidity":53.49,"pressure":1000.44,"battery":2.977,"battery_low":false,"tx_power":4,"publisher":"esp32-test"}"#
        );
    }

    #[test]
    fn format_5_invalid_values_are_left_out() {
        let data = [
            0x05, 0x80, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(encode_raw(&data), r#"{"publisher":"esp32-test"}"#);
    }

    #[test]
    fn format_3_has_no_tx_power() {
        // Humidity 50 %, 21.5 °C, 1000 hPa, 2.4 V.
        let data = [0x03, 100, 21, 50, 0xC3, 0x50, 0, 0, 0, 0, 0, 0, 0x09, 0x60];
        assert_eq!(
            encode_raw(&data),
            r#"{"temperature":21.5,"humidity":50.0,"pressure":1000.0,"battery":2.4,"battery_low":true,"publisher":"esp32-test"}"#
        );
    }
}
