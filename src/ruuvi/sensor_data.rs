use ruuvi_sensor_protocol::{
    BatteryPotential, CarbonDioxide, Humidity, Luminosity, NitrogenOxides, ParticulateMatter,
    Pressure, SensorValues, Temperature, TransmitterPower, VolatileOrganicCompounds,
};

use crate::ruuvi::BDAddr;

#[derive(Debug)]
pub struct SensorData {
    pub bdaddr: BDAddr,
    values: SensorValues,
}

impl SensorData {
    pub const fn new(bdaddr: BDAddr, values: SensorValues) -> Self {
        Self { bdaddr, values }
    }

    #[allow(clippy::cast_precision_loss)] // sensor value ranges are well within f32 precision
    pub fn temperature(&self) -> Option<f32> {
        self.values
            .temperature_as_millicelsius()
            .map(|v| v as f32 / 1000.0)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn humidity(&self) -> Option<f32> {
        self.values.humidity_as_ppm().map(|v| v as f32 / 10000.0)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn pressure(&self) -> Option<f32> {
        self.values.pressure_as_pascals().map(|v| v as f32 / 100.0)
    }

    pub fn battery(&self) -> Option<f32> {
        self.values
            .battery_potential_as_millivolts()
            .map(|v| f32::from(v) / 1000.0)
    }

    pub fn battery_low(&self) -> Option<bool> {
        let battery = self.battery()?;
        let temperature = self.temperature()?;

        // Logic copied from https://github.com/ruuvi/com.ruuvi.station/
        Some(
            (temperature <= -20.0 && battery < 2.0)
                || (temperature > -20.0 && temperature < 0.0 && battery < 2.3)
                || (temperature >= 0.0 && battery < 2.5),
        )
    }

    pub fn tx_power(&self) -> Option<i8> {
        self.values.tx_power_as_dbm()
    }

    pub fn co2(&self) -> Option<u16> {
        self.values.carbon_dioxide_as_ppm()
    }

    pub fn pm1(&self) -> Option<f32> {
        self.values
            .pm1_0_as_nanograms_per_cubic_meter()
            .map(micrograms)
    }

    pub fn pm2_5(&self) -> Option<f32> {
        self.values
            .pm2_5_as_nanograms_per_cubic_meter()
            .map(micrograms)
    }

    pub fn pm4(&self) -> Option<f32> {
        self.values
            .pm4_0_as_nanograms_per_cubic_meter()
            .map(micrograms)
    }

    pub fn pm10(&self) -> Option<f32> {
        self.values
            .pm10_0_as_nanograms_per_cubic_meter()
            .map(micrograms)
    }

    pub fn voc(&self) -> Option<u16> {
        self.values.voc_index()
    }

    pub fn nox(&self) -> Option<u16> {
        self.values.nox_index()
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn luminosity(&self) -> Option<f32> {
        self.values
            .luminosity_as_millilux()
            .map(|v| v as f32 / 1000.0)
    }

    pub fn iaqs(&self) -> Option<u8> {
        Some(iaqs(self.pm2_5()?, self.co2()?))
    }
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

#[allow(clippy::cast_precision_loss)]
fn micrograms(nanograms: u32) -> f32 {
    nanograms as f32 / 1000.0
}

#[cfg(test)]
mod tests {
    use ruuvi_sensor_protocol::SensorValues;

    use super::{SensorData, iaqs};
    use crate::ruuvi::BDAddr;

    /// Builds a `SensorData` from optional temperature (millicelsius) and battery (millivolts)
    /// using the `RuuviTag` data format v5 encoding.
    fn make_sensor(temp_mc: Option<i32>, battery_mv: Option<u16>) -> SensorData {
        // Temperature: raw_i16 = mc / 5; i16::MIN signals "invalid" in the v5 spec
        #[allow(clippy::cast_possible_truncation)] // test values are always within i16 range
        let [t1, t2] = match temp_mc {
            Some(mc) => ((mc / 5) as i16).to_be_bytes(),
            None => i16::MIN.to_be_bytes(),
        };
        // Power info: top 11 bits = battery raw (mv - 1600); 0x7FF signals invalid.
        // Bottom 5 bits = tx_power raw; set to 0 (-40 dBm) for simplicity.
        let [pow1, pow2] = match battery_mv {
            Some(mv) => ((mv - 1600) << 5).to_be_bytes(),
            None => (0x7FF_u16 << 5).to_be_bytes(),
        };
        // 24-byte v5 packet: version byte + 23 data bytes.
        // Layout: temp(2), humidity(2), pressure(2), accel xyz(6), power(2), mov(1), seq(2), mac(6)
        #[rustfmt::skip]
        let payload = [
            5, t1, t2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, pow1, pow2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let values = SensorValues::from_manufacturer_specific_data(0x0499, payload).unwrap();
        SensorData::new(BDAddr::from([0u8; 6]), values)
    }

    fn decode(payload: &[u8]) -> SensorData {
        let values = SensorValues::from_manufacturer_specific_data(0x0499, payload).unwrap();
        SensorData::new(BDAddr::from([0u8; 6]), values)
    }

    // Test vectors from the Ruuvi data format E1 and 6 specifications.
    #[test]
    fn format_e1_air_quality() {
        #[rustfmt::skip]
        let s = decode(&[
            0xE1, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x65, 0x00, 0x70, 0x04, 0xBD, 0x11,
            0xCA, 0x00, 0xC9, 0x0A, 0x02, 0x13, 0xE0, 0xAC, 0x00, 0x00, 0x00, 0xDE, 0xCD, 0xEE,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ]);
        assert_eq!(s.co2(), Some(201));
        assert_eq!(s.pm1(), Some(10.1));
        assert_eq!(s.pm2_5(), Some(11.2));
        assert_eq!(s.pm4(), Some(121.3));
        assert_eq!(s.pm10(), Some(455.4));
        assert_eq!(s.voc(), Some(10));
        assert_eq!(s.nox(), Some(2));
        assert_eq!(s.luminosity(), Some(13027.0));
        assert_eq!(s.iaqs(), Some(81));
    }

    #[test]
    fn format_6_has_only_pm2_5() {
        #[rustfmt::skip]
        let s = decode(&[
            0x06, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x70, 0x00, 0xC9, 0x05, 0x01, 0xD9,
            0x00, 0xCD, 0x00, 0x4C, 0x88, 0x4F,
        ]);
        assert_eq!(s.pm1(), None);
        assert_eq!(s.pm2_5(), Some(11.2));
        assert_eq!(s.pm4(), None);
        assert_eq!(s.pm10(), None);
        assert_eq!(s.luminosity(), Some(13026.67));
        assert_eq!(s.iaqs(), Some(81));
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
    fn format_5_has_no_air_quality() {
        let s = make_sensor(Some(20_000), Some(3000));
        assert_eq!(s.co2(), None);
        assert_eq!(s.pm2_5(), None);
        assert_eq!(s.voc(), None);
        assert_eq!(s.nox(), None);
        assert_eq!(s.luminosity(), None);
        assert_eq!(s.iaqs(), None);
    }

    #[test]
    fn battery_low_none_when_battery_missing() {
        let s = make_sensor(Some(20_000), None);
        assert_eq!(s.battery_low(), None);
    }

    #[test]
    fn battery_low_none_when_temperature_missing() {
        let s = make_sensor(None, Some(2400));
        assert_eq!(s.battery_low(), None);
    }

    // --- Zone 1: temp <= -20 °C — threshold is 2.0 V ---

    #[test]
    fn very_cold_battery_below_2v_is_low() {
        let s = make_sensor(Some(-25_000), Some(1999));
        assert_eq!(s.battery_low(), Some(true));
    }

    #[test]
    fn very_cold_battery_at_2v_is_not_low() {
        // boundary: < 2.0 V is low, so 2.0 V exactly is fine
        let s = make_sensor(Some(-25_000), Some(2000));
        assert_eq!(s.battery_low(), Some(false));
    }

    #[test]
    fn minus20_uses_2v_threshold_not_2_3v() {
        // At exactly -20 °C the condition `temperature <= -20.0` applies (2.0 V threshold).
        // 2.2 V is above 2.0 V, so it should NOT be flagged low.
        let s = make_sensor(Some(-20_000), Some(2200));
        assert_eq!(s.battery_low(), Some(false));
    }

    // --- Zone 2: -20 °C < temp < 0 °C — threshold is 2.3 V ---

    #[test]
    fn mid_cold_battery_below_2_3v_is_low() {
        let s = make_sensor(Some(-10_000), Some(2299));
        assert_eq!(s.battery_low(), Some(true));
    }

    #[test]
    fn mid_cold_battery_at_2_3v_is_not_low() {
        let s = make_sensor(Some(-10_000), Some(2300));
        assert_eq!(s.battery_low(), Some(false));
    }

    #[test]
    fn just_above_minus20_uses_stricter_2_3v_threshold() {
        // One step above -20 °C: same 2.2 V that is safe at -20 °C is now low.
        let s = make_sensor(Some(-19_000), Some(2200));
        assert_eq!(s.battery_low(), Some(true));
    }

    // --- Zone 3: temp >= 0 °C — threshold is 2.5 V ---

    #[test]
    fn warm_battery_below_2_5v_is_low() {
        let s = make_sensor(Some(20_000), Some(2499));
        assert_eq!(s.battery_low(), Some(true));
    }

    #[test]
    fn warm_battery_at_2_5v_is_not_low() {
        let s = make_sensor(Some(20_000), Some(2500));
        assert_eq!(s.battery_low(), Some(false));
    }

    #[test]
    fn zero_celsius_uses_2_5v_threshold() {
        // 0 °C hits `temperature >= 0.0`, so the 2.5 V threshold applies.
        let s = make_sensor(Some(0), Some(2499));
        assert_eq!(s.battery_low(), Some(true));
    }
}
