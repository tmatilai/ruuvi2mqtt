//! The checks, without MQTT, so that unit tests can run them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::iter;
use std::time::Duration;

use serde_json::Value;

use crate::time;

/// A tag's Home Assistant entities, as `src/homeassistant.rs` publishes
/// them: (component, entity).
const TAG_ENTITIES: [(&str, &str); 6] = [
    ("sensor", "temperature"),
    ("sensor", "humidity"),
    ("sensor", "pressure"),
    ("sensor", "battery"),
    ("binary_sensor", "battery_low"),
    ("sensor", "tx_power"),
];

/// A Ruuvi Air's entities, the same way
const AIR_ENTITIES: [(&str, &str); 13] = [
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
/// range is data format E1's. The air quality score needs PM2.5 and CO₂.
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
enum Model {
    Tag,
    Air,
}

impl Model {
    fn entities(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Tag => &TAG_ENTITIES,
            Self::Air => &AIR_ENTITIES,
        }
    }
}

/// How many examples of a problem the report lists
const MAX_EXAMPLES: usize = 5;

pub struct Config {
    pub base_topic: String,
    /// The commit under test, at least 7 hex digits
    pub commit: String,
    /// The Linux instances' MQTT client IDs
    pub instances: Vec<String>,
    /// The ESP32 boards' MQTT client IDs, to check their firmware and cycle
    pub boards: Vec<String>,
    /// How often a board publishes its diagnostics
    pub board_cycle: Duration,
    /// The tags that must be published, as in their topics (lowercase hex),
    /// the Airs included
    pub tags: Vec<String>,
    /// Which of the tags are Ruuvi Airs. Any other device is validated as a
    /// tag.
    pub airs: Vec<String>,
    pub first_reading: Duration,
    pub max_gap: Duration,
    pub throttle: Duration,
    /// Percentage of a tag's readings that may be duplicates
    pub max_duplicates: u8,
}

impl Config {
    fn model(&self, id: &str) -> Model {
        if self.airs.iter().any(|a| a == id) {
            Model::Air
        } else {
            Model::Tag
        }
    }
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
    /// The ESP32 boards' diagnostics, by client ID
    boards: BTreeMap<String, Vec<Diagnostics>>,
    invalid_readings: Problems,
    invalid_discovery: Problems,
    invalid_diagnostics: Problems,
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
    /// When, and from which publisher
    readings: Vec<(Duration, String)>,
    publishers: BTreeMap<String, usize>,
}

/// One cycle's diagnostics from an ESP32 board. The times are in ms from the
/// start of the board's cycle.
struct Diagnostics {
    at: Duration,
    hostname: String,
    firmware: String,
    chip: String,
    /// The board's cycle counter, which restarts on a cold boot or crash
    cycle: u64,
    settings: String,
    wifi_cache: String,
    scan_start_ms: u64,
    scan_end_ms: u64,
    wifi_ms: u64,
    mqtt_ms: u64,
    cycle_ms: u64,
    /// Tag ID to (the strongest RSSI, advertisements)
    tags: BTreeMap<String, (i64, u64)>,
    /// Ruuvi advertisements before and after Wi-Fi connected
    ruuvi_adverts: [u64; 2],
    /// Other advertisements, the same way
    other_adverts: [u64; 2],
    error: Option<String>,
}

impl Diagnostics {
    /// The scan time in ms before and after Wi-Fi connected
    fn scan_windows(&self) -> [u64; 2] {
        let split = self.wifi_ms.clamp(self.scan_start_ms, self.scan_end_ms);
        [split - self.scan_start_ms, self.scan_end_ms - split]
    }
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
            boards: BTreeMap::new(),
            invalid_readings: Problems::default(),
            invalid_discovery: Problems::default(),
            invalid_diagnostics: Problems::default(),
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
            if let Some(hostname) = rest.strip_prefix("diagnostics/") {
                return self.on_diagnostics(at, hostname, payload, retained);
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
            validate_reading(payload, self.config.model(id))
        };
        match result {
            Ok(publisher) => {
                let tag = self.tags.entry(id.to_string()).or_default();
                *tag.publishers.entry(publisher.clone()).or_default() += 1;
                tag.readings.push((at, publisher));
                None
            }
            Err(err) => self
                .invalid_readings
                .add(format!("+{} {id}: {err}", time::format(at))),
        }
    }

    fn on_diagnostics(
        &mut self,
        at: Duration,
        hostname: &str,
        payload: &[u8],
        retained: bool,
    ) -> Option<String> {
        let result = if retained {
            Err("retained".to_string())
        } else {
            parse_diagnostics(at, hostname, payload)
        };
        match result {
            Ok((client_id, cycle)) => {
                self.boards.entry(client_id).or_default().push(cycle);
                None
            }
            Err(err) => self
                .invalid_diagnostics
                .add(format!("+{} {hostname}: {err}", time::format(at))),
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
        let mut progress = format!(
            "{readings} readings, {heard} of {} tags",
            self.config.tags.len()
        );
        let cycles: usize = self.boards.values().map(Vec::len).sum();
        if cycles > 0 {
            let _ = write!(progress, ", {cycles} ESP32 cycles");
        }
        progress
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
        self.check_boards(&mut r, end);
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
        let boards = self.board_ids();
        let _ = writeln!(r.text, "\nTags:");
        for id in &c.tags {
            let tag = self.tags.get(id);
            let label = self.label(id);
            let readings: Vec<Duration> =
                tag.map_or_else(Vec::new, |t| t.readings.iter().map(|(at, _)| *at).collect());
            let _ = write!(r.text, "  {label}: {} readings", readings.len());
            if let Some(&first) = readings.first() {
                let max_gap = max_gap(&readings, end);
                // The boards don't coordinate with the instances or each other
                let throttled: Vec<Duration> = tag
                    .iter()
                    .flat_map(|t| &t.readings)
                    .filter(|(_, publisher)| !boards.contains(publisher.as_str()))
                    .map(|(at, _)| *at)
                    .collect();
                let duplicates = throttled
                    .windows(2)
                    .filter(|w| w[1].saturating_sub(w[0]) < c.throttle * 9 / 10)
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
                if duplicates * 100 > usize::from(c.max_duplicates) * throttled.len() {
                    r.failures.push(format!(
                        "{label}: {duplicates} of {} readings within the throttle",
                        throttled.len()
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
            let expected = c.model(id).entities();
            let (entities, unexpected): (Vec<&str>, Vec<&str>) = tag
                .iter()
                .flat_map(|t| &t.entities)
                .partition(|e| expected.iter().any(|(_, x)| x == *e));
            let _ = writeln!(r.text, ", discovery {}/{}", entities.len(), expected.len());
            if entities.len() < expected.len() {
                r.failures.push(format!(
                    "{label}: {} of {} discovery configs",
                    entities.len(),
                    expected.len()
                ));
            }
            // Retained configs from another model stay in Home Assistant
            if !unexpected.is_empty() {
                r.failures.push(format!(
                    "{label}: unexpected discovery configs: {}",
                    unexpected.join(", ")
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
        if self.invalid_diagnostics.count > 0 {
            r.failures.push(format!(
                "{} invalid diagnostics",
                self.invalid_diagnostics.count
            ));
        }
        let boards = self.board_ids();
        let mut unknown_publishers: BTreeMap<&str, usize> = BTreeMap::new();
        for (publisher, n) in self.tags.values().flat_map(|t| &t.publishers) {
            if !self.config.instances.contains(publisher) && !boards.contains(publisher.as_str()) {
                *unknown_publishers.entry(publisher).or_default() += n;
            }
        }
        for (publisher, n) in unknown_publishers {
            r.failures.push(format!(
                "{n} readings from {publisher}, not an instance or a board"
            ));
        }
        for (title, problems) in [
            ("Invalid readings", &self.invalid_readings),
            ("Invalid discovery configs", &self.invalid_discovery),
            ("Invalid diagnostics", &self.invalid_diagnostics),
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

    /// The boards given with `--board`, and the ones that sent diagnostics
    fn board_ids(&self) -> BTreeSet<&str> {
        self.config
            .boards
            .iter()
            .chain(self.boards.keys())
            .map(String::as_str)
            .collect()
    }

    /// The ESP32 section. The reception figures are only reported: what good
    /// values are is still unknown. With `--board`, the firmware and the
    /// cycle are checked.
    fn check_boards(&self, r: &mut Report, end: Duration) {
        let c = &self.config;
        let ids = self.board_ids();
        if ids.is_empty() {
            return;
        }
        let _ = writeln!(r.text, "\nESP32 boards:");
        for id in ids {
            let checked = c.boards.iter().any(|b| b == id);
            let cycles = self.boards.get(id).map_or(&[][..], Vec::as_slice);
            let Some(last) = cycles.last() else {
                let _ = writeln!(r.text, "  {id}: no diagnostics");
                if checked {
                    r.failures.push(format!("{id}: no diagnostics"));
                }
                continue;
            };
            let _ = writeln!(
                r.text,
                "  {id} ({}, {}): {} cycles, {}",
                last.hostname,
                last.chip,
                cycles.len(),
                last.firmware
            );
            self.report_board(r, cycles);
            if checked {
                check_board(r, id, cycles, c, end);
            }
        }
    }

    fn report_board(&self, r: &mut Report, cycles: &[Diagnostics]) {
        let mut settings: BTreeMap<&str, usize> = BTreeMap::new();
        for cycle in cycles {
            *settings.entry(&cycle.settings).or_default() += 1;
        }
        let settings: Vec<String> = match settings.len() {
            1 => settings.into_keys().map(str::to_string).collect(),
            _ => settings
                .iter()
                .map(|(s, k)| format!("{s} ({k} cycles)"))
                .collect(),
        };
        let _ = writeln!(r.text, "    settings: {}", settings.join("; "));
        self.report_board_tags(r, cycles);

        let windows = sum_pairs(cycles.iter().map(Diagnostics::scan_windows));
        let ruuvi = sum_pairs(cycles.iter().map(|cy| cy.ruuvi_adverts));
        let other = sum_pairs(cycles.iter().map(|cy| cy.other_adverts));
        let _ = writeln!(
            r.text,
            "    adverts/s before Wi-Fi connected: Ruuvi {}, other {}; after: Ruuvi {}, other {}",
            per_second(ruuvi[0], windows[0]),
            per_second(other[0], windows[0]),
            per_second(ruuvi[1], windows[1]),
            per_second(other[1], windows[1]),
        );

        let ms = |f: fn(&Diagnostics) -> u64| -> i64 {
            mean(
                &cycles
                    .iter()
                    .map(|cy| i64::try_from(f(cy)).unwrap_or(i64::MAX))
                    .collect::<Vec<_>>(),
            )
        };
        let max_cycle_ms = cycles
            .iter()
            .map(|cy| cy.cycle_ms)
            .max()
            .unwrap_or_default();
        let _ = writeln!(
            r.text,
            "    mean ms: Wi-Fi {}, MQTT {}, scan {}..{}, cycle {} (max {max_cycle_ms})",
            ms(|cy| cy.wifi_ms),
            ms(|cy| cy.mqtt_ms),
            ms(|cy| cy.scan_start_ms),
            ms(|cy| cy.scan_end_ms),
            ms(|cy| cy.cycle_ms),
        );

        let mut caches: BTreeMap<&str, usize> = BTreeMap::new();
        for cycle in cycles {
            *caches.entry(&cycle.wifi_cache).or_default() += 1;
        }
        let caches: Vec<String> = caches.iter().map(|(k, n)| format!("{n} {k}")).collect();
        let (missed, restarts) = missed_cycles(cycles);
        let _ = writeln!(
            r.text,
            "    Wi-Fi cache {}; cycle counter: {missed} missed, {restarts} restarts",
            caches.join(", ")
        );
        let errors: Vec<&Diagnostics> = cycles.iter().filter(|cy| cy.error.is_some()).collect();
        if !errors.is_empty() {
            let _ = writeln!(r.text, "    {} cycles with errors:", errors.len());
            for cycle in errors.iter().take(MAX_EXAMPLES) {
                let _ = writeln!(
                    r.text,
                    "      +{} {}",
                    time::format(cycle.at),
                    cycle.error.as_deref().unwrap_or_default()
                );
            }
        }
    }

    /// The hit rate, and each expected tag's reception
    fn report_board_tags(&self, r: &mut Report, cycles: &[Diagnostics]) {
        let c = &self.config;
        let n = cycles.len();
        let heard: usize = cycles
            .iter()
            .map(|cycle| {
                c.tags
                    .iter()
                    .filter(|t| cycle.tags.contains_key(*t))
                    .count()
            })
            .sum();
        let expected = c.tags.len() * n;
        let _ = writeln!(
            r.text,
            "    hit rate {}% ({heard} of {expected} tag cycles)",
            heard * 100 / expected.max(1)
        );
        for id in &c.tags {
            let seen: Vec<(i64, u64)> = cycles
                .iter()
                .filter_map(|cy| cy.tags.get(id))
                .copied()
                .collect();
            let _ = write!(
                r.text,
                "    {}: {} of {n} cycles",
                self.label(id),
                seen.len()
            );
            if !seen.is_empty() {
                let rssi: Vec<i64> = seen.iter().map(|(rssi, _)| *rssi).collect();
                let adverts: u64 = seen.iter().map(|(_, a)| a).sum();
                let _ = write!(
                    r.text,
                    ", RSSI {} ({}..{}), {:.1} adverts per cycle heard",
                    mean(&rssi),
                    rssi.iter().min().unwrap_or(&0),
                    rssi.iter().max().unwrap_or(&0),
                    ratio(adverts, seen.len() as u64)
                );
            }
            let _ = writeln!(r.text);
        }
        let others: BTreeSet<&String> = cycles
            .iter()
            .flat_map(|cy| cy.tags.keys())
            .filter(|t| !c.tags.contains(t))
            .collect();
        if !others.is_empty() {
            let _ = writeln!(r.text, "    {} other tags", others.len());
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
    let &(_, entity) = TAG_ENTITIES
        .iter()
        .chain(&AIR_ENTITIES)
        .find(|(c, e)| *c == component && *e == entity)?;
    Some((id, entity))
}

/// A tag's ID in its topics: the MAC as 12 lowercase hex digits
fn is_tag_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The longest time without an event, the end included
fn max_gap(times: &[Duration], end: Duration) -> Duration {
    let Some(&last) = times.last() else {
        return end;
    };
    times
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .chain(iter::once(end.saturating_sub(last)))
        .max()
        .unwrap_or_default()
}

/// With `--board`: the board runs the commit under test, publishes about
/// once per `board_cycle`, and never goes `max_gap` without diagnostics.
fn check_board(r: &mut Report, id: &str, cycles: &[Diagnostics], c: &Config, end: Duration) {
    if let Some(err) = cycles
        .iter()
        .find_map(|cy| check_version(&cy.firmware, &c.commit).err())
    {
        r.failures.push(format!("{id}: {err}"));
    }
    let times: Vec<Duration> = cycles.iter().map(|cy| cy.at).collect();
    let gap = max_gap(&times, end).max(times[0]);
    if gap > c.max_gap {
        r.failures
            .push(format!("{id}: no diagnostics for {}", time::format(gap)));
    }
    if let [first, .., last] = times[..] {
        let interval =
            last.saturating_sub(first) / u32::try_from(times.len() - 1).unwrap_or(u32::MAX);
        let (low, high) = (c.board_cycle * 9 / 10, c.board_cycle * 11 / 10);
        if !(low..=high).contains(&interval) {
            r.failures.push(format!(
                "{id}: a cycle every {}s, not {}s",
                interval.as_secs(),
                c.board_cycle.as_secs()
            ));
        }
    }
}

/// Cycles missing from the counter, and how often it restarted
fn missed_cycles(cycles: &[Diagnostics]) -> (u64, usize) {
    let mut missed = 0;
    let mut restarts = 0;
    for w in cycles.windows(2) {
        if w[1].cycle > w[0].cycle {
            missed += w[1].cycle - w[0].cycle - 1;
        } else {
            restarts += 1;
            missed += w[1].cycle.saturating_sub(1);
        }
    }
    (missed, restarts)
}

fn sum_pairs(pairs: impl Iterator<Item = [u64; 2]>) -> [u64; 2] {
    pairs.fold([0, 0], |acc, p| [acc[0] + p[0], acc[1] + p[1]])
}

#[allow(clippy::cast_precision_loss)] // counts are far below 2^52
fn ratio(n: u64, d: u64) -> f64 {
    n as f64 / d.max(1) as f64
}

/// Events per second over `ms`, or `-` without time
fn per_second(n: u64, ms: u64) -> String {
    if ms == 0 {
        "-".to_string()
    } else {
        format!("{:.1}", ratio(n * 1000, ms))
    }
}

/// The rounded mean, 0 for none
fn mean(values: &[i64]) -> i64 {
    let n = i64::try_from(values.len()).unwrap_or(i64::MAX).max(1);
    let sum: i64 = values.iter().sum();
    (sum + sum.signum() * n / 2) / n
}

/// Returns the board's client ID and its diagnostics.
fn parse_diagnostics(
    at: Duration,
    hostname: &str,
    payload: &[u8],
) -> Result<(String, Diagnostics), String> {
    let d: Value = serde_json::from_slice(payload).map_err(|err| format!("not JSON: {err}"))?;
    let string = |key: &str| match &d[key] {
        Value::String(s) if !s.is_empty() => Ok(s.clone()),
        other => Err(format!("{key}: {other}")),
    };
    let number = |key: &str| number(&d, key);
    let pair = |key: &str| {
        Ok::<_, String>([
            number(&format!("{key}_before_wifi"))?,
            number(&format!("{key}_after_wifi"))?,
        ])
    };
    let client_id = string("client_id")?;
    let diagnostics = Diagnostics {
        at,
        hostname: hostname.to_string(),
        firmware: string("firmware")?,
        chip: string("chip")?,
        cycle: number("cycle")?,
        settings: parse_settings(&d["settings"])?,
        wifi_cache: string("wifi_cache")?,
        scan_start_ms: number("scan_start_ms")?,
        scan_end_ms: number("scan_end_ms")?,
        wifi_ms: number("wifi_ms")?,
        mqtt_ms: number("mqtt_ms")?,
        cycle_ms: number("cycle_ms")?,
        tags: parse_board_tags(&d["tags"])?,
        ruuvi_adverts: pair("adverts")?,
        other_adverts: pair("other_adverts")?,
        error: d["error"].as_str().map(str::to_string),
    };
    if diagnostics.scan_start_ms > diagnostics.scan_end_ms {
        return Err(format!(
            "scan_start_ms {} after scan_end_ms {}",
            diagnostics.scan_start_ms, diagnostics.scan_end_ms
        ));
    }
    Ok((client_id, diagnostics))
}

fn number(v: &Value, key: &str) -> Result<u64, String> {
    v[key].as_u64().ok_or_else(|| format!("{key}: {}", v[key]))
}

/// `parallel 5s`, with `, external antenna` if set
fn parse_settings(s: &Value) -> Result<String, String> {
    let mode = s["scan_mode"]
        .as_str()
        .ok_or_else(|| format!("settings: {s}"))?;
    let mut settings = format!("{mode} {}s", number(s, "scan_duration")?);
    if let Some(antenna) = s["antenna"].as_str() {
        let _ = write!(settings, ", {antenna} antenna");
    }
    Ok(settings)
}

fn parse_board_tags(tags: &Value) -> Result<BTreeMap<String, (i64, u64)>, String> {
    let Value::Object(tags) = tags else {
        return Err(format!("tags: {tags}"));
    };
    tags.iter()
        .map(
            |(id, tag)| match (tag["rssi"].as_i64(), tag["adverts"].as_u64()) {
                (Some(rssi), Some(adverts)) if is_tag_id(id) => Ok((id.clone(), (rssi, adverts))),
                _ => Err(format!("tag {id}: {tag}")),
            },
        )
        .collect()
}

/// Returns the reading's publisher.
fn validate_reading(payload: &[u8], model: Model) -> Result<String, String> {
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
    check_version(status["version"].as_str().ok_or("no version")?, commit)
}

/// The version must be built from `commit` without local changes.
fn check_version(version: &str, commit: &str) -> Result<(), String> {
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
            boards: Vec::new(),
            board_cycle: Duration::from_secs(67),
            tags: vec![TAG.into()],
            airs: Vec::new(),
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
        discovery(watch, TAG, "Cellar", &TAG_ENTITIES);
        let topic = format!("ruuvi2mqtt/{TAG}");
        for m in 0..minutes {
            watch.on_message(secs(30 + m * 60), &topic, reading(TALLI).as_bytes(), false);
            if m < duplicates {
                watch.on_message(secs(31 + m * 60), &topic, reading(PANNU).as_bytes(), false);
            }
        }
    }

    fn discovery(watch: &mut Watch, id: &str, name: &str, entities: &[(&str, &str)]) {
        for (component, entity) in entities {
            let topic = format!("homeassistant/{component}/ruuvi_{id}/{entity}/config");
            let config = format!(
                r#"{{"state_topic":"ruuvi2mqtt/{id}","unique_id":"ruuvi_{id}_{entity}","device":{{"name":"{name}"}}}}"#
            );
            watch.on_message(secs(0), &topic, config.as_bytes(), true);
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
            validate_reading(reading(PANNU).as_bytes(), Model::Tag),
            Ok(PANNU.to_string())
        );
        // A tag without the pressure sensor, in an old data format
        let no_pressure = reading(PANNU)
            .replace("1003.2", "null")
            .replace(r#""tx_power":4"#, r#""tx_power":null"#);
        assert!(validate_reading(no_pressure.as_bytes(), Model::Tag).is_ok());
        let no_temperature = reading(PANNU).replace("21.3", "null");
        assert_eq!(
            validate_reading(no_temperature.as_bytes(), Model::Tag),
            Err("temperature: null".into())
        );
        let no_publisher = reading(PANNU).replace(r#","publisher":"ruuvi2mqtt_pannu""#, "");
        assert_eq!(
            validate_reading(no_publisher.as_bytes(), Model::Tag),
            Err("publisher: null".into())
        );
        assert!(validate_reading(b"21.3", Model::Tag).is_err());
        assert_eq!(
            validate_reading(air_reading(PANNU).as_bytes(), Model::Tag),
            Err("battery: null".into())
        );
    }

    const AIR: &str = "c4d2a1b3e5f6";

    /// A format 6 reading: no PM1, PM4 or PM10
    fn air_reading(publisher: &str) -> String {
        format!(
            r#"{{"humidity":55.3,"pressure":1011.02,"temperature":29.5,"battery":null,"battery_low":null,"tx_power":null,"co2":201,"pm2_5":11.2,"voc":5,"nox":1,"luminosity":13026.67,"iaqs":81,"calibrating":false,"publisher":"{publisher}"}}"#
        )
    }

    #[test]
    fn air_readings_validate() {
        assert_eq!(
            validate_reading(air_reading(PANNU).as_bytes(), Model::Air),
            Ok(PANNU.to_string())
        );
        assert_eq!(
            validate_reading(reading(PANNU).as_bytes(), Model::Air),
            Err("battery: 2.9".into())
        );
        let battery_low =
            air_reading(PANNU).replace(r#""battery_low":null"#, r#""battery_low":false"#);
        assert_eq!(
            validate_reading(battery_low.as_bytes(), Model::Air),
            Err("battery_low: false".into())
        );
        let calibrating = air_reading(PANNU).replace(r#""calibrating":false,"#, "");
        assert_eq!(
            validate_reading(calibrating.as_bytes(), Model::Air),
            Err("calibrating: null".into())
        );
    }

    #[test]
    fn healthy_air_passes() {
        let mut w = watch();
        w.config.tags.push(AIR.into());
        w.config.airs.push(AIR.into());
        healthy(&mut w, 15);
        discovery(&mut w, AIR, "Living room", &AIR_ENTITIES);
        let topic = format!("ruuvi2mqtt/{AIR}");
        for m in 0..15 {
            w.on_message(
                secs(40 + m * 60),
                &topic,
                air_reading(TALLI).as_bytes(),
                false,
            );
        }
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
        assert!(
            report
                .text
                .contains("Living room (c4d2a1b3e5f6): 15 readings")
        );
        assert!(report.text.contains(", discovery 13/13\n"));
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
            "FAIL: 1 readings from esp32, not an instance or a board"
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
    fn unexpected_discovery_fails() {
        let mut w = watch();
        healthy(&mut w, 15);
        discovery(&mut w, TAG, "Cellar", &[("sensor", "co2")]);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: Cellar (f817624e4c0b): unexpected discovery configs: co2"
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
        w.on_message(secs(100), "zigbee2mqtt/bridge/state", b"online", false);
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

    const BOARD: &str = "ruuvi2mqtt-c6";

    /// A board's diagnostics: Wi-Fi connects 1 s into the 5 s scan
    fn diagnostics(firmware: &str, cycle: u64, tags: &str) -> String {
        format!(
            r#"{{"firmware":"ruuvi2mqtt-esp32 {firmware}","client_id":"{BOARD}","chip":"esp32c6","cycle":{cycle},"settings":{{"scan_mode":"parallel","scan_duration":5}},"wifi_rssi":-61,"wifi_channel":6,"wifi_cache":"hit","free_heap":151000,"scan_start_ms":200,"scan_end_ms":5200,"wifi_ms":1200,"mqtt_ms":2000,"cycle_ms":5300,"tags":{tags},"adverts_before_wifi":1,"adverts_after_wifi":8,"other_adverts_before_wifi":10,"other_adverts_after_wifi":80}}"#
        )
    }

    /// `minutes` of healthy instances, and a board cycle every 67 s, each
    /// with a reading of TAG right after TALLI's
    fn with_board(watch: &mut Watch, minutes: u64, firmware: &str) {
        healthy(watch, minutes);
        let topic = format!("ruuvi2mqtt/{TAG}");
        let tags = format!(r#"{{"{TAG}":{{"rssi":-74,"adverts":2,"first_ms":1210}}}}"#);
        for n in 0..minutes * 60 / 67 {
            let at = 31 + n * 67;
            watch.on_message(secs(at), &topic, reading(BOARD).as_bytes(), false);
            let payload = diagnostics(firmware, 10 + n, &tags);
            watch.on_message(
                secs(at),
                "ruuvi2mqtt/diagnostics/ruuvitest-c6",
                payload.as_bytes(),
                false,
            );
        }
    }

    fn watch_board() -> Watch {
        let mut w = watch();
        w.config.boards = vec![BOARD.into()];
        w
    }

    #[test]
    fn board_is_reported_and_not_a_duplicate() {
        let mut w = watch();
        with_board(&mut w, 15, "1.5.2-dev (abcdef0)");
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
        assert!(report.text.contains(concat!(
            "\nESP32 boards:\n",
            "  ruuvi2mqtt-c6 (ruuvitest-c6, esp32c6): 13 cycles, ruuvi2mqtt-esp32 1.5.2-dev (abcdef0)\n",
            "    settings: parallel 5s\n",
            "    hit rate 100% (13 of 13 tag cycles)\n",
            "    Cellar (f817624e4c0b): 13 of 13 cycles, RSSI -74 (-74..-74), 2.0 adverts per cycle heard\n",
            "    adverts/s before Wi-Fi connected: Ruuvi 1.0, other 10.0; after: Ruuvi 2.0, other 20.0\n",
            "    mean ms: Wi-Fi 1200, MQTT 2000, scan 200..5200, cycle 5300 (max 5300)\n",
            "    Wi-Fi cache 13 hit; cycle counter: 0 missed, 0 restarts\n",
        )), "{}", report.text);
        assert!(
            report
                .text
                .contains("from ruuvi2mqtt-c6 13, ruuvi2mqtt_talli 15,")
        );
    }

    #[test]
    fn checked_board_passes() {
        let mut w = watch_board();
        with_board(&mut w, 15, "1.5.2-dev (6534f11)");
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
    }

    #[test]
    fn checked_board_with_wrong_firmware_fails() {
        let mut w = watch_board();
        with_board(&mut w, 15, "1.5.2-dev (6534f11-dirty)");
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: ruuvi2mqtt-c6: ruuvi2mqtt-esp32 1.5.2-dev (6534f11-dirty) has local changes"
        );
    }

    #[test]
    fn checked_board_without_diagnostics_fails() {
        let mut w = watch_board();
        healthy(&mut w, 15);
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.summary, "FAIL: ruuvi2mqtt-c6: no diagnostics");
    }

    #[test]
    fn checked_board_with_a_gap_or_a_wrong_cycle_fails() {
        let mut w = watch_board();
        with_board(&mut w, 15, "1.5.2-dev (6534f11)");
        let report = w.finish(Duration::from_mins(30));
        assert_eq!(
            report.failures.last().map(String::as_str),
            Some("ruuvi2mqtt-c6: no diagnostics for 16:05")
        );

        let mut w = watch_board();
        w.config.board_cycle = Duration::from_secs(30);
        with_board(&mut w, 15, "1.5.2-dev (6534f11)");
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(
            report.summary,
            "FAIL: ruuvi2mqtt-c6: a cycle every 67s, not 30s"
        );
    }

    #[test]
    fn missed_cycles_and_restarts_are_counted() {
        let mut w = watch();
        healthy(&mut w, 5);
        let topic = "ruuvi2mqtt/diagnostics/ruuvitest-c6";
        for (at, cycle) in [(0, 7), (67, 8), (201, 10), (268, 2)] {
            let payload = diagnostics("1.5.2-dev (6534f11)", cycle, "{}");
            w.on_message(secs(at), topic, payload.as_bytes(), false);
        }
        let report = w.finish(Duration::from_mins(5));
        assert!(
            report
                .text
                .contains("    hit rate 0% (0 of 4 tag cycles)\n")
        );
        assert!(
            report
                .text
                .contains("cycle counter: 2 missed, 1 restarts\n"),
            "{}",
            report.text
        );
    }

    #[test]
    fn sequential_scan_has_no_time_after_wifi() {
        let payload = diagnostics("1.5.2-dev", 1, "{}")
            .replace(
                r#""parallel","scan_duration":5"#,
                r#""sequential","scan_duration":8,"antenna":"external""#,
            )
            .replace(r#""wifi_ms":1200"#, r#""wifi_ms":9500"#);
        let (_, d) = parse_diagnostics(secs(0), "c6", payload.as_bytes()).unwrap();
        assert_eq!(d.settings, "sequential 8s, external antenna");
        assert_eq!(d.scan_windows(), [5000, 0]);
        assert_eq!(per_second(0, 0), "-");
    }

    #[test]
    fn invalid_diagnostics_fail() {
        let mut w = watch();
        healthy(&mut w, 15);
        let topic = "ruuvi2mqtt/diagnostics/esp32";
        let note = w.on_message(secs(100), topic, br#"{"firmware":"x"}"#, false);
        assert_eq!(note.as_deref(), Some("+1:40 esp32: client_id: null"));
        let bad_tag = diagnostics("1.5.2-dev", 1, r#"{"f817624e4c0b":{"rssi":"weak"}}"#);
        let note = w.on_message(secs(160), topic, bad_tag.as_bytes(), false);
        assert_eq!(
            note.as_deref(),
            Some(r#"+2:40 esp32: tag f817624e4c0b: {"rssi":"weak"}"#)
        );
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.summary, "FAIL: 2 invalid diagnostics");
    }
}
