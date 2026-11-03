//! The board as a Home Assistant device, like the app's instances.

use serde_json::{json, Value};

pub struct Board<'a> {
    pub client_id: &'a str,
    pub hostname: &'a str,
    pub base_topic: &'a str,
    pub version: &'a str,
    pub chip: &'a str,
    pub mode: Mode,
}

#[derive(Clone, Copy)]
pub enum Mode {
    Continuous,
    /// Without a status, the sensors expire instead.
    DeepSleep {
        expire_after: u32,
    },
}

/// The discovery configs: topic and payload. The other mode's are empty,
/// which deletes them after a mode change.
#[must_use]
pub fn configs(board: &Board) -> Vec<(String, String)> {
    let id = topic_id(board.client_id);
    let status_topic = format!("{}/status/{}", board.base_topic, board.client_id);
    let mut device = json!({
        "name": board.client_id,
        "identifiers": [board.client_id],
        "model": "ruuvi2mqtt-esp32",
        "sw_version": board.version,
        "hw_version": board.chip,
    });

    let mut configs: Vec<(&str, &str, Value)> = sensors(board.mode)
        .into_iter()
        .map(|(field, mut config)| {
            config["value_template"] = format!("{{{{ value_json.{field} }}}}").into();
            ("sensor", field, config)
        })
        .collect();
    configs.push((
        "sensor",
        "scan_mode",
        json!({
            "name": "Scan mode",
            "entity_category": "diagnostic",
            "value_template": "{{ value_json.settings.scan_mode }}",
        }),
    ));
    for (_, _, config) in &mut configs {
        config["state_topic"] =
            format!("{}/diagnostics/{}", board.base_topic, board.hostname).into();
        match board.mode {
            Mode::Continuous => {
                config["availability_topic"] = status_topic.clone().into();
                config["availability_template"] = "{{ value_json.state }}".into();
            }
            Mode::DeepSleep { expire_after } => config["expire_after"] = expire_after.into(),
        }
    }
    if let Mode::Continuous = board.mode {
        device["configuration_url"] = format!("http://{}.local/", board.hostname).into();
        configs.push((
            "binary_sensor",
            "status",
            json!({
                "name": "Status",
                "device_class": "connectivity",
                "state_topic": status_topic,
                "value_template": "{{ value_json.state }}",
                "payload_on": "online",
                "payload_off": "offline",
            }),
        ));
    }

    let topic = |component, key| format!("homeassistant/{component}/{id}/{key}/config");
    let other_mode: &[(&str, &str)] = match board.mode {
        Mode::Continuous => &[("sensor", "cycle")],
        Mode::DeepSleep { .. } => &[("sensor", "uptime_s"), ("binary_sensor", "status")],
    };
    configs
        .into_iter()
        .map(|(component, key, mut config)| {
            config["unique_id"] = format!("{id}_{key}").into();
            config["device"] = device.clone();
            (topic(component, key), config.to_string())
        })
        .chain(
            other_mode
                .iter()
                .map(|&(component, key)| (topic(component, key), String::new())),
        )
        .collect()
}

/// The sensors that read a diagnostics field of the same name.
fn sensors(mode: Mode) -> Vec<(&'static str, Value)> {
    let mut sensors = vec![
        (
            "wifi_rssi",
            json!({
                "name": "Wi-Fi signal",
                "device_class": "signal_strength",
                "unit_of_measurement": "dBm",
                "state_class": "measurement",
            }),
        ),
        ("wifi_bssid", json!({ "name": "Access point" })),
        (
            "free_heap",
            json!({
                "name": "Free heap",
                "device_class": "data_size",
                "unit_of_measurement": "B",
                "state_class": "measurement",
            }),
        ),
        (
            "cycles_failed",
            json!({ "name": "Failed cycles", "state_class": "total_increasing" }),
        ),
    ];
    match mode {
        Mode::Continuous => sensors.push((
            "uptime_s",
            json!({
                "name": "Uptime",
                "device_class": "duration",
                "unit_of_measurement": "s",
            }),
        )),
        // Deep sleep restarts the uptime every cycle.
        Mode::DeepSleep { .. } => sensors.push(("cycle", json!({ "name": "Cycle" }))),
    }
    sensors
}

/// A discovery topic's IDs allow only ASCII letters, digits, `_` and `-`.
fn topic_id(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(mode: Mode) -> Board<'static> {
        Board {
            client_id: "kitchen.esp32",
            hostname: "ruuvi2mqtt-kitchen",
            base_topic: "ruuvi2mqtt",
            version: "1.7.1-dev (83e48fb)",
            chip: "esp32c6",
            mode,
        }
    }

    fn config<'a>(configs: &'a [(String, String)], topic: &str) -> &'a str {
        &configs.iter().find(|(t, _)| t == topic).unwrap().1
    }

    const DEVICE: &str = r#""hw_version":"esp32c6","identifiers":["kitchen.esp32"],"model":"ruuvi2mqtt-esp32","name":"kitchen.esp32","sw_version":"1.7.1-dev (83e48fb)"}"#;

    #[test]
    fn continuous() {
        let configs = configs(&board(Mode::Continuous));
        let topics: Vec<&str> = configs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            topics,
            [
                "homeassistant/sensor/kitchen_esp32/wifi_rssi/config",
                "homeassistant/sensor/kitchen_esp32/wifi_bssid/config",
                "homeassistant/sensor/kitchen_esp32/free_heap/config",
                "homeassistant/sensor/kitchen_esp32/cycles_failed/config",
                "homeassistant/sensor/kitchen_esp32/uptime_s/config",
                "homeassistant/sensor/kitchen_esp32/scan_mode/config",
                "homeassistant/binary_sensor/kitchen_esp32/status/config",
                "homeassistant/sensor/kitchen_esp32/cycle/config",
            ]
        );
        assert_eq!(config(&configs, topics[7]), "");
        assert_eq!(
            config(&configs, topics[0]),
            [
                r#"{"availability_template":"{{ value_json.state }}","#,
                r#""availability_topic":"ruuvi2mqtt/status/kitchen.esp32","#,
                r#""device":{"configuration_url":"http://ruuvi2mqtt-kitchen.local/","#,
                DEVICE,
                ",",
                r#""device_class":"signal_strength","name":"Wi-Fi signal","#,
                r#""state_class":"measurement","#,
                r#""state_topic":"ruuvi2mqtt/diagnostics/ruuvi2mqtt-kitchen","#,
                r#""unique_id":"kitchen_esp32_wifi_rssi","#,
                r#""unit_of_measurement":"dBm","value_template":"{{ value_json.wifi_rssi }}"}"#,
            ]
            .concat()
        );
        assert_eq!(
            config(&configs, topics[6]),
            [
                r#"{"device":{"configuration_url":"http://ruuvi2mqtt-kitchen.local/","#,
                DEVICE,
                ",",
                r#""device_class":"connectivity","name":"Status","#,
                r#""payload_off":"offline","payload_on":"online","#,
                r#""state_topic":"ruuvi2mqtt/status/kitchen.esp32","#,
                r#""unique_id":"kitchen_esp32_status","value_template":"{{ value_json.state }}"}"#,
            ]
            .concat()
        );
    }

    #[test]
    fn deep_sleep() {
        let configs = configs(&board(Mode::DeepSleep { expire_after: 195 }));
        let topics: Vec<&str> = configs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            topics,
            [
                "homeassistant/sensor/kitchen_esp32/wifi_rssi/config",
                "homeassistant/sensor/kitchen_esp32/wifi_bssid/config",
                "homeassistant/sensor/kitchen_esp32/free_heap/config",
                "homeassistant/sensor/kitchen_esp32/cycles_failed/config",
                "homeassistant/sensor/kitchen_esp32/cycle/config",
                "homeassistant/sensor/kitchen_esp32/scan_mode/config",
                "homeassistant/sensor/kitchen_esp32/uptime_s/config",
                "homeassistant/binary_sensor/kitchen_esp32/status/config",
            ]
        );
        assert_eq!(config(&configs, topics[6]), "");
        assert_eq!(config(&configs, topics[7]), "");
        assert_eq!(
            config(&configs, topics[5]),
            [
                r#"{"device":{"#,
                DEVICE,
                r#","entity_category":"diagnostic","expire_after":195,"name":"Scan mode","#,
                r#""state_topic":"ruuvi2mqtt/diagnostics/ruuvi2mqtt-kitchen","#,
                r#""unique_id":"kitchen_esp32_scan_mode","#,
                r#""value_template":"{{ value_json.settings.scan_mode }}"}"#,
            ]
            .concat()
        );
    }
}
