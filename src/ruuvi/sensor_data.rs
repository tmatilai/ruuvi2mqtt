use ruuvi_sensor_protocol::{Humidity, Pressure, SensorValues, Temperature};

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
}
