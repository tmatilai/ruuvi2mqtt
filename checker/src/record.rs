//! The record mode's state, without MQTT. A run reads it to know which tags
//! to expect and how often they come.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value, json};

use crate::payload::{client_id, device_name, parse_discovery};
use crate::time::max_gap;
use crate::topic::Topic;

/// How far back the state's readings and gaps go, in seconds
pub const WINDOW: u64 = 3600;
/// A longer break, with the recorder stopped or disconnected, restarts the
/// recording. Otherwise the break would look like a gap of every tag.
const MAX_BREAK: u64 = 120;
/// A device not heard for this long drops out of the state
const DEVICE_TTL: u64 = 24 * 3600;

/// All times are Unix seconds.
pub struct Recorder {
    base_topic: String,
    /// Since when it has recorded without a break
    since: u64,
    /// By tag ID
    devices: BTreeMap<String, Device>,
}

#[derive(Default)]
struct Device {
    /// From the discovery configs
    name: Option<String>,
    /// When, and from which publisher, within the window. The last one stays
    /// even when older.
    readings: VecDeque<(u64, String)>,
    battery: Option<f64>,
    battery_low: Option<bool>,
}

impl Recorder {
    pub fn new(base_topic: &str, now: u64) -> Self {
        Self {
            base_topic: base_topic.to_string(),
            since: now,
            devices: BTreeMap::new(),
        }
    }

    /// Continues from the state it wrote before a restart. Call
    /// [`Self::resume`] with the state's `written` once connected.
    pub fn load(base_topic: &str, state: &str) -> Result<(Self, u64), String> {
        let state: Value = serde_json::from_str(state).map_err(|err| format!("not JSON: {err}"))?;
        let number = |key: &str| state[key].as_u64().ok_or(format!("{key}: {}", state[key]));
        let mut recorder = Self::new(base_topic, number("since")?);
        let devices = state["devices"]
            .as_object()
            .ok_or(format!("devices: {}", state["devices"]))?;
        for (id, d) in devices {
            let readings = d["readings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| Some((r[0].as_u64()?, r[1].as_str()?.to_string())))
                .collect();
            recorder.devices.insert(
                id.clone(),
                Device {
                    name: d["name"].as_str().map(str::to_string),
                    readings,
                    battery: d["battery"].as_f64(),
                    battery_low: d["battery_low"].as_bool(),
                },
            );
        }
        Ok((recorder, number("written")?))
    }

    /// After a break from `from` to `now`, with the recorder stopped or
    /// disconnected. A long one restarts the recording.
    pub fn resume(&mut self, from: u64, now: u64) {
        if now.saturating_sub(from) > MAX_BREAK {
            self.since = now;
        }
    }

    pub fn on_message(&mut self, now: u64, topic: &str, payload: &[u8], retained: bool) {
        match Topic::parse(&self.base_topic, topic) {
            Topic::Reading(id) if !retained => self.on_reading(now, id, payload),
            Topic::Discovery { id, entity } => {
                let name = parse_discovery(payload, &self.base_topic, id, entity)
                    .as_ref()
                    .and_then(device_name);
                if let Some(name) = name {
                    self.devices.entry(id.to_string()).or_default().name = Some(name);
                }
            }
            _ => {}
        }
    }

    /// The check validates the readings. The recorder needs only the publisher
    /// and the battery.
    fn on_reading(&mut self, now: u64, id: &str, payload: &[u8]) {
        let Ok(reading) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        let Some(publisher) = client_id(&reading["publisher"]) else {
            return;
        };
        let device = self.devices.entry(id.to_string()).or_default();
        device.readings.push_back((now, publisher.to_string()));
        device.battery = reading["battery"].as_f64();
        device.battery_low = reading["battery_low"].as_bool();
    }

    /// Drops the readings and devices that are too old. A device with a name
    /// stays without readings: the discovery configs are retained, so they
    /// come only once, often before a rare tag's first reading.
    fn prune(&mut self, now: u64) {
        let start = now.saturating_sub(WINDOW);
        self.devices.retain(|_, d| {
            while d.readings.len() > 1 && d.readings[0].0 < start {
                d.readings.pop_front();
            }
            if d.readings
                .back()
                .is_some_and(|(at, _)| *at + DEVICE_TTL < now)
            {
                d.readings.clear();
                d.battery = None;
                d.battery_low = None;
            }
            !d.readings.is_empty() || d.name.is_some()
        });
    }

    /// The state to write, as JSON. README.md describes its fields.
    pub fn state(&mut self, now: u64) -> Value {
        self.prune(now);
        let start = now.saturating_sub(WINDOW).max(self.since);
        let devices: Map<String, Value> = self
            .devices
            .iter()
            .filter(|(_, d)| !d.readings.is_empty())
            .map(|(id, d)| {
                let times: Vec<u64> = d
                    .readings
                    .iter()
                    .map(|(at, _)| *at)
                    .filter(|at| *at >= start)
                    .collect();
                let mut publishers: BTreeMap<&str, usize> = BTreeMap::new();
                for (_, publisher) in d.readings.iter().filter(|(at, _)| *at >= start) {
                    *publishers.entry(publisher).or_default() += 1;
                }
                let device = json!({
                    "name": d.name,
                    "last": d.readings.back().map(|(at, _)| at),
                    "max_gap": max_gap(&times, start, now),
                    "publishers": publishers,
                    "battery": d.battery,
                    "battery_low": d.battery_low,
                    "readings": d.readings.iter().map(|(at, p)| json!([at, p])).collect::<Vec<_>>(),
                });
                (id.clone(), device)
            })
            .collect();
        json!({
            "written": now,
            "since": self.since,
            "window": WINDOW,
            "devices": devices,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{HOST1, TAG, discovery, reading};

    const T0: u64 = 1_800_000_000;

    fn recorder_with(times: &[u64]) -> Recorder {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        let topic = format!("ruuvi2mqtt/{TAG}");
        for &t in times {
            r.on_message(T0 + t, &topic, reading(HOST1).as_bytes(), false);
        }
        r
    }

    #[test]
    fn records_last_gap_publishers_and_battery() {
        let mut r = recorder_with(&[30, 90, 400, 460]);
        r.on_message(
            T0,
            &format!("homeassistant/sensor/ruuvi_{TAG}/temperature/config"),
            discovery(TAG, "temperature", "Cellar").as_bytes(),
            true,
        );
        let state = r.state(T0 + 600);
        let d = &state["devices"][TAG];
        assert_eq!(d["name"], "Cellar");
        assert_eq!(d["last"], T0 + 460);
        // From 90 to 400
        assert_eq!(d["max_gap"], 310);
        assert_eq!(d["publishers"][HOST1], 4);
        assert_eq!(d["battery"], 2.9);
        assert_eq!(d["battery_low"], false);
        assert_eq!(state["since"], T0);
        assert_eq!(state["written"], T0 + 600);
    }

    #[test]
    fn keeps_the_name_until_the_first_reading() {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        r.on_message(
            T0,
            &format!("homeassistant/sensor/ruuvi_{TAG}/temperature/config"),
            discovery(TAG, "temperature", "Cellar").as_bytes(),
            true,
        );
        assert_eq!(r.state(T0 + 60)["devices"], json!({}));
        r.on_message(
            T0 + 300,
            &format!("ruuvi2mqtt/{TAG}"),
            reading(HOST1).as_bytes(),
            false,
        );
        assert_eq!(r.state(T0 + 360)["devices"][TAG]["name"], "Cellar");
        // Not heard for a day, and back
        assert_eq!(r.state(T0 + 301 + DEVICE_TTL)["devices"], json!({}));
        let now = T0 + 400 + DEVICE_TTL;
        r.on_message(
            now,
            &format!("ruuvi2mqtt/{TAG}"),
            reading(HOST1).as_bytes(),
            false,
        );
        assert_eq!(r.state(now)["devices"][TAG]["name"], "Cellar");
    }

    #[test]
    fn window_drops_old_readings_but_keeps_the_last() {
        let mut r = recorder_with(&[0, 60, 120]);
        let state = r.state(T0 + 120 + WINDOW + 300);
        let d = &state["devices"][TAG];
        assert_eq!(d["last"], T0 + 120);
        assert_eq!(d["readings"].as_array().map(Vec::len), Some(1));
        assert_eq!(d["publishers"], json!({}));
        assert_eq!(d["max_gap"], WINDOW);
        // Not heard for a day
        let state = r.state(T0 + 120 + DEVICE_TTL + 1);
        assert_eq!(state["devices"], json!({}));
    }

    #[test]
    fn ignores_retained_and_invalid_readings() {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        let topic = format!("ruuvi2mqtt/{TAG}");
        r.on_message(T0, &topic, reading(HOST1).as_bytes(), true);
        r.on_message(T0, &topic, b"21.3", false);
        r.on_message(T0, &topic, br#"{"temperature":21.3}"#, false);
        r.on_message(
            T0,
            "ruuvi2mqtt/status/host1",
            br#"{"state":"online"}"#,
            true,
        );
        assert_eq!(r.state(T0 + 60)["devices"], json!({}));
    }

    #[test]
    fn a_long_break_restarts_the_recording() {
        let mut r = recorder_with(&[30, 90]);
        r.resume(T0 + 100, T0 + 100 + MAX_BREAK);
        assert_eq!(r.state(T0 + 300)["since"], T0);
        r.resume(T0 + 300, T0 + 301 + MAX_BREAK);
        let now = T0 + 301 + MAX_BREAK;
        let state = r.state(now);
        assert_eq!(state["since"], now);
        // The readings before the break don't count for the gap
        assert_eq!(state["devices"][TAG]["max_gap"], 0);
    }

    #[test]
    fn loads_its_state() {
        let mut r = recorder_with(&[30, 90, 400]);
        let state = r.state(T0 + 420).to_string();
        let (mut loaded, written) = Recorder::load("ruuvi2mqtt", &state).unwrap();
        assert_eq!(written, T0 + 420);
        loaded.resume(written, T0 + 450);
        assert_eq!(
            loaded.state(T0 + 450)["devices"],
            r.state(T0 + 450)["devices"]
        );
        assert!(Recorder::load("ruuvi2mqtt", "{}").is_err());
    }
}
