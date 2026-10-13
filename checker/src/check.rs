//! The checks, without MQTT, so that unit tests can run them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::iter;
use std::time::Duration;

use serde_json::Value;

use crate::time;

/// Each tag's Home Assistant entities, as `src/homeassistant.rs` publishes
/// them: (component, entity).
const ENTITIES: [(&str, &str); 6] = [
    ("sensor", "temperature"),
    ("sensor", "humidity"),
    ("sensor", "pressure"),
    ("sensor", "battery"),
    ("binary_sensor", "battery_low"),
    ("sensor", "tx_power"),
];

/// The values in a reading: (key, min, max, required). The ranges are the
/// sensors'. Some tags have no humidity or pressure sensor. Data formats
/// before 5 have no TX power.
const VALUES: [(&str, f64, f64, bool); 5] = [
    ("temperature", -40.0, 85.0, true),
    ("humidity", 0.0, 100.0, false),
    ("pressure", 500.0, 1155.0, false),
    ("battery", 1.6, 3.7, true),
    ("tx_power", -40.0, 20.0, false),
];

/// How many examples of a problem the report lists
const MAX_EXAMPLES: usize = 5;

pub struct Config {
    pub base_topic: String,
    /// The commit under test, at least 7 hex digits
    pub commit: String,
    /// The Linux instances' MQTT client IDs
    pub instances: Vec<String>,
    /// The tags that must be published, as in their topics (lowercase hex)
    pub tags: Vec<String>,
    pub first_reading: Duration,
    pub max_gap: Duration,
    pub throttle: Duration,
    /// Percentage of a tag's readings that may be duplicates
    pub max_duplicates: u8,
}

pub struct Report {
    pub text: String,
    pub failures: Vec<String>,
    /// One line: `ok: ...` or `FAIL: ...`
    pub summary: String,
}

/// Collects what the broker sends while the checker watches.
pub struct Watch {
    config: Config,
    statuses: BTreeMap<String, Vec<Status>>,
    tags: BTreeMap<String, Tag>,
    invalid_readings: Problems,
    invalid_discovery: Problems,
    unknown_publishers: BTreeMap<String, usize>,
    other_topics: usize,
}

struct Status {
    at: Duration,
    /// Retained: the status from before the checker connected
    retained: bool,
    payload: String,
}

#[derive(Default)]
struct Tag {
    /// From the discovery configs
    name: Option<String>,
    entities: BTreeSet<&'static str>,
    readings: Vec<Duration>,
    publishers: BTreeMap<String, usize>,
}

#[derive(Default)]
struct Problems {
    count: usize,
    examples: Vec<String>,
}

impl Problems {
    /// Returns the example if the report lists it.
    fn add(&mut self, example: String) -> Option<String> {
        self.count += 1;
        (self.examples.len() < MAX_EXAMPLES).then(|| {
            self.examples.push(example.clone());
            example
        })
    }
}

impl Watch {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            statuses: BTreeMap::new(),
            tags: BTreeMap::new(),
            invalid_readings: Problems::default(),
            invalid_discovery: Problems::default(),
            unknown_publishers: BTreeMap::new(),
            other_topics: 0,
        }
    }

    /// Records a message. Returns a note to print right away, if any.
    pub fn on_message(
        &mut self,
        at: Duration,
        topic: &str,
        payload: &[u8],
        retained: bool,
    ) -> Option<String> {
        if let Some(rest) = topic
            .strip_prefix(self.config.base_topic.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
        {
            if let Some(client_id) = rest.strip_prefix("status/") {
                return self.on_status(at, client_id, payload, retained);
            }
            if is_tag_id(rest) {
                return self.on_reading(at, rest, payload, retained);
            }
        } else if let Some((id, entity)) = topic
            .strip_prefix("homeassistant/")
            .and_then(parse_discovery_topic)
        {
            return self.on_discovery(topic, id, entity, payload);
        }
        self.other_topics += 1;
        None
    }

    fn on_status(
        &mut self,
        at: Duration,
        client_id: &str,
        payload: &[u8],
        retained: bool,
    ) -> Option<String> {
        let payload = String::from_utf8_lossy(payload).into_owned();
        let history = self.statuses.entry(client_id.to_string()).or_default();
        // The broker sends the retained status again when the checker reconnects
        if retained && history.last().is_some_and(|s| s.payload == payload) {
            return None;
        }
        let note = (self.config.instances.iter().any(|i| i == client_id)
            && !(history.is_empty() && retained))
            .then(|| format!("+{} {client_id}: {payload}", time::format(at)));
        history.push(Status {
            at,
            retained,
            payload,
        });
        note
    }

    fn on_reading(
        &mut self,
        at: Duration,
        id: &str,
        payload: &[u8],
        retained: bool,
    ) -> Option<String> {
        let result = if retained {
            Err("retained".to_string())
        } else {
            validate_reading(payload)
        };
        match result {
            Ok(publisher) => {
                let tag = self.tags.entry(id.to_string()).or_default();
                tag.readings.push(at);
                if !self.config.instances.contains(&publisher) {
                    *self
                        .unknown_publishers
                        .entry(publisher.clone())
                        .or_default() += 1;
                }
                *tag.publishers.entry(publisher).or_default() += 1;
                None
            }
            Err(err) => self
                .invalid_readings
                .add(format!("+{} {id}: {err}", time::format(at))),
        }
    }

    fn on_discovery(
        &mut self,
        topic: &str,
        id: &str,
        entity: &'static str,
        payload: &[u8],
    ) -> Option<String> {
        let tag = self.tags.entry(id.to_string()).or_default();
        // An empty retained message deletes the config
        if payload.is_empty() {
            tag.entities.remove(entity);
            return None;
        }
        let config: Option<Value> = serde_json::from_slice(payload).ok();
        let valid = config.as_ref().is_some_and(|config| {
            config["state_topic"] == format!("{}/{id}", self.config.base_topic)
                && config["unique_id"] == format!("ruuvi_{id}_{entity}")
        });
        if !valid {
            tag.entities.remove(entity);
            return self
                .invalid_discovery
                .add(format!("{topic}: {}", String::from_utf8_lossy(payload)));
        }
        tag.entities.insert(entity);
        if let Some(name) = config
            .as_ref()
            .and_then(|config| config.pointer("/device/name"))
            .and_then(Value::as_str)
        {
            tag.name = Some(name.to_string());
        }
        None
    }

    /// The progress so far, in one line
    pub fn progress(&self) -> String {
        let readings: usize = self.tags.values().map(|t| t.readings.len()).sum();
        let heard = self
            .config
            .tags
            .iter()
            .filter(|id| self.tags.get(*id).is_some_and(|t| !t.readings.is_empty()))
            .count();
        format!(
            "{readings} readings, {heard} of {} tags",
            self.config.tags.len()
        )
    }

    /// Checks what was collected. `end` is how long the checker watched.
    pub fn finish(&self, end: Duration) -> Report {
        let mut r = Report {
            text: String::new(),
            failures: Vec::new(),
            summary: String::new(),
        };
        let short = &self.config.commit[..7];
        let _ = writeln!(r.text, "Commit {short}, watched for {}", time::format(end));
        self.check_instances(&mut r);
        self.check_tags(&mut r, end);
        self.check_messages(&mut r);

        if !r.failures.is_empty() {
            let _ = writeln!(r.text, "\nFailures:");
            for failure in &r.failures {
                let _ = writeln!(r.text, "  - {failure}");
            }
        }
        r.summary = match &r.failures[..] {
            [] => format!(
                "ok: {} tags from {} instances on {short}, {}",
                self.config.tags.len(),
                self.config.instances.len(),
                time::format(end)
            ),
            [failure] => format!("FAIL: {failure}"),
            [failure, rest @ ..] => format!("FAIL: {failure} (and {} more)", rest.len()),
        };
        r
    }

    fn check_instances(&self, r: &mut Report) {
        let c = &self.config;
        let _ = writeln!(r.text, "\nInstances:");
        for id in &c.instances {
            let Some(history) = self.statuses.get(id) else {
                let _ = writeln!(r.text, "  {id}: no status");
                r.failures.push(format!("{id}: no status"));
                continue;
            };
            let first = &history[0];
            let _ = writeln!(r.text, "  {id}: {}", first.payload);
            for status in &history[1..] {
                let _ = writeln!(
                    r.text,
                    "    +{} {}",
                    time::format(status.at),
                    status.payload
                );
            }
            if !first.retained {
                r.failures
                    .push(format!("{id}: came online at +{}", time::format(first.at)));
            } else if let Err(err) = check_status(&first.payload, &c.commit) {
                r.failures.push(format!("{id}: {err}"));
            } else if history.len() > 1 {
                r.failures.push(format!(
                    "{id}: the status changed {} times",
                    history.len() - 1
                ));
            }
        }
        let others: Vec<_> = self
            .statuses
            .iter()
            .filter(|(id, _)| !c.instances.contains(id))
            .collect();
        if !others.is_empty() {
            let _ = writeln!(r.text, "\nOther statuses:");
            for (id, history) in others {
                let last = history.last().map_or("", |s| s.payload.as_str());
                let _ = writeln!(r.text, "  {id}: {last}");
            }
        }
    }

    fn check_tags(&self, r: &mut Report, end: Duration) {
        let c = &self.config;
        let _ = writeln!(r.text, "\nTags:");
        for id in &c.tags {
            let tag = self.tags.get(id);
            let label = self.label(id);
            let readings = tag.map_or(&[][..], |t| &t.readings);
            let _ = write!(r.text, "  {label}: {} readings", readings.len());
            if let (Some(&first), Some(&last)) = (readings.first(), readings.last()) {
                let intervals: Vec<Duration> = readings
                    .windows(2)
                    .map(|w| w[1].saturating_sub(w[0]))
                    .collect();
                let max_gap = intervals
                    .iter()
                    .copied()
                    .chain(iter::once(end.saturating_sub(last)))
                    .max()
                    .unwrap_or_default();
                let duplicates = intervals
                    .iter()
                    .filter(|&&i| i < c.throttle * 9 / 10)
                    .count();
                let _ = write!(
                    r.text,
                    ", first +{}, max gap {}, {duplicates} duplicates",
                    time::format(first),
                    time::format(max_gap)
                );
                if first > c.first_reading {
                    r.failures.push(format!(
                        "{label}: first reading only at +{}",
                        time::format(first)
                    ));
                }
                if max_gap > c.max_gap {
                    r.failures
                        .push(format!("{label}: no reading for {}", time::format(max_gap)));
                }
                if duplicates * 100 > usize::from(c.max_duplicates) * readings.len() {
                    r.failures.push(format!(
                        "{label}: {duplicates} of {} readings within the throttle",
                        readings.len()
                    ));
                }
            } else {
                r.failures.push(format!("{label}: no readings"));
            }
            if let Some(tag) = tag.filter(|t| !t.publishers.is_empty()) {
                let publishers: Vec<String> = tag
                    .publishers
                    .iter()
                    .map(|(p, n)| format!("{p} {n}"))
                    .collect();
                let _ = write!(r.text, ", from {}", publishers.join(", "));
            }
            let entities = tag.map_or(0, |t| t.entities.len());
            let _ = writeln!(r.text, ", discovery {entities}/{}", ENTITIES.len());
            if entities < ENTITIES.len() {
                r.failures.push(format!(
                    "{label}: {entities} of {} discovery configs",
                    ENTITIES.len()
                ));
            }
        }
        let others: Vec<_> = self
            .tags
            .iter()
            .filter(|(id, t)| !c.tags.contains(id) && !t.readings.is_empty())
            .collect();
        if !others.is_empty() {
            let _ = writeln!(r.text, "\nOther tags:");
            for (id, tag) in others {
                let _ = writeln!(
                    r.text,
                    "  {}: {} readings",
                    self.label(id),
                    tag.readings.len()
                );
            }
        }
    }

    /// Invalid and unexpected messages
    fn check_messages(&self, r: &mut Report) {
        if self.invalid_readings.count > 0 {
            r.failures
                .push(format!("{} invalid readings", self.invalid_readings.count));
        }
        if self.invalid_discovery.count > 0 {
            r.failures.push(format!(
                "{} invalid discovery configs",
                self.invalid_discovery.count
            ));
        }
        for (publisher, n) in &self.unknown_publishers {
            r.failures
                .push(format!("{n} readings from {publisher}, not an instance"));
        }
        for (title, problems) in [
            ("Invalid readings", &self.invalid_readings),
            ("Invalid discovery configs", &self.invalid_discovery),
        ] {
            if problems.count > 0 {
                let _ = writeln!(r.text, "\n{title} ({}):", problems.count);
                for example in &problems.examples {
                    let _ = writeln!(r.text, "  {example}");
                }
            }
        }
        if self.other_topics > 0 {
            let _ = writeln!(r.text, "\n{} messages on other topics", self.other_topics);
        }
    }

    fn label(&self, id: &str) -> String {
        match self.tags.get(id).and_then(|t| t.name.as_deref()) {
            Some(name) => format!("{name} ({id})"),
            None => id.to_string(),
        }
    }
}

/// Returns the tag's ID and the entity from
/// `<component>/ruuvi_<id>/<entity>/config`.
fn parse_discovery_topic(rest: &str) -> Option<(&str, &'static str)> {
    let parts: Vec<&str> = rest.split('/').collect();
    let [component, object, entity, "config"] = parts[..] else {
        return None;
    };
    let id = object.strip_prefix("ruuvi_").filter(|id| is_tag_id(id))?;
    let &(_, entity) = ENTITIES
        .iter()
        .find(|(c, e)| *c == component && *e == entity)?;
    Some((id, entity))
}

/// A tag's ID in its topics: the MAC as 12 lowercase hex digits
fn is_tag_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Returns the reading's publisher.
fn validate_reading(payload: &[u8]) -> Result<String, String> {
    let reading: Value =
        serde_json::from_slice(payload).map_err(|err| format!("not JSON: {err}"))?;
    for (key, min, max, required) in VALUES {
        match &reading[key] {
            Value::Number(n) => {
                let value = n.as_f64().unwrap_or(f64::NAN);
                if !(min..=max).contains(&value) {
                    return Err(format!("{key} {value} out of range"));
                }
            }
            Value::Null if !required => {}
            other => return Err(format!("{key}: {other}")),
        }
    }
    if !reading["battery_low"].is_boolean() {
        return Err(format!("battery_low: {}", reading["battery_low"]));
    }
    match &reading["publisher"] {
        Value::String(publisher) if !publisher.is_empty() => Ok(publisher.clone()),
        other => Err(format!("publisher: {other}")),
    }
}

/// The status must be online, with a version built from `commit` without
/// local changes.
fn check_status(payload: &str, commit: &str) -> Result<(), String> {
    let status: Value = serde_json::from_str(payload).map_err(|_| "status isn't JSON")?;
    if status["state"] != "online" {
        return Err(format!("state {}", status["state"]));
    }
    let version = status["version"].as_str().ok_or("no version")?;
    let revision = version
        .rsplit_once(" (")
        .and_then(|(_, r)| r.strip_suffix(')'))
        .ok_or_else(|| format!("no commit in version {version}"))?;
    if revision.ends_with("-dirty") {
        return Err(format!("{version} has local changes"));
    }
    if revision.len() < 7 || !(commit.starts_with(revision) || revision.starts_with(commit)) {
        return Err(format!("runs {version}, not {}", &commit[..7]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "6534f11d0c2e5a7b9f8e1d2c3b4a5968778695a4";
    const PANNU: &str = "ruuvi2mqtt_pannu";
    const TALLI: &str = "ruuvi2mqtt_talli";
    const TAG: &str = "f817624e4c0b";

    fn watch() -> Watch {
        Watch::new(Config {
            base_topic: "ruuvi2mqtt".into(),
            commit: COMMIT.into(),
            instances: vec![PANNU.into(), TALLI.into()],
            tags: vec![TAG.into()],
            first_reading: Duration::from_mins(5),
            max_gap: Duration::from_mins(10),
            throttle: Duration::from_mins(1),
            max_duplicates: 10,
        })
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn online(version: &str) -> String {
        format!(r#"{{"state":"online","version":"{version}"}}"#)
    }

    fn reading(publisher: &str) -> String {
        format!(
            r#"{{"humidity":45.5,"pressure":1003.2,"temperature":21.3,"battery":2.9,"battery_low":false,"tx_power":4,"publisher":"{publisher}"}}"#
        )
    }

    /// Statuses for both instances, discovery for TAG, and a reading every
    /// minute for `minutes`, the first `duplicates` of them twice
    fn healthy_with(watch: &mut Watch, minutes: u64, duplicates: u64) {
        for instance in [PANNU, TALLI] {
            let topic = format!("ruuvi2mqtt/status/{instance}");
            watch.on_message(
                secs(0),
                &topic,
                online("1.5.2-dev (6534f11)").as_bytes(),
                true,
            );
        }
        for (component, entity) in ENTITIES {
            let topic = format!("homeassistant/{component}/ruuvi_{TAG}/{entity}/config");
            let config = format!(
                r#"{{"state_topic":"ruuvi2mqtt/{TAG}","unique_id":"ruuvi_{TAG}_{entity}","device":{{"name":"Cellar"}}}}"#
            );
            watch.on_message(secs(0), &topic, config.as_bytes(), true);
        }
        let topic = format!("ruuvi2mqtt/{TAG}");
        for m in 0..minutes {
            watch.on_message(secs(30 + m * 60), &topic, reading(TALLI).as_bytes(), false);
            if m < duplicates {
                watch.on_message(secs(31 + m * 60), &topic, reading(PANNU).as_bytes(), false);
            }
        }
    }

    fn healthy(watch: &mut Watch, minutes: u64) {
        healthy_with(watch, minutes, 0);
    }

    #[test]
    fn healthy_passes() {
        let mut w = watch();
        healthy(&mut w, 15);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
        assert_eq!(
            report.summary,
            "ok: 1 tags from 2 instances on 6534f11, 15:00"
        );
        assert!(report.text.contains("Cellar (f817624e4c0b): 15 readings"));
    }

    #[test]
    fn wrong_or_dirty_version_fails() {
        for version in ["1.5.2-dev (abcdef0)", "1.5.2-dev (6534f11-dirty)", "1.5.1"] {
            let mut w = watch();
            healthy(&mut w, 15);
            w.on_message(
                secs(0),
                "ruuvi2mqtt/status/ruuvi2mqtt_pannu",
                online(version).as_bytes(),
                true,
            );
            let report = w.finish(Duration::from_mins(15));
            assert_eq!(report.failures.len(), 1, "{version}: {}", report.text);
            assert!(report.summary.starts_with("FAIL: ruuvi2mqtt_pannu: "));
        }
    }

    #[test]
    fn longer_revision_matches() {
        assert_eq!(
            check_status(&online("1.5.2-dev (6534f11d0)"), COMMIT),
            Ok(())
        );
    }

    #[test]
    fn status_change_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        let topic = "ruuvi2mqtt/status/ruuvi2mqtt_talli";
        let note = w.on_message(secs(300), topic, br#"{"state":"offline"}"#, false);
        assert_eq!(
            note.as_deref(),
            Some(r#"+5:00 ruuvi2mqtt_talli: {"state":"offline"}"#)
        );
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: ruuvi2mqtt_talli: the status changed 1 times"
        );
    }

    #[test]
    fn retained_status_again_is_no_change() {
        let mut w = watch();
        healthy(&mut w, 15);
        // After the checker's own reconnect
        let topic = "ruuvi2mqtt/status/ruuvi2mqtt_talli";
        assert_eq!(
            w.on_message(
                secs(300),
                topic,
                online("1.5.2-dev (6534f11)").as_bytes(),
                true
            ),
            None
        );
        assert_eq!(
            w.finish(Duration::from_mins(15)).failures,
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_status_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        w.statuses.remove(PANNU);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.summary, "FAIL: ruuvi2mqtt_pannu: no status");
    }

    #[test]
    fn missing_tag_fails() {
        let mut w = watch();
        healthy(&mut w, 0);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.summary, "FAIL: Cellar (f817624e4c0b): no readings");
    }

    #[test]
    fn late_first_reading_and_gap_fail() {
        let mut w = watch();
        healthy(&mut w, 0);
        w.on_message(
            secs(400),
            &format!("ruuvi2mqtt/{TAG}"),
            reading(PANNU).as_bytes(),
            false,
        );
        let report = w.finish(Duration::from_mins(20));
        assert_eq!(
            report.failures,
            [
                "Cellar (f817624e4c0b): first reading only at +6:40",
                "Cellar (f817624e4c0b): no reading for 13:20",
            ]
        );
    }

    #[test]
    fn duplicates_beyond_the_tolerance_fail() {
        // 10 % of 16 readings tolerates one duplicate
        let mut w = watch();
        healthy_with(&mut w, 15, 1);
        assert_eq!(
            w.finish(Duration::from_mins(15)).failures,
            Vec::<String>::new()
        );
        let mut w = watch();
        healthy_with(&mut w, 15, 2);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: Cellar (f817624e4c0b): 2 of 17 readings within the throttle"
        );
    }

    #[test]
    fn invalid_reading_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        let topic = format!("ruuvi2mqtt/{TAG}");
        let bad = reading(TALLI).replace("45.5", "145.5");
        let note = w.on_message(secs(100), &topic, bad.as_bytes(), false);
        assert_eq!(
            note.as_deref(),
            Some("+1:40 f817624e4c0b: humidity 145.5 out of range")
        );
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.summary, "FAIL: 1 invalid readings");
    }

    #[test]
    fn readings_validate() {
        assert_eq!(
            validate_reading(reading(PANNU).as_bytes()),
            Ok(PANNU.to_string())
        );
        // A tag without the pressure sensor, in an old data format
        let no_pressure = reading(PANNU)
            .replace("1003.2", "null")
            .replace(r#""tx_power":4"#, r#""tx_power":null"#);
        assert!(validate_reading(no_pressure.as_bytes()).is_ok());
        let no_temperature = reading(PANNU).replace("21.3", "null");
        assert_eq!(
            validate_reading(no_temperature.as_bytes()),
            Err("temperature: null".into())
        );
        let no_publisher = reading(PANNU).replace(r#","publisher":"ruuvi2mqtt_pannu""#, "");
        assert_eq!(
            validate_reading(no_publisher.as_bytes()),
            Err("publisher: null".into())
        );
        assert!(validate_reading(b"21.3").is_err());
    }

    #[test]
    fn unknown_publisher_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        w.on_message(
            secs(880),
            &format!("ruuvi2mqtt/{TAG}"),
            reading("esp32").as_bytes(),
            false,
        );
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: 1 readings from esp32, not an instance"
        );
    }

    #[test]
    fn missing_or_deleted_discovery_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        let topic = format!("homeassistant/binary_sensor/ruuvi_{TAG}/battery_low/config");
        w.on_message(secs(0), &topic, b"", true);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: Cellar (f817624e4c0b): 5 of 6 discovery configs"
        );
    }

    #[test]
    fn other_tags_and_topics_are_reported() {
        let mut w = watch();
        healthy(&mut w, 15);
        w.on_message(
            secs(100),
            "ruuvi2mqtt/fc2966f1fd67",
            reading(PANNU).as_bytes(),
            false,
        );
        w.on_message(secs(100), "ruuvi2mqtt/diagnostics/esp32", b"{}", false);
        w.on_message(
            secs(0),
            "ruuvi2mqtt/status/conn-plain",
            br#"{"state":"offline"}"#,
            true,
        );
        let report = w.finish(Duration::from_mins(15));
        assert!(report.failures.is_empty(), "{}", report.text);
        assert!(
            report
                .text
                .contains("\nOther tags:\n  fc2966f1fd67: 1 readings\n")
        );
        assert!(report.text.contains("\nOther statuses:\n  conn-plain: "));
        assert!(report.text.contains("\n1 messages on other topics\n"));
    }
}
