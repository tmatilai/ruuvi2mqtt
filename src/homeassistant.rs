use serde::Serialize;
use std::str;

use crate::config;
use crate::ruuvi::{self, BDAddr};

#[derive(Debug, Serialize)]
pub struct Device<'a> {
    name: String,
    unique_id: String,
    state_class: &'a str,
    state_topic: String,
    json_attributes_topic: String,
    value_template: String,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    payload_info: Option<PayloadInfo>,
    #[serde(flatten)]
    device_type: DeviceType<'a>,
    device: DeviceInfo<'a>,
    #[serde(skip)]
    bdaddr: BDAddr,
    #[serde(skip)]
    pub topic: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceInfo<'a> {
    name: String,
    identifiers: Vec<String>,
    manufacturer: &'a str,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct DeviceType<'a> {
    #[serde(skip)]
    pub component: &'a str,
    #[serde(skip)]
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_class: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit_of_measurement: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_category: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<&'a str>,
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
    battery: Option<f32>,
    battery_low: Option<bool>,
    tx_power: Option<i8>,
}

#[derive(Debug, Serialize)]
pub struct PayloadInfo {
    payload_on: bool,
    payload_off: bool,
}

impl<'a> Device<'a> {
    pub fn all(config: &config::Config) -> Vec<Device<'a>> {
        let mut devices = Vec::new();

        for (bdaddr, device) in &config.devices {
            let id = bdaddr.to_string_no_delim();

            for device_type in DeviceType::all() {
                let state_topic = format!("{}/{}", config.mqtt.base_topic, id);
                let snake_name = device_type.name.to_lowercase().replace(' ', "_");
                devices.push(Self {
                    name: format!("{} {}", device.name, device_type.name),
                    unique_id: format!("ruuvi_{id}_{snake_name}"),
                    state_class: "measurement",
                    state_topic: state_topic.clone(),
                    json_attributes_topic: state_topic,
                    value_template: format!("{{{{ value_json.{snake_name} }}}}"),
                    payload_info: PayloadInfo::from(device_type),
                    device_type: *device_type,
                    device: DeviceInfo::new(device.name.clone(), *bdaddr),
                    bdaddr: *bdaddr,
                    topic: format!(
                        "homeassistant/{}/ruuvi_{}/{}/config",
                        device_type.component, id, snake_name
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
    pub fn new(name: String, data: &ruuvi::SensorData, base_topic: &str) -> Self {
        Self {
            name,
            topic: format!("{}/{}", base_topic, data.bdaddr.to_string_no_delim()),
            humidity: data.humidity(),
            pressure: data.pressure(),
            temperature: data.temperature(),
            battery: data.battery(),
            battery_low: data.battery_low(),
            tx_power: data.tx_power(),
        }
    }
}

impl DeviceInfo<'_> {
    pub fn new(name: String, bdaddr: BDAddr) -> Self {
        Self {
            name,
            identifiers: vec![bdaddr.to_string()],
            manufacturer: "Ruuvi",
        }
    }
}

impl<'a> DeviceType<'a> {
    pub fn all() -> std::slice::Iter<'a, Self> {
        static DEVICE_TYPES: [DeviceType<'static>; 6] = [
            DeviceType {
                component: "sensor",
                name: "Temperature",
                device_class: Some("temperature"),
                unit_of_measurement: Some("°C"),
                entity_category: None,
                icon: None,
            },
            DeviceType {
                component: "sensor",
                name: "Humidity",
                device_class: Some("humidity"),
                unit_of_measurement: Some("%"),
                entity_category: None,
                icon: None,
            },
            DeviceType {
                component: "sensor",
                name: "Pressure",
                device_class: Some("pressure"),
                unit_of_measurement: Some("hPa"),
                entity_category: None,
                icon: None,
            },
            DeviceType {
                component: "sensor",
                name: "Battery",
                device_class: None,
                unit_of_measurement: Some("V"),
                entity_category: Some("diagnostic"),
                icon: Some("mdi:battery"),
            },
            DeviceType {
                component: "binary_sensor",
                name: "Battery Low",
                device_class: Some("battery"),
                unit_of_measurement: None,
                entity_category: Some("diagnostic"),
                icon: None,
            },
            DeviceType {
                component: "sensor",
                name: "TX Power",
                device_class: None,
                unit_of_measurement: Some("dBm"),
                entity_category: Some("diagnostic"),
                icon: Some("mdi:signal"),
            },
        ];
        DEVICE_TYPES.iter()
    }
}

impl PayloadInfo {
    pub fn from(device_type: &DeviceType) -> Option<Self> {
        match device_type.component {
            "binary_sensor" => Some(Self {
                payload_on: true,
                payload_off: false,
            }),
            _ => None,
        }
    }
}
