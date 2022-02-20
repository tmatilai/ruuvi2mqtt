use serde::Serialize;

use crate::config;
use crate::ruuvi::{self, BDAddr};

#[derive(Debug, Serialize)]
pub struct Device<'a> {
    name: String,
    unique_id: String,
    state_topic: String,
    value_template: String,
    #[serde(flatten)]
    device_type: DeviceType<'a>,
    #[serde(skip)]
    bdaddr: BDAddr,
    #[serde(skip)]
    pub topic: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct DeviceType<'a> {
    pub device_class: &'a str,
    pub unit_of_measurement: &'a str,
}

#[derive(Debug, Serialize)]
pub struct SensorData {
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub topic: String,
    humidity: Option<f32>,
    pressure: Option<f32>,
    temperature: Option<f32>,
}

impl<'a> Device<'a> {
    pub fn all(config: &config::Config) -> Vec<Device<'a>> {
        let mut devices = Vec::new();

        for (bdaddr, device) in &config.devices {
            for device_type in DeviceType::all() {
                let device_class = device_type.device_class;
                devices.push(Self {
                    name: format!("{} {}", device.name, device_class),
                    unique_id: format!("{}_{}", bdaddr, device_class),
                    state_topic: format!(
                        "{}/{}",
                        config.mqtt.base_topic,
                        bdaddr.to_string_no_delim()
                    ),
                    value_template: format!("{{{{ value_json.{} }}}}", device_class),
                    device_type: *device_type,
                    bdaddr: *bdaddr,
                    topic: format!(
                        "homeassistant/sensor/ruuvi_{}/{}/config",
                        bdaddr.to_string_no_delim(),
                        device_class
                    ),
                });
            }
        }
        devices
    }

    pub fn topic(&self) -> String {
        self.topic.clone()
    }
}

impl SensorData {
    pub fn new(name: String, data: ruuvi::SensorData, base_topic: &str) -> Self {
        Self {
            name,
            topic: format!("{}/{}", base_topic, data.bdaddr.to_string_no_delim()),
            humidity: data.humidity(),
            pressure: data.pressure(),
            temperature: data.temperature(),
        }
    }
}

impl<'a> DeviceType<'a> {
    pub fn all() -> std::slice::Iter<'a, Self> {
        static DEVICE_TYPES: [DeviceType<'static>; 3] = [
            DeviceType {
                device_class: "temperature",
                unit_of_measurement: "°C",
            },
            DeviceType {
                device_class: "humidity",
                unit_of_measurement: "%",
            },
            DeviceType {
                device_class: "pressure",
                unit_of_measurement: "hPa",
            },
        ];
        DEVICE_TYPES.iter()
    }
}
