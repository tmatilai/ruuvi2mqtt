use ruuvi_sensor_protocol::{
    BatteryPotential, Humidity, Pressure, SensorValues, Temperature, TransmitterPower,
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
}

#[cfg(test)]
mod tests {
    use ruuvi_sensor_protocol::SensorValues;

    use super::SensorData;
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
