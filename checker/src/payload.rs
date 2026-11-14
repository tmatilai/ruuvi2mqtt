//! The app's payloads: readings, discovery configs, statuses and
//! diagnostics. They mirror `src/homeassistant.rs`, `src/mqtt.rs` and
//! `src/diagnostics.rs`; update them when those change.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;

use crate::diagnostics::number;
use crate::topic::is_tag_id;

/// A tag's Home Assistant entities, as `src/homeassistant.rs` publishes
/// them: (component, entity).
pub const TAG_ENTITIES: [(&str, &str); 6] = [
    ("sensor", "temperature"),
    ("sensor", "humidity"),
    ("sensor", "pressure"),
    ("sensor", "battery"),
    ("binary_sensor", "battery_low"),
    ("sensor", "tx_power"),
];

/// A Ruuvi Air's entities, the same way
pub const AIR_ENTITIES: [(&str, &str); 13] = [
    ("sensor", "temperature"),
    ("sensor", "humidity"),
    ("sensor", "pressure"),
    ("sensor", "co2"),
    ("sensor", "pm1"),
    ("sensor", "pm2_5"),
    ("sensor", "pm4"),
    ("sensor", "pm10"),
    ("sensor", "voc"),
    ("sensor", "nox"),
    ("sensor", "luminosity"),
    ("sensor", "iaqs"),
    ("binary_sensor", "calibrating"),
];

#[derive(Clone, Copy, PartialEq)]
enum Presence {
    Required,
    Optional,
    Absent,
}

use Presence::{Absent, Optional, Required};

/// The values in a reading: (key, min, max, in a tag's, in an Air's). The
/// ranges are the sensors'. Some tags have no humidity or pressure sensor.
/// Data formats before 5 have no TX power. An Air has no battery and no TX
/// power, and sends PM1, PM4 and PM10 only in data format E1. Luminosity's
/// range is data format E1's. The air quality score needs PM2.5 and CO₂, so
/// it is optional too.
const VALUES: [(&str, f64, f64, Presence, Presence); 14] = [
    ("temperature", -40.0, 85.0, Required, Required),
    ("humidity", 0.0, 100.0, Optional, Optional),
    ("pressure", 500.0, 1155.0, Optional, Optional),
    ("battery", 1.6, 3.7, Required, Absent),
    ("tx_power", -40.0, 20.0, Optional, Absent),
    ("co2", 0.0, 40_000.0, Absent, Optional),
    ("pm1", 0.0, 1000.0, Absent, Optional),
    ("pm2_5", 0.0, 1000.0, Absent, Optional),
    ("pm4", 0.0, 1000.0, Absent, Optional),
    ("pm10", 0.0, 1000.0, Absent, Optional),
    ("voc", 1.0, 500.0, Absent, Optional),
    ("nox", 1.0, 500.0, Absent, Optional),
    ("luminosity", 0.0, 167_772.14, Absent, Optional),
    ("iaqs", 0.0, 100.0, Absent, Optional),
];

#[derive(Clone, Copy, PartialEq)]
pub enum Model {
    Tag,
    Air,
}

impl Model {
    pub fn entities(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Tag => &TAG_ENTITIES,
            Self::Air => &AIR_ENTITIES,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Reading {
    pub publisher: String,
    /// It has PM1, PM4 and PM10, which only data format E1 sends
    pub e1: bool,
}

pub fn validate_reading(payload: &[u8], model: Model) -> Result<Reading, String> {
    let reading: Value =
        serde_json::from_slice(payload).map_err(|err| format!("not JSON: {err}"))?;
    for (key, min, max, tag, air) in VALUES {
        let presence = match model {
            Model::Tag => tag,
            Model::Air => air,
        };
        match &reading[key] {
            Value::Number(n) if presence != Absent => {
                let value = n.as_f64().unwrap_or(f64::NAN);
                if !(min..=max).contains(&value) {
                    return Err(format!("{key} {value} out of range"));
                }
            }
            Value::Null if presence != Required => {}
            other => return Err(format!("{key}: {other}")),
        }
    }
    let battery_low = &reading["battery_low"];
    if battery_low.is_boolean() != (model == Model::Tag) {
        return Err(format!("battery_low: {battery_low}"));
    }
    let calibrating = &reading["calibrating"];
    if calibrating.is_boolean() != (model == Model::Air) {
        return Err(format!("calibrating: {calibrating}"));
    }
    let publisher = client_id(&reading["publisher"])
        .ok_or_else(|| format!("publisher: {}", reading["publisher"]))?;
    Ok(Reading {
        publisher: publisher.to_string(),
        e1: ["pm1", "pm4", "pm10"]
            .iter()
            .all(|key| reading[key].is_number()),
    })
}

/// A minute of an instance's diagnostics
pub struct Diagnostics {
    /// When the checker got them
    pub at: Duration,
    pub client_id: String,
    pub adapter: String,
    /// Since the instance started
    pub mqtt_reconnects: u32,
    pub readings_published: u32,
    pub readings_throttled: u32,
    /// Tag ID to (the strongest RSSI, if known, and readings)
    pub tags: BTreeMap<String, (Option<i8>, u32)>,
}

/// An instance's diagnostics, or `None` for a board's, which have `firmware`
/// instead of `version`.
pub fn parse_diagnostics(at: Duration, payload: &[u8]) -> Option<Result<Diagnostics, String>> {
    let d: Value = serde_json::from_slice(payload).ok()?;
    d.get("version")?;
    let parse = || {
        let number = |key: &str| number(&d, key);
        Ok(Diagnostics {
            at,
            client_id: client_id(&d["client_id"])
                .ok_or_else(|| format!("client_id: {}", d["client_id"]))?
                .to_string(),
            adapter: d["adapter"]
                .as_str()
                .map(printable)
                .ok_or_else(|| format!("adapter: {}", d["adapter"]))?,
            mqtt_reconnects: number("mqtt_reconnects")?,
            readings_published: number("readings_published")?,
            readings_throttled: number("readings_throttled")?,
            tags: parse_tags(&d["tags"])?,
        })
    };
    Some(parse())
}

fn parse_tags(tags: &Value) -> Result<BTreeMap<String, (Option<i8>, u32)>, String> {
    let Value::Object(tags) = tags else {
        return Err(format!("tags: {tags}"));
    };
    tags.iter()
        .map(|(id, tag)| {
            let rssi = match &tag["rssi"] {
                Value::Null => Some(None),
                rssi => rssi.as_i64().and_then(|r| i8::try_from(r).ok()).map(Some),
            };
            match (rssi, number(tag, "readings")) {
                (Some(rssi), Ok(readings)) if is_tag_id(id) => Ok((id.clone(), (rssi, readings))),
                _ => Err(format!("tag {}: {tag}", printable(id))),
            }
        })
        .collect()
}

/// An MQTT client ID, such as a reading's publisher. The report shows it
/// unescaped, so one with control characters is invalid.
pub fn client_id(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|id| !id.is_empty() && !id.contains(char::is_control))
}

pub fn parse_discovery(payload: &[u8], base_topic: &str, id: &str, entity: &str) -> Option<Value> {
    let config: Value = serde_json::from_slice(payload).ok()?;
    (config["state_topic"] == format!("{base_topic}/{id}")
        && config["unique_id"] == format!("ruuvi_{id}_{entity}"))
    .then_some(config)
}

/// The client ID of an instance's discovery config, if it is one
pub fn instance_discovery(payload: &[u8], base_topic: &str) -> Option<String> {
    let config: Value = serde_json::from_slice(payload).ok()?;
    let id = config["state_topic"]
        .as_str()?
        .strip_prefix(&format!("{base_topic}/status/"))?;
    client_id(&config["device"]["identifiers"][0])
        .filter(|i| *i == id)
        .map(str::to_string)
}

pub fn device_name(config: &Value) -> Option<String> {
    config.pointer("/device/name")?.as_str().map(printable)
}

/// The tag's name from its discovery config. Unlike `parse_discovery`, it
/// accepts a config on any base topic, because the boards don't publish the
/// tags' configs.
pub fn tag_name(payload: &[u8], id: &str, entity: &str) -> Option<String> {
    let config: Value = serde_json::from_slice(payload).ok()?;
    let state_topic = config["state_topic"].as_str()?;
    (state_topic.ends_with(&format!("/{id}"))
        && config["unique_id"] == format!("ruuvi_{id}_{entity}"))
    .then(|| device_name(&config))?
}

pub fn check_status(payload: &str, commit: &str) -> Result<(), String> {
    let status: Value = serde_json::from_str(payload).map_err(|_| "status isn't JSON")?;
    if status["state"] != "online" {
        return Err(format!("state {}", status["state"]));
    }
    check_version(status["version"].as_str().ok_or("no version")?, commit)
}

/// The version must be built from `commit` without local changes.
pub fn check_version(version: &str, commit: &str) -> Result<(), String> {
    let revision = version
        .rsplit_once(" (")
        .and_then(|(_, r)| r.strip_suffix(')'));
    let version = printable(version);
    let revision = revision.ok_or_else(|| format!("no commit in version {version}"))?;
    if revision.ends_with("-dirty") {
        return Err(format!("{version} has local changes"));
    }
    if revision.len() < 7 || !(commit.starts_with(revision) || revision.starts_with(commit)) {
        return Err(format!("runs {version}, not {}", &commit[..7]));
    }
    Ok(())
}

/// Escapes the control characters in text from the broker. Otherwise anyone
/// who can publish could add lines to the report, such as a false summary, or
/// send escape sequences to the terminal.
pub fn printable(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() {
            let _ = write!(out, "{}", c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{
        COMMIT, HOST1, TAG, air_reading, air_reading_e1, discovery, online, reading,
    };

    #[test]
    fn tag_names_come_from_any_base_topic() {
        let config = discovery(TAG, "temperature", "Cellar");
        assert_eq!(
            tag_name(config.as_bytes(), TAG, "temperature"),
            Some("Cellar".into())
        );
        let other = config.replace("ruuvi2mqtt/", "ruuvi2mqtt-dev/");
        assert_eq!(
            tag_name(other.as_bytes(), TAG, "temperature"),
            Some("Cellar".into())
        );
        // Another tag's, or another entity's
        assert_eq!(
            tag_name(config.as_bytes(), "aabbccddee02", "temperature"),
            None
        );
        assert_eq!(tag_name(config.as_bytes(), TAG, "humidity"), None);
    }

    #[test]
    fn readings_validate() {
        assert_eq!(
            validate_reading(reading(HOST1).as_bytes(), Model::Tag),
            Ok(Reading {
                publisher: HOST1.into(),
                e1: false
            })
        );
        // A tag without the pressure sensor, in an old data format
        let no_pressure = reading(HOST1)
            .replace("1003.2", "null")
            .replace(r#""tx_power":4"#, r#""tx_power":null"#);
        assert!(validate_reading(no_pressure.as_bytes(), Model::Tag).is_ok());
        let no_temperature = reading(HOST1).replace("21.3", "null");
        assert_eq!(
            validate_reading(no_temperature.as_bytes(), Model::Tag),
            Err("temperature: null".into())
        );
        let no_publisher = reading(HOST1).replace(r#","publisher":"ruuvi2mqtt_host1""#, "");
        assert_eq!(
            validate_reading(no_publisher.as_bytes(), Model::Tag),
            Err("publisher: null".into())
        );
        assert!(validate_reading(b"21.3", Model::Tag).is_err());
        assert_eq!(
            validate_reading(air_reading(HOST1).as_bytes(), Model::Tag),
            Err("battery: null".into())
        );
    }

    #[test]
    fn publisher_with_control_characters_is_invalid() {
        assert_eq!(
            validate_reading(reading(r"x\nok: all good").as_bytes(), Model::Tag),
            Err(r#"publisher: "x\nok: all good""#.into())
        );
    }

    #[test]
    fn air_readings_validate() {
        assert_eq!(
            validate_reading(air_reading(HOST1).as_bytes(), Model::Air),
            Ok(Reading {
                publisher: HOST1.into(),
                e1: false
            })
        );
        assert_eq!(
            validate_reading(air_reading_e1(HOST1).as_bytes(), Model::Air),
            Ok(Reading {
                publisher: HOST1.into(),
                e1: true
            })
        );
        assert_eq!(
            validate_reading(reading(HOST1).as_bytes(), Model::Air),
            Err("battery: 2.9".into())
        );
        let battery_low =
            air_reading(HOST1).replace(r#""battery_low":null"#, r#""battery_low":false"#);
        assert_eq!(
            validate_reading(battery_low.as_bytes(), Model::Air),
            Err("battery_low: false".into())
        );
        let calibrating = air_reading(HOST1).replace(r#""calibrating":false,"#, "");
        assert_eq!(
            validate_reading(calibrating.as_bytes(), Model::Air),
            Err("calibrating: null".into())
        );
    }

    #[test]
    fn longer_revision_matches() {
        assert_eq!(
            check_status(&online("1.5.2-dev (6534f11d0)"), COMMIT),
            Ok(())
        );
    }

    #[test]
    fn escapes_control_characters() {
        assert_eq!(printable("Cellar ä"), "Cellar ä");
        assert_eq!(printable("a\nb\r\u{1b}[2J"), r"a\nb\r\u{1b}[2J");
    }
}
