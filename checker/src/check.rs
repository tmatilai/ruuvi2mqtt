//! The checks, without MQTT, so that unit tests can run them.

mod boards;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Duration;

use crate::diagnostics::{self, Diagnostics};
use crate::payload::{self, Model, Reading, printable};
use crate::time;
use crate::topic::Topic;

/// How many examples of a problem the report lists
const MAX_EXAMPLES: usize = 5;

pub struct Config {
    pub base_topic: String,
    /// The commit under test, at least 7 hex digits
    pub commit: String,
    /// The app instances' MQTT client IDs
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
    /// The tags' longest gap before the watch, from the recorder. A tag gets
    /// twice its gap for its first reading and its gaps, if that is longer.
    pub gaps: BTreeMap<String, Duration>,
    pub first_reading: Duration,
    pub max_gap: Duration,
    pub throttle: Duration,
    /// Percentage of a tag's readings that may be duplicates
    pub max_duplicates: u8,
}

impl Config {
    /// A tag's limits for its first reading and its gaps
    fn limits(&self, id: &str) -> (Duration, Duration) {
        let slack = self
            .gaps
            .get(id)
            .map_or(Duration::ZERO, |gap| gap.saturating_mul(2));
        (self.first_reading.max(slack), self.max_gap.max(slack))
    }

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
    /// The instances' discovery configs: topic to client ID
    instance_discovery: BTreeMap<String, String>,
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
    /// Whether it is online with the commit under test
    check: Result<(), String>,
}

#[derive(Default)]
struct Tag {
    /// From the discovery configs
    name: Option<String>,
    entities: BTreeSet<&'static str>,
    /// When, and from which publisher
    readings: Vec<(Duration, String)>,
    publishers: BTreeMap<String, usize>,
    /// An Air's readings with data format E1's values, by publisher
    e1: BTreeMap<String, usize>,
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
            instance_discovery: BTreeMap::new(),
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
        match Topic::parse(&self.config.base_topic, topic) {
            Topic::Status(client_id) => self.on_status(at, client_id, payload, retained),
            Topic::Diagnostics(hostname) => self.on_diagnostics(at, hostname, payload, retained),
            Topic::Reading(id) => self.on_reading(at, id, payload, retained),
            Topic::Discovery { id, entity } => self.on_discovery(topic, id, entity, payload),
            Topic::StatusDiscovery => {
                // An empty payload deletes the config
                match payload::instance_discovery(payload, &self.config.base_topic) {
                    Some(id) => self.instance_discovery.insert(topic.to_string(), id),
                    None => self.instance_discovery.remove(topic),
                };
                None
            }
            Topic::Other => {
                self.other_topics += 1;
                None
            }
        }
    }

    fn on_status(
        &mut self,
        at: Duration,
        client_id: &str,
        payload: &[u8],
        retained: bool,
    ) -> Option<String> {
        let raw = String::from_utf8_lossy(payload);
        let payload = printable(&raw);
        let history = self.statuses.entry(client_id.to_string()).or_default();
        // The broker sends the retained status again when the checker reconnects
        if retained && history.last().is_some_and(|s| s.payload == payload) {
            return None;
        }
        let watched = (self.config.instances.iter())
            .chain(&self.config.boards)
            .any(|i| i == client_id);
        let note = (watched && !(history.is_empty() && retained))
            .then(|| format!("+{} {client_id}: {payload}", time::format(at)));
        history.push(Status {
            at,
            retained,
            payload,
            check: payload::check_status(&raw, &self.config.commit),
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
            payload::validate_reading(payload, self.config.model(id))
        };
        match result {
            Ok(Reading { publisher, e1 }) => {
                let tag = self.tags.entry(id.to_string()).or_default();
                *tag.publishers.entry(publisher.clone()).or_default() += 1;
                if e1 {
                    *tag.e1.entry(publisher.clone()).or_default() += 1;
                }
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
            diagnostics::parse(at, hostname, payload)
        };
        match result {
            Ok(cycle) => {
                self.boards
                    .entry(cycle.client_id.clone())
                    .or_default()
                    .push(cycle);
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
        let Some(config) = payload::parse_discovery(payload, &self.config.base_topic, id, entity)
        else {
            tag.entities.remove(entity);
            let payload = printable(&String::from_utf8_lossy(payload));
            return self.invalid_discovery.add(format!("{topic}: {payload}"));
        };
        tag.entities.insert(entity);
        if let Some(name) = payload::device_name(&config) {
            tag.name = Some(name);
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
            self.check_status(r, id, &format!("  {id}: "));
            if !self.instance_discovery.values().any(|i| i == id) {
                let _ = writeln!(r.text, "    no discovery config");
                r.failures.push(format!("{id}: no discovery config"));
            }
        }
        let boards = self.board_ids();
        let others: Vec<_> = self
            .statuses
            .iter()
            .filter(|(id, _)| !c.instances.contains(id) && !boards.contains(id.as_str()))
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
            self.check_timing(r, id, &label, end);
            let model = c.model(id);
            if let Some(tag) = tag.filter(|t| !t.publishers.is_empty()) {
                // Only reported: an ESP32, and an instance without a
                // Bluetooth 5 adapter, get only an Air's format 6
                let publishers: Vec<String> = tag
                    .publishers
                    .iter()
                    .map(|(p, n)| match model {
                        Model::Tag => format!("{p} {n}"),
                        Model::Air => {
                            format!("{p} {n} ({} E1)", tag.e1.get(p).copied().unwrap_or(0))
                        }
                    })
                    .collect();
                let _ = write!(r.text, ", from {}", publishers.join(", "));
            }
            let expected = model.entities();
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
            // The app clears another model's configs, as one left retained
            // stays in Home Assistant
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

    /// A tag's first reading, gaps and duplicates
    fn check_timing(&self, r: &mut Report, id: &str, label: &str, end: Duration) {
        let c = &self.config;
        let tag = self.tags.get(id);
        let readings: Vec<Duration> =
            tag.map_or_else(Vec::new, |t| t.readings.iter().map(|(at, _)| *at).collect());
        let _ = write!(r.text, "  {label}: {} readings", readings.len());
        let Some(&first) = readings.first() else {
            r.failures.push(format!("{label}: no readings"));
            return;
        };
        let max_gap = time::max_gap(&readings, first, end);
        // The boards don't throttle, so only the instances' readings count
        let boards = self.board_ids();
        let throttled: Vec<Duration> = tag
            .iter()
            .flat_map(|t| &t.readings)
            .filter(|(_, publisher)| !boards.contains(publisher.as_str()))
            .map(|(at, _)| *at)
            .collect();
        let duplicate_within = c.throttle.saturating_mul(9) / 10;
        let duplicates = throttled
            .windows(2)
            .filter(|w| w[1].saturating_sub(w[0]) < duplicate_within)
            .count();
        let _ = write!(
            r.text,
            ", first +{}, max gap {}",
            time::format(first),
            time::format(max_gap)
        );
        if let Some(gap) = c.gaps.get(id) {
            let _ = write!(r.text, " ({} before)", time::format(*gap));
        }
        let _ = write!(r.text, ", {duplicates} duplicates");
        let (first_limit, gap_limit) = c.limits(id);
        if first > first_limit {
            r.failures.push(format!(
                "{label}: first reading only at +{}",
                time::format(first)
            ));
        }
        if max_gap > gap_limit {
            r.failures
                .push(format!("{label}: no reading for {}", time::format(max_gap)));
        }
        if duplicates * 100 > usize::from(c.max_duplicates) * throttled.len() {
            r.failures.push(format!(
                "{label}: {duplicates} of {} readings within the throttle",
                throttled.len()
            ));
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
    fn check_status(&self, r: &mut Report, id: &str, head: &str) {
        let Some(history) = self.statuses.get(id) else {
            let _ = writeln!(r.text, "{head}no status");
            r.failures.push(format!("{id}: no status"));
            return;
        };
        let first = &history[0];
        let _ = writeln!(r.text, "{head}{}", first.payload);
        let indent = head.len() - head.trim_start().len() + 2;
        for status in &history[1..] {
            let _ = writeln!(
                r.text,
                "{:indent$}+{} {}",
                "",
                time::format(status.at),
                status.payload
            );
        }
        if !first.retained {
            r.failures
                .push(format!("{id}: came online at +{}", time::format(first.at)));
        } else if let Err(err) = &first.check {
            r.failures.push(format!("{id}: {err}"));
        } else if history.len() > 1 {
            r.failures.push(format!(
                "{id}: the status changed {} times",
                history.len() - 1
            ));
        }
    }

    fn board_ids(&self) -> BTreeSet<&str> {
        self.config
            .boards
            .iter()
            .chain(self.boards.keys())
            .map(String::as_str)
            .collect()
    }

    fn label(&self, id: &str) -> String {
        match self.tags.get(id).and_then(|t| t.name.as_deref()) {
            Some(name) => format!("{name} ({id})"),
            None => id.to_string(),
        }
    }
}
