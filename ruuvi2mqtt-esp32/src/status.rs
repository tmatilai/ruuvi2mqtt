//! The status page's report: what the board hears right now, and how good
//! its spot is.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::mac::Mac;

/// The report covers the last minute.
pub const WINDOW_MS: u64 = 60_000;

/// Below this much of the window, the rates are too rough to grade, and a
/// tag may not have been heard yet.
const MEASURING_MS: u64 = 20_000;

/// An Air sends a few reports a second, one per advertising set.
const MAX_REPORTS: usize = 600;

/// How often [`Live::record`] drops the old reports.
const PRUNE_MS: u64 = 10_000;

/// The retained Home Assistant configs that the Linux app publishes, one per
/// tag. Both models have a temperature.
pub const DISCOVERY_TOPIC: &str = "homeassistant/sensor/+/temperature/config";

/// How good something is, worst last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    Good,
    Fair,
    Poor,
}

impl Grade {
    /// Wi-Fi below −75 dBm drops and retries.
    fn wifi(rssi: i8) -> Self {
        match rssi {
            -67.. => Self::Good,
            -75.. => Self::Fair,
            _ => Self::Poor,
        }
    }

    /// Below −90 dBm, a tag fades in and out.
    fn rssi(rssi: i8) -> Self {
        match rssi {
            -80.. => Self::Good,
            -90.. => Self::Fair,
            _ => Self::Poor,
        }
    }

    /// A cycle publishes a tag if one report arrives, so a few a minute
    /// leave little margin.
    fn rate(per_min: u32) -> Self {
        match per_min {
            10.. => Self::Good,
            3.. => Self::Fair,
            _ => Self::Poor,
        }
    }
}

/// The reports of the last minute, and the tags' names.
#[derive(Debug, Default)]
pub struct Live {
    /// Each report's time, in ms since boot, and RSSI.
    heard: BTreeMap<Mac, VecDeque<(u64, i8)>>,
    names: BTreeMap<Mac, String>,
    /// When [`Live::prune`] last ran.
    pruned_ms: u64,
}

/// The board's own values for [`Live::report`].
#[derive(Debug, Clone, Copy)]
pub struct Board<'a> {
    pub hostname: &'a str,
    pub firmware: &'a str,
    /// The `ANTENNA` setting.
    pub antenna: Option<&'a str>,
    pub uptime_ms: u64,
    /// The access point's RSSI and channel.
    pub wifi: (i8, u8),
    /// Since boot.
    pub wifi_drops: u32,
    /// The cycles since boot, how many failed, and whether the last did.
    pub cycles: u32,
    pub cycles_failed: u32,
    pub last_cycle_failed: bool,
}

/// The JSON of `/api/status`.
#[derive(Debug, Serialize)]
pub struct Report<'a> {
    pub hostname: &'a str,
    pub firmware: &'a str,
    pub antenna: Option<&'a str>,
    pub uptime_s: u64,
    /// How much the rates cover, up to a minute.
    pub window_s: u64,
    /// Set while the window is too short to grade the rates.
    pub measuring: bool,
    /// The worst of Wi-Fi, publishing and the target tags.
    pub verdict: Grade,
    /// What isn't good, worst first.
    pub problems: Vec<Problem>,
    pub wifi: Wifi,
    pub publishing: Publishing,
    /// The target tags first, then the others by name. A tag in the
    /// discovery configs is listed even when not heard.
    pub tags: Vec<Tag>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub grade: Grade,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct Wifi {
    pub rssi: i8,
    pub channel: u8,
    /// Since boot. One makes the grade fair at best.
    pub drops: u32,
    pub grade: Grade,
}

/// Fair until the first cycle, poor while the last one failed, and fair
/// while any since boot did.
#[derive(Debug, Serialize)]
pub struct Publishing {
    pub cycles: u32,
    pub failed: u32,
    pub grade: Grade,
}

#[derive(Debug, Serialize)]
pub struct Tag {
    pub mac: Mac,
    pub name: Option<String>,
    /// One that the spot should cover.
    pub target: bool,
    pub reports_per_min: u32,
    /// The mean and the range, in dBm. Unset when not heard.
    pub rssi: Option<i8>,
    pub rssi_min: Option<i8>,
    pub rssi_max: Option<i8>,
    pub last_heard_s: Option<u64>,
    pub grade: Grade,
}

impl Live {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            heard: BTreeMap::new(),
            names: BTreeMap::new(),
            pruned_ms: 0,
        }
    }

    pub fn record(&mut self, mac: Mac, rssi: i8, now_ms: u64) {
        let reports = self.heard.entry(mac).or_default();
        if reports.len() == MAX_REPORTS {
            reports.pop_front();
        }
        reports.push_back((now_ms, rssi));
        // Without the page open, nothing else drops them.
        if now_ms.saturating_sub(self.pruned_ms) > PRUNE_MS {
            self.prune(now_ms);
        }
    }

    /// Drops the reports older than the window, and the tags left without.
    fn prune(&mut self, now: u64) {
        for reports in self.heard.values_mut() {
            while reports
                .front()
                .is_some_and(|&(at, _)| now.saturating_sub(at) > WINDOW_MS)
            {
                reports.pop_front();
            }
        }
        self.heard.retain(|_, reports| !reports.is_empty());
        self.pruned_ms = now;
    }

    /// A tag's name from its discovery config, or `None` once the config is
    /// cleared.
    pub fn set_name(&mut self, mac: Mac, name: Option<String>) {
        match name {
            Some(name) => self.names.insert(mac, name),
            None => self.names.remove(&mac),
        };
    }

    /// Grades the spot for the `targets`.
    pub fn report<'a>(&mut self, board: &Board<'a>, targets: &[Mac]) -> Report<'a> {
        let now = board.uptime_ms;
        self.prune(now);

        let window_ms = now.clamp(1, WINDOW_MS);
        let measuring = window_ms < MEASURING_MS;

        let macs = self.heard.keys().chain(self.names.keys());
        let mut tags: Vec<Tag> = macs
            .chain(targets)
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|mac| self.tag(mac, targets.contains(&mac), now, window_ms, measuring))
            .collect();
        tags.sort_by(|a, b| {
            (!a.target, a.name.is_none(), &a.name, a.mac).cmp(&(
                !b.target,
                b.name.is_none(),
                &b.name,
                b.mac,
            ))
        });

        let (rssi, channel) = board.wifi;
        let mut wifi_grade = Grade::wifi(rssi);
        if board.wifi_drops > 0 {
            wifi_grade = wifi_grade.max(Grade::Fair);
        }
        let wifi = Wifi {
            rssi,
            channel,
            drops: board.wifi_drops,
            grade: wifi_grade,
        };
        let publishing = Publishing {
            cycles: board.cycles,
            failed: board.cycles_failed,
            grade: if board.last_cycle_failed {
                Grade::Poor
            } else if board.cycles == 0 || board.cycles_failed > 0 {
                Grade::Fair
            } else {
                Grade::Good
            },
        };

        let mut problems = Vec::new();
        if wifi.grade != Grade::Good {
            let mut details = Vec::new();
            if Grade::wifi(rssi) != Grade::Good {
                details.push(format!("{rssi} dBm"));
            }
            if wifi.drops > 0 {
                details.push(format!("dropped {}", times(wifi.drops)));
            }
            problems.push(Problem {
                grade: wifi.grade,
                text: format!("Wi-Fi: {}", details.join(", ")),
            });
        }
        if publishing.grade != Grade::Good {
            let text = if publishing.cycles == 0 {
                "Publishing: not yet".to_string()
            } else {
                format!(
                    "Publishing: {} of {} failed",
                    publishing.failed,
                    cycles(publishing.cycles)
                )
            };
            problems.push(Problem {
                grade: publishing.grade,
                text,
            });
        }
        for tag in tags.iter().filter(|t| t.target && t.grade != Grade::Good) {
            problems.push(Problem {
                grade: tag.grade,
                text: tag_problem(tag, measuring),
            });
        }
        // Stable, so the order above stays within a grade.
        problems.sort_by_key(|p| Reverse(p.grade));

        let verdict = problems
            .iter()
            .map(|p| p.grade)
            .max()
            .unwrap_or(Grade::Good);

        Report {
            hostname: board.hostname,
            firmware: board.firmware,
            antenna: board.antenna,
            uptime_s: now / 1000,
            window_s: window_ms / 1000,
            measuring,
            verdict,
            problems,
            wifi,
            publishing,
            tags,
        }
    }

    fn tag(&self, mac: Mac, target: bool, now: u64, window_ms: u64, measuring: bool) -> Tag {
        let name = self.names.get(&mac).cloned();
        let Some(reports) = self.heard.get(&mac) else {
            return Tag {
                mac,
                name,
                target,
                reports_per_min: 0,
                rssi: None,
                rssi_min: None,
                rssi_max: None,
                last_heard_s: None,
                // It may not have sent yet.
                grade: if measuring { Grade::Fair } else { Grade::Poor },
            };
        };
        let count = reports.len() as u64;
        let sum: i64 = reports.iter().map(|&(_, rssi)| i64::from(rssi)).sum();
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        // a mean of i8 values
        let mean = (sum as f64 / count as f64).round() as i8;
        let per_min = u32::try_from(count * 60_000 / window_ms).unwrap_or(u32::MAX);
        let mut grade = Grade::rssi(mean);
        if !measuring {
            grade = grade.max(Grade::rate(per_min));
        }
        Tag {
            mac,
            name,
            target,
            reports_per_min: per_min,
            rssi: Some(mean),
            rssi_min: reports.iter().map(|&(_, rssi)| rssi).min(),
            rssi_max: reports.iter().map(|&(_, rssi)| rssi).max(),
            last_heard_s: reports.back().map(|&(at, _)| now.saturating_sub(at) / 1000),
            grade,
        }
    }
}

fn times(n: u32) -> String {
    if n == 1 {
        "once".to_string()
    } else {
        format!("{n} times")
    }
}

fn cycles(n: u32) -> String {
    if n == 1 {
        "1 cycle".to_string()
    } else {
        format!("{n} cycles")
    }
}

/// What is wrong with a target tag, by name, with the numbers that aren't
/// good.
fn tag_problem(tag: &Tag, measuring: bool) -> String {
    let name = tag.name.clone().unwrap_or_else(|| tag.mac.to_string());
    let Some(rssi) = tag.rssi else {
        let not_heard = if measuring {
            "not heard yet"
        } else {
            "not heard"
        };
        return format!("{name}: {not_heard}");
    };
    let mut details = Vec::new();
    if Grade::rssi(rssi) != Grade::Good {
        details.push(format!("{rssi} dBm"));
    }
    if !measuring && Grade::rate(tag.reports_per_min) != Grade::Good {
        details.push(format!("{} reports a minute", tag.reports_per_min));
    }
    format!("{name}: {}", details.join(", "))
}

/// The target tags in `/api/status?tags=<mac>,<mac>`, as the page sends
/// them.
#[must_use]
pub fn targets(uri: &str) -> Vec<Mac> {
    let Some((_, query)) = uri.split_once('?') else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("tags="))
        .flat_map(|macs| macs.split([',', ' ']))
        .filter_map(Mac::from_topic_str)
        .collect()
}

/// A tag's name from a message on [`DISCOVERY_TOPIC`]: `None` for another
/// topic, and `Some((mac, None))` once the config is cleared.
#[must_use]
pub fn discovery_name(topic: &str, payload: &[u8]) -> Option<(Mac, Option<String>)> {
    #[derive(Deserialize)]
    struct Config {
        device: Device,
    }
    #[derive(Deserialize)]
    struct Device {
        name: String,
    }

    let id = topic
        .strip_prefix("homeassistant/sensor/ruuvi_")?
        .strip_suffix("/temperature/config")?;
    let mac = Mac::from_topic_str(id)?;
    if payload.is_empty() {
        return Some((mac, None));
    }
    let config: Config = serde_json::from_slice(payload).ok()?;
    Some((mac, Some(config.device.name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEDROOM: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
    const OFFICE: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02]);
    const STRANGER: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x03]);

    fn board(uptime_ms: u64) -> Board<'static> {
        Board {
            hostname: "ruuvi2mqtt-test",
            firmware: "ruuvi2mqtt-esp32 1.6.1-dev (4518da5)",
            antenna: None,
            uptime_ms,
            wifi: (-58, 6),
            wifi_drops: 0,
            cycles: 12,
            cycles_failed: 0,
            last_cycle_failed: false,
        }
    }

    /// `count` reports at `rssi`, spread over the minute before `now`.
    fn hear(live: &mut Live, mac: Mac, rssi: i8, count: u64, now: u64) {
        for i in 0..count {
            live.record(mac, rssi, now - WINDOW_MS + i * WINDOW_MS / count);
        }
    }

    fn named() -> Live {
        let mut live = Live::default();
        live.set_name(BEDROOM, Some("Bedroom".into()));
        live.set_name(OFFICE, Some("Office".into()));
        live
    }

    #[test]
    fn a_good_spot() {
        let mut live = named();
        hear(&mut live, BEDROOM, -72, 40, 120_000);
        hear(&mut live, OFFICE, -88, 30, 120_000);
        let report = live.report(&board(120_000), &[BEDROOM]);
        assert_eq!(report.verdict, Grade::Good);
        assert_eq!(report.problems, []);
        assert!(!report.measuring);
        assert_eq!(report.window_s, 60);
        let bedroom = &report.tags[0];
        assert_eq!(
            (bedroom.name.as_deref(), bedroom.target, bedroom.grade),
            (Some("Bedroom"), true, Grade::Good)
        );
        assert_eq!((bedroom.reports_per_min, bedroom.rssi), (40, Some(-72)));
        // Office isn't a target, so its marginal RSSI doesn't count.
        assert_eq!(report.tags[1].grade, Grade::Fair);
    }

    #[test]
    fn lists_the_problems_worst_first() {
        let mut live = named();
        hear(&mut live, BEDROOM, -86, 40, 120_000);
        hear(&mut live, STRANGER, -60, 40, 120_000);
        let board = Board {
            wifi: (-71, 11),
            wifi_drops: 1,
            cycles_failed: 2,
            last_cycle_failed: true,
            ..board(120_000)
        };
        let report = live.report(&board, &[BEDROOM, OFFICE]);
        assert_eq!(report.verdict, Grade::Poor);
        let problems: Vec<_> = report
            .problems
            .iter()
            .map(|p| (p.grade, p.text.as_str()))
            .collect();
        assert_eq!(
            problems,
            [
                (Grade::Poor, "Publishing: 2 of 12 cycles failed"),
                (Grade::Poor, "Office: not heard"),
                (Grade::Fair, "Wi-Fi: -71 dBm, dropped once"),
                (Grade::Fair, "Bedroom: -86 dBm"),
            ]
        );
        // Targets first, then by name, the unnamed last.
        let order: Vec<_> = report.tags.iter().map(|t| t.mac).collect();
        assert_eq!(order, [BEDROOM, OFFICE, STRANGER]);
    }

    #[test]
    fn grades_the_rate_once_measured() {
        let mut live = named();
        hear(&mut live, BEDROOM, -70, 2, 120_000);
        let report = live.report(&board(120_000), &[BEDROOM]);
        assert_eq!(report.problems[0].text, "Bedroom: 2 reports a minute");
        assert_eq!(report.verdict, Grade::Poor);

        let mut live = named();
        live.record(BEDROOM, -70, 9_000);
        let report = live.report(&board(10_000), &[BEDROOM, OFFICE]);
        assert!(report.measuring);
        assert_eq!(report.window_s, 10);
        assert_eq!(report.tags[0].reports_per_min, 6);
        assert_eq!(report.tags[0].grade, Grade::Good);
        assert_eq!(report.problems[0].text, "Office: not heard yet");
        assert_eq!(report.verdict, Grade::Fair);
    }

    #[test]
    fn drops_old_reports() {
        let mut live = Live::default();
        live.record(STRANGER, -60, 1_000);
        live.record(BEDROOM, -80, 1_000);
        live.record(BEDROOM, -70, 50_000);
        let report = live.report(&board(70_000), &[]);
        let tags: Vec<_> = report
            .tags
            .iter()
            .map(|t| (t.mac, t.rssi_min, t.rssi_max, t.last_heard_s))
            .collect();
        assert_eq!(tags, [(BEDROOM, Some(-70), Some(-70), Some(20))]);
        assert!(!live.heard.contains_key(&STRANGER));
    }

    #[test]
    fn drops_old_reports_without_the_page() {
        let mut live = Live::default();
        live.record(STRANGER, -60, 1_000);
        live.record(BEDROOM, -80, 1_000);
        live.record(BEDROOM, -70, 70_000);
        assert_eq!(live.heard.len(), 1);
        assert_eq!(live.heard[&BEDROOM].len(), 1);
    }

    #[test]
    fn before_the_first_cycle() {
        let board = Board {
            cycles: 0,
            ..board(30_000)
        };
        let report = Live::default().report(&board, &[]);
        assert_eq!(report.verdict, Grade::Fair);
        assert_eq!(report.problems[0].text, "Publishing: not yet");
        assert_eq!(report.tags.len(), 0);
    }

    #[test]
    fn counts_since_boot() {
        let dropped = Board {
            wifi_drops: 3,
            ..board(120_000)
        };
        let report = Live::default().report(&dropped, &[]);
        assert_eq!(report.verdict, Grade::Fair);
        assert_eq!(report.problems[0].text, "Wi-Fi: dropped 3 times");

        let failed = Board {
            cycles: 1,
            cycles_failed: 1,
            last_cycle_failed: true,
            ..board(120_000)
        };
        let report = Live::default().report(&failed, &[]);
        assert_eq!(report.verdict, Grade::Poor);
        assert_eq!(report.problems[0].text, "Publishing: 1 of 1 cycle failed");
    }

    #[test]
    fn json() {
        let mut live = named();
        live.record(BEDROOM, -71, 115_000);
        live.record(BEDROOM, -75, 118_000);
        let board = Board {
            antenna: Some("internal"),
            ..board(120_000)
        };
        let json = serde_json::to_string(&live.report(&board, &[BEDROOM])).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"hostname":"ruuvi2mqtt-test","firmware":"ruuvi2mqtt-esp32 1.6.1-dev (4518da5)","#,
                r#""antenna":"internal","uptime_s":120,"window_s":60,"measuring":false,"verdict":"poor","#,
                r#""problems":[{"grade":"poor","text":"Bedroom: 2 reports a minute"}],"#,
                r#""wifi":{"rssi":-58,"channel":6,"drops":0,"grade":"good"},"#,
                r#""publishing":{"cycles":12,"failed":0,"grade":"good"},"#,
                r#""tags":[{"mac":"aabbccddee01","name":"Bedroom","target":true,"reports_per_min":2,"#,
                r#""rssi":-73,"rssi_min":-75,"rssi_max":-71,"last_heard_s":2,"grade":"poor"},"#,
                r#"{"mac":"aabbccddee02","name":"Office","target":false,"reports_per_min":0,"#,
                r#""rssi":null,"rssi_min":null,"rssi_max":null,"last_heard_s":null,"grade":"poor"}]}"#
            )
        );
    }

    #[test]
    fn parses_the_targets() {
        assert_eq!(
            targets("/api/status?tags=aabbccddee01,AABBCCDDEE02,bad"),
            [BEDROOM, OFFICE]
        );
        assert_eq!(targets("/api/status?x=1&tags=aabbccddee01"), [BEDROOM]);
        assert_eq!(targets("/api/status"), []);
        assert_eq!(targets("/api/status?tags="), []);
    }

    #[test]
    fn reads_the_name_from_discovery() {
        let topic = "homeassistant/sensor/ruuvi_aabbccddee01/temperature/config";
        let payload = br#"{"name":"Bedroom Temperature","device":{"name":"Bedroom","identifiers":["AA:BB:CC:DD:EE:01"],"manufacturer":"Ruuvi"}}"#;
        assert_eq!(
            discovery_name(topic, payload),
            Some((BEDROOM, Some("Bedroom".into())))
        );
        assert_eq!(discovery_name(topic, b""), Some((BEDROOM, None)));
        assert_eq!(discovery_name(topic, b"{}"), None);
        assert_eq!(
            discovery_name(
                "homeassistant/sensor/ruuvi_aabbccddee01/humidity/config",
                payload
            ),
            None
        );
    }
}
