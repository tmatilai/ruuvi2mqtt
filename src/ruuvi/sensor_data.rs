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

    pub fn temperature(&self) -> Option<f32> {
        self.values
            .temperature_as_millicelsius()
            .map(|v| v as f32 / 1000.0)
    }

    pub fn humidity(&self) -> Option<f32> {
        self.values.humidity_as_ppm().map(|v| v as f32 / 10000.0)
    }

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
