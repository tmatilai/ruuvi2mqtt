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
    /// The reading's key, also used in the topic and `unique_id`
    #[serde(skip)]
    pub key: &'a str,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    iaqs: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibrating: Option<bool>,
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

            for device_type in DeviceType::for_model(device.model) {
                let state_topic = format!("{}/{}", config.mqtt.base_topic, id);
                let key = device_type.key;
                devices.push(Self {
                    name: format!("{} {}", device.name, device_type.name),
                    unique_id: format!("ruuvi_{id}_{key}"),
                    state_class: "measurement",
                    state_topic: state_topic.clone(),
                    json_attributes_topic: state_topic,
                    value_template: format!("{{{{ value_json.{key} }}}}"),
                    payload_info: PayloadInfo::from(device_type),
                    device_type: *device_type,
                    device: DeviceInfo::new(device.name.clone(), *bdaddr),
                    topic: format!(
                        "homeassistant/{}/ruuvi_{}/{}/config",
                        device_type.component, id, key
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
            iaqs: data.iaqs(),
            calibrating: data.calibrating(),
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

impl DeviceType<'static> {
    pub fn for_model(model: config::Model) -> &'static [Self] {
        match model {
            config::Model::Tag => &TAG_TYPES,
            config::Model::Air => &AIR_TYPES,
        }
    }

    const fn sensor(name: &'static str, key: &'static str) -> Self {
        Self {
            component: "sensor",
            name,
            key,
            device_class: None,
            unit_of_measurement: None,
            entity_category: None,
            icon: None,
        }
    }

    const fn binary_sensor(name: &'static str, key: &'static str) -> Self {
        Self {
            component: "binary_sensor",
            ..Self::sensor(name, key)
        }
    }

    const fn class(self, device_class: &'static str) -> Self {
        Self {
            device_class: Some(device_class),
            ..self
        }
    }

    const fn unit(self, unit: &'static str) -> Self {
        Self {
            unit_of_measurement: Some(unit),
            ..self
        }
    }

    const fn diagnostic(self) -> Self {
        Self {
            entity_category: Some("diagnostic"),
            ..self
        }
    }

    const fn icon(self, icon: &'static str) -> Self {
        Self {
            icon: Some(icon),
            ..self
        }
    }
}

const TEMPERATURE: DeviceType = DeviceType::sensor("Temperature", "temperature")
    .class("temperature")
    .unit("°C");
const HUMIDITY: DeviceType = DeviceType::sensor("Humidity", "humidity")
    .class("humidity")
    .unit("%");
const PRESSURE: DeviceType = DeviceType::sensor("Pressure", "pressure")
    .class("pressure")
    .unit("hPa");

static TAG_TYPES: [DeviceType; 6] = [
    TEMPERATURE,
    HUMIDITY,
    PRESSURE,
    DeviceType::sensor("Battery", "battery")
        .unit("V")
        .diagnostic()
        .icon("mdi:battery"),
    DeviceType::binary_sensor("Battery Low", "battery_low")
        .class("battery")
        .diagnostic(),
    DeviceType::sensor("TX Power", "tx_power")
        .unit("dBm")
        .diagnostic()
        .icon("mdi:signal"),
];

// An Air has no battery and no TX power.
static AIR_TYPES: [DeviceType; 13] = [
    TEMPERATURE,
    HUMIDITY,
    PRESSURE,
    DeviceType::sensor("CO2", "co2")
        .class("carbon_dioxide")
        .unit("ppm"),
    DeviceType::sensor("PM1", "pm1").class("pm1").unit("µg/m³"),
    DeviceType::sensor("PM2.5", "pm2_5")
        .class("pm25")
        .unit("µg/m³"),
    DeviceType::sensor("PM4", "pm4").class("pm4").unit("µg/m³"),
    DeviceType::sensor("PM10", "pm10")
        .class("pm10")
        .unit("µg/m³"),
    // Sensirion's VOC and NOx indexes have no unit, so no device class fits.
    DeviceType::sensor("VOC", "voc").icon("mdi:air-filter"),
    DeviceType::sensor("NOx", "nox").icon("mdi:molecule"),
    DeviceType::sensor("Illuminance", "luminosity")
        .class("illuminance")
        .unit("lx"),
    // Not the aqi device class: a higher AQI is worse, a higher score better.
    DeviceType::sensor("Indoor Air Quality Score", "iaqs").icon("mdi:gauge"),
    // The air quality values may be off while the sensors calibrate.
    DeviceType::binary_sensor("Calibrating", "calibrating")
        .diagnostic()
        .icon("mdi:progress-wrench"),
];

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

    fn mqtt() -> config::Mqtt {
        config::Mqtt {
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
        }
    }

    fn reading(payload: &[u8]) -> String {
        let config = mqtt();
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
            r#"{"humidity":55.3,"pressure":1011.02,"temperature":29.5,"battery":null,"battery_low":null,"tx_power":null,"co2":201,"pm2_5":11.2,"voc":5,"nox":1,"luminosity":13026.67,"iaqs":81,"calibrating":false,"publisher":"test"}"#
        );
    }

    /// The discovery topics and payloads for one device
    fn discovery(model: config::Model) -> Vec<(String, String)> {
        let device = config::Device {
            name: String::from("Office"),
            model,
        };
        let config = config::Config {
            mqtt: mqtt(),
            devices: [(BDAddr::from([0xaa, 0x12, 0xbb, 0x34, 0xcc, 0x56]), device)].into(),
        };
        Device::all(&config)
            .into_iter()
            .map(|d| (d.topic.clone(), serde_json::to_string(&d).unwrap()))
            .collect()
    }

    fn topics(discovery: &[(String, String)]) -> Vec<&str> {
        discovery
            .iter()
            .map(|(topic, _)| {
                topic
                    .strip_prefix("homeassistant/")
                    .and_then(|t| t.strip_suffix("/config"))
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn tag_discovery() {
        let discovery = discovery(config::Model::Tag);
        assert_eq!(
            topics(&discovery),
            [
                "sensor/ruuvi_aa12bb34cc56/temperature",
                "sensor/ruuvi_aa12bb34cc56/humidity",
                "sensor/ruuvi_aa12bb34cc56/pressure",
                "sensor/ruuvi_aa12bb34cc56/battery",
                "binary_sensor/ruuvi_aa12bb34cc56/battery_low",
                "sensor/ruuvi_aa12bb34cc56/tx_power",
            ]
        );
        assert_eq!(
            discovery[4].1,
            r#"{"name":"Office Battery Low","unique_id":"ruuvi_aa12bb34cc56_battery_low","state_class":"measurement","state_topic":"ruuvi2mqtt/aa12bb34cc56","json_attributes_topic":"ruuvi2mqtt/aa12bb34cc56","value_template":"{{ value_json.battery_low }}","payload_on":true,"payload_off":false,"device_class":"battery","entity_category":"diagnostic","device":{"name":"Office","identifiers":["AA:12:BB:34:CC:56"],"manufacturer":"Ruuvi"}}"#
        );
    }

    #[test]
    fn air_discovery() {
        let discovery = discovery(config::Model::Air);
        assert_eq!(
            topics(&discovery),
            [
                "sensor/ruuvi_aa12bb34cc56/temperature",
                "sensor/ruuvi_aa12bb34cc56/humidity",
                "sensor/ruuvi_aa12bb34cc56/pressure",
                "sensor/ruuvi_aa12bb34cc56/co2",
                "sensor/ruuvi_aa12bb34cc56/pm1",
                "sensor/ruuvi_aa12bb34cc56/pm2_5",
                "sensor/ruuvi_aa12bb34cc56/pm4",
                "sensor/ruuvi_aa12bb34cc56/pm10",
                "sensor/ruuvi_aa12bb34cc56/voc",
                "sensor/ruuvi_aa12bb34cc56/nox",
                "sensor/ruuvi_aa12bb34cc56/luminosity",
                "sensor/ruuvi_aa12bb34cc56/iaqs",
                "binary_sensor/ruuvi_aa12bb34cc56/calibrating",
            ]
        );
        assert_eq!(
            discovery[5].1,
            r#"{"name":"Office PM2.5","unique_id":"ruuvi_aa12bb34cc56_pm2_5","state_class":"measurement","state_topic":"ruuvi2mqtt/aa12bb34cc56","json_attributes_topic":"ruuvi2mqtt/aa12bb34cc56","value_template":"{{ value_json.pm2_5 }}","device_class":"pm25","unit_of_measurement":"µg/m³","device":{"name":"Office","identifiers":["AA:12:BB:34:CC:56"],"manufacturer":"Ruuvi"}}"#
        );
    }
}
