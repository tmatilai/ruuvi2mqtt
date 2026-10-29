use ruuvi_sensor_protocol::{
    BatteryPotential, Calibration, CarbonDioxide, Humidity, Luminosity, NitrogenOxides,
    ParticulateMatter, Pressure, SensorValues, Temperature, TransmitterPower,
    VolatileOrganicCompounds,
};
use serde::Serialize;

/// JSON-serializable sensor reading.
///
/// Field names match `homeassistant::SensorData` in the ruuvi2mqtt app.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    co2: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pm1: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pm2_5: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pm4: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pm10: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    voc: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nox: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    luminosity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iaqs: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibrating: Option<bool>,
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

    // Temperature-aware battery_low logic, matching the ruuvi2mqtt app
    // (ported from https://github.com/ruuvi/com.ruuvi.station/).
    let battery_low = match (battery, temperature) {
        (Some(volts), Some(temp)) => Some(
            (temp <= -20.0 && volts < 2.0)
                || (temp > -20.0 && temp < 0.0 && volts < 2.3)
                || (temp >= 0.0 && volts < 2.5),
        ),
        _ => None,
    };

    let micrograms = |ng: u32| ng as f32 / 1000.0;
    let pm2_5 = v.pm2_5_as_nanograms_per_cubic_meter().map(micrograms);
    let co2 = v.carbon_dioxide_as_ppm();

    let payload = SensorPayload {
        temperature,
        humidity,
        pressure,
        battery,
        battery_low,
        tx_power: v.tx_power_as_dbm(),
        co2,
        pm1: v.pm1_0_as_nanograms_per_cubic_meter().map(micrograms),
        pm2_5,
        pm4: v.pm4_0_as_nanograms_per_cubic_meter().map(micrograms),
        pm10: v.pm10_0_as_nanograms_per_cubic_meter().map(micrograms),
        voc: v.voc_index(),
        nox: v.nox_index(),
        luminosity: v.luminosity_as_millilux().map(|mlx| mlx as f32 / 1000.0),
        iaqs: pm2_5.zip(co2).map(|(pm2_5, co2)| iaqs(pm2_5, co2)),
        calibrating: v.calibration_in_progress(),
        publisher,
    };

    serde_json::to_string(&payload).unwrap()
}

/// Data format 6, an Air's legacy advertisement
const FORMAT_6: u8 = 6;
/// Data format E1, an Air's extended advertisement
const FORMAT_E1: u8 = 0xE1;

/// Whether a reading in data format `format` replaces one in `previous`
/// from the same device. An Air sends formats 6 and E1. Only E1 has PM1,
/// PM4 and PM10, so Ruuvi says to drop format 6 when both arrive.
#[must_use]
pub fn replaces(format: u8, previous: u8) -> bool {
    !(format == FORMAT_6 && previous == FORMAT_E1)
}

/// Ruuvi's indoor air quality score, 0–100, rounded like Ruuvi's apps.
/// Formula from
/// <https://docs.ruuvi.com/ruuvi-air-firmware/ruuvi-indoor-air-quality-score-iaqs>
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0–100
fn iaqs(pm2_5: f32, co2: u16) -> u8 {
    let pm2_5 = pm2_5.clamp(0.0, 60.0);
    let co2 = f32::from(co2).clamp(420.0, 2300.0);
    let distance = (pm2_5 * 100.0 / 60.0).hypot((co2 - 420.0) * 100.0 / 1880.0);
    (100.0 - distance).clamp(0.0, 100.0).round() as u8
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

    // Test vectors from the Ruuvi data format E1 and 6 specifications.
    #[test]
    fn format_e1_air_quality() {
        let data = [
            0xE1, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x65, 0x00, 0x70, 0x04, 0xBD, 0x11,
            0xCA, 0x00, 0xC9, 0x0A, 0x02, 0x13, 0xE0, 0xAC, 0x00, 0x00, 0x00, 0xDE, 0xCD, 0xEE,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ];
        assert_eq!(
            encode_raw(&data),
            r#"{"temperature":29.5,"humidity":55.3,"pressure":1011.02,"co2":201,"pm1":10.1,"pm2_5":11.2,"pm4":121.3,"pm10":455.4,"voc":10,"nox":2,"luminosity":13027.0,"iaqs":81,"calibrating":true,"publisher":"esp32-test"}"#
        );
    }

    #[test]
    fn format_6_has_only_pm2_5() {
        let data = [
            0x06, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x70, 0x00, 0xC9, 0x05, 0x01, 0xD9,
            0x00, 0xCD, 0x00, 0x4C, 0x88, 0x4F,
        ];
        assert_eq!(
            encode_raw(&data),
            r#"{"temperature":29.5,"humidity":55.3,"pressure":1011.02,"co2":201,"pm2_5":11.2,"voc":5,"nox":1,"luminosity":13026.67,"iaqs":81,"calibrating":false,"publisher":"esp32-test"}"#
        );
    }

    #[test]
    fn format_6_does_not_replace_e1() {
        assert!(!replaces(FORMAT_6, FORMAT_E1));
        assert!(replaces(FORMAT_E1, FORMAT_6));
        assert!(replaces(FORMAT_6, FORMAT_6));
        assert!(replaces(FORMAT_E1, FORMAT_E1));
        assert!(replaces(5, 5));
    }

    // Computed from the formula; Ruuvi publishes no test vectors.
    #[test]
    fn iaqs_values() {
        assert_eq!(iaqs(0.0, 420), 100);
        assert_eq!(iaqs(60.0, 420), 0);
        assert_eq!(iaqs(0.0, 2300), 0);
        assert_eq!(iaqs(6.0, 600), 86);
        assert_eq!(iaqs(11.2, 201), 81);
        assert_eq!(iaqs(11.4, 886), 69);
        assert_eq!(iaqs(100.0, 5000), 0);
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
