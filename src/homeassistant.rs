use serde::Serialize;
use std::str;

use crate::config;
use crate::ruuvi::{self, BDAddr};

#[derive(Debug, Serialize)]
#[allow(clippy::struct_field_names)] // field names intentionally mirror the HA MQTT topic structure
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
    pub topic: String,
    humidity: Option<f32>,
    pressure: Option<f32>,
    temperature: Option<f32>,
    battery: Option<f32>,
    battery_low: Option<bool>,
    tx_power: Option<i8>,
    // Ruuvi Air values. Left out when missing, so a tag's reading stays as
    // it was.
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
    /// MQTT client ID, so a reading can be traced to the instance that sent it.
    publisher: String,
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
                    topic: format!(
                        "homeassistant/{}/ruuvi_{}/{}/config",
                        device_type.component, id, snake_name
                    ),
                });
            }
        }
        devices
    }
}

impl SensorData {
    pub fn new(data: &ruuvi::SensorData, config: &config::Mqtt) -> Self {
        Self {
            topic: format!("{}/{}", config.base_topic, data.bdaddr.to_string_no_delim()),
            humidity: data.humidity(),
            pressure: data.pressure(),
            temperature: data.temperature(),
            battery: data.battery(),
            battery_low: data.battery_low(),
            tx_power: data.tx_power(),
            co2: data.co2(),
            pm1: data.pm1(),
            pm2_5: data.pm2_5(),
            pm4: data.pm4(),
            pm10: data.pm10(),
            voc: data.voc(),
            nox: data.nox(),
            luminosity: data.luminosity(),
            publisher: config.client_id.clone(),
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

#[cfg(test)]
mod tests {
    use ruuvi_sensor_protocol::SensorValues;
    use tokio::time::Duration;

    use super::*;

    fn reading(payload: &[u8]) -> String {
        let config = config::Mqtt {
            server: String::from("localhost"),
            port: None,
            tls: false,
            tls_insecure: false,
            ca_file: None,
            user: None,
            password: None,
            client_id: String::from("test"),
            base_topic: String::from("ruuvi2mqtt"),
            throttle: Duration::from_mins(1),
        };
        let values = SensorValues::from_manufacturer_specific_data(0x0499, payload).unwrap();
        let data = ruuvi::SensorData::new(BDAddr::from([0u8; 6]), values);
        serde_json::to_string(&SensorData::new(&data, &config)).unwrap()
    }

    // Test vectors from the Ruuvi data format 5 and 6 specifications.
    #[test]
    fn tag_reading_has_no_air_quality_keys() {
        #[rustfmt::skip]
        let json = reading(&[
            0x05, 0x12, 0xFC, 0x53, 0x94, 0xC3, 0x7C, 0x00, 0x04, 0xFF, 0xFC, 0x04, 0x0C, 0xAC,
            0x36, 0x42, 0x00, 0xCD, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ]);
        assert_eq!(
            json,
            r#"{"humidity":53.49,"pressure":1000.44,"temperature":24.3,"battery":2.977,"battery_low":false,"tx_power":4,"publisher":"test"}"#
        );
    }

    #[test]
    fn air_reading_has_air_quality_keys() {
        #[rustfmt::skip]
        let json = reading(&[
            0x06, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x70, 0x00, 0xC9, 0x05, 0x01, 0xD9,
            0x00, 0xCD, 0x00, 0x4C, 0x88, 0x4F,
        ]);
        assert_eq!(
            json,
            r#"{"humidity":55.3,"pressure":1011.02,"temperature":29.5,"battery":null,"battery_low":null,"tx_power":null,"co2":201,"pm2_5":11.2,"voc":5,"nox":1,"luminosity":13026.67,"publisher":"test"}"#
        );
    }
}
