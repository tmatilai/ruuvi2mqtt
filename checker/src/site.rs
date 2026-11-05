//! The site table: how well each instance and board hears each tag, from
//! their diagnostics. Without MQTT, so that unit tests can render it.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Duration;

use crate::diagnostics;
use crate::payload::{self, device_name, parse_discovery};
use crate::stats::{Mean, count};
use crate::time;
use crate::topic::Topic;

/// How good something is, worst last. The RSSI and Wi-Fi thresholds are the
/// boards' status page's, in `ruuvi2mqtt-esp32/src/status.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Grade {
    Good,
    Fair,
    Poor,
}

impl Grade {
    /// Wi-Fi below −75 dBm drops and retries.
    fn wifi(rssi: i64) -> Self {
        match rssi {
            -67.. => Self::Good,
            -75.. => Self::Fair,
            _ => Self::Poor,
        }
    }

    /// Below −90 dBm, a tag fades in and out.
    fn rssi(rssi: i64) -> Self {
        match rssi {
            -80.. => Self::Good,
            -90.. => Self::Fair,
            _ => Self::Poor,
        }
    }

    /// The share of the periods that heard a tag: a publisher publishes it
    /// only in those.
    fn heard(percent: u64) -> Self {
        match percent {
            90.. => Self::Good,
            50.. => Self::Fair,
            _ => Self::Poor,
        }
    }

    fn mark(self) -> &'static str {
        match self {
            Self::Good => "",
            Self::Fair => "~",
            Self::Poor => "!",
        }
    }
}

/// What the broker sends while the site command listens.
pub struct Site {
    base_topic: String,
    /// Tag ID to name, from the discovery configs
    names: BTreeMap<String, String>,
    /// By the name in their diagnostics topic
    publishers: BTreeMap<String, Publisher>,
}

#[derive(Default)]
struct Publisher {
    board: bool,
    /// The diagnostics it sent: an instance's minutes or a board's cycles
    periods: u64,
    /// By tag ID
    tags: BTreeMap<String, Heard>,
    /// A board's, from its last cycle
    firmware: Option<String>,
    cycle: Option<u32>,
    restarts: u64,
    wifi_rssi: Mean,
}

/// The periods that heard a tag, and the mean of their strongest RSSIs
#[derive(Default)]
struct Heard {
    periods: u64,
    rssi: Mean,
}

/// How well a publisher hears a tag
#[derive(Clone, Copy)]
struct Cell {
    rssi: Option<i64>,
    /// Percent of the periods
    heard: u64,
}

impl Cell {
    fn grade(&self) -> Grade {
        let rssi = self.rssi.map_or(Grade::Good, Grade::rssi);
        rssi.max(Grade::heard(self.heard))
    }
}

impl Site {
    pub fn new(base_topic: &str) -> Self {
        Self {
            base_topic: base_topic.to_string(),
            names: BTreeMap::new(),
            publishers: BTreeMap::new(),
        }
    }

    pub fn on_message(&mut self, topic: &str, payload: &[u8]) {
        match Topic::parse(&self.base_topic, topic) {
            Topic::Discovery { id, entity } => {
                let name = parse_discovery(payload, &self.base_topic, id, entity)
                    .as_ref()
                    .and_then(device_name);
                if let Some(name) = name {
                    self.names.insert(id.to_string(), name);
                }
            }
            Topic::Diagnostics(name) => self.on_diagnostics(name, payload),
            _ => {}
        }
    }

    /// The check reports invalid diagnostics. The table skips them.
    fn on_diagnostics(&mut self, name: &str, payload: &[u8]) {
        if let Some(minute) = payload::parse_diagnostics(Duration::ZERO, payload) {
            if let Ok(minute) = minute {
                let p = self.publishers.entry(name.to_string()).or_default();
                p.add(minute.tags.iter().map(|(id, (rssi, _))| (id, *rssi)));
            }
        } else if let Ok(cycle) = diagnostics::parse(Duration::ZERO, name, payload) {
            let p = self.publishers.entry(name.to_string()).or_default();
            p.board_cycle(&cycle);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.publishers.is_empty()
    }

    pub fn render(&self, over: Duration) -> String {
        let mut out = String::new();
        // The instances first
        let mut columns: Vec<(&String, &Publisher)> = self.publishers.iter().collect();
        columns.sort_by_key(|(_, p)| p.board);
        let boards = columns.iter().filter(|(_, p)| p.board).count();
        let _ = writeln!(
            out,
            "Over {}, the diagnostics of {} and {}.",
            time::format(over),
            plural(columns.len() - boards, "instance"),
            plural(boards, "board"),
        );
        let mut rows: Vec<(&String, &String)> = self.names.iter().collect();
        rows.sort_by_key(|(id, name)| (*name, *id));

        let problems = problems(&columns, &rows);
        if !problems.is_empty() {
            let _ = writeln!(out, "\nProblems:");
            for (grade, text) in problems {
                let _ = writeln!(out, "  {} {text}", grade.mark());
            }
        }

        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|(id, _)| {
                columns
                    .iter()
                    .map(|(_, p)| p.cell(id).map_or("-".to_string(), cell_text))
                    .collect()
            })
            .collect();
        let labels: Vec<&str> = columns.iter().map(|(name, _)| label(name)).collect();
        let kinds: Vec<&str> = columns
            .iter()
            .map(|(_, p)| if p.board { "board" } else { "instance" })
            .collect();
        let widths: Vec<usize> = (0..columns.len())
            .map(|i| {
                let cells = cells.iter().map(|row| row[i].chars().count());
                cells
                    .chain([labels[i].chars().count(), kinds[i].len()])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let name_width = rows
            .iter()
            .map(|(_, name)| name.chars().count())
            .max()
            .unwrap_or(0);
        let _ = writeln!(out);
        for header in [&labels, &kinds] {
            let _ = write!(out, "{:name_width$}", "");
            for (text, width) in header.iter().zip(&widths) {
                let _ = write!(out, "  {text:>width$}");
            }
            let _ = writeln!(out);
        }
        for ((_, name), row) in rows.iter().zip(&cells) {
            let _ = write!(out, "{name:name_width$}");
            for (text, width) in row.iter().zip(&widths) {
                let _ = write!(out, "  {text:>width$}");
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(
            out,
            "\nA cell has the tag's RSSI, the mean of the strongest per minute (instance) or\n\
             cycle (board), and the share of those that heard it. ~ is fair, ! poor."
        );
        let others = self.other_tags();
        if others > 0 {
            let _ = writeln!(
                out,
                "{}, without a discovery config.",
                plural(others, "other tag")
            );
        }
        out
    }

    /// The tags heard that have no discovery config, such as a neighbor's
    fn other_tags(&self) -> usize {
        let heard: BTreeSet<&String> = self
            .publishers
            .values()
            .flat_map(|p| p.tags.keys())
            .collect();
        heard
            .into_iter()
            .filter(|id| !self.names.contains_key(*id))
            .count()
    }
}

/// The coverage problems, worst first
fn problems(
    columns: &[(&String, &Publisher)],
    rows: &[(&String, &String)],
) -> Vec<(Grade, String)> {
    let mut problems = Vec::new();
    let mut unheard = Vec::new();
    let boards = columns.iter().any(|(_, p)| p.board);
    for (id, name) in rows {
        let heard: Vec<(&str, &Publisher, Grade)> = columns
            .iter()
            .filter_map(|(n, p)| Some((label(n), *p, p.cell(id)?.grade())))
            .collect();
        if heard.is_empty() {
            unheard.push(name.as_str());
            continue;
        }
        let well: Vec<&(&str, &Publisher, Grade)> =
            heard.iter().filter(|(_, _, g)| *g < Grade::Poor).collect();
        let problem = match well[..] {
            [] => (Grade::Poor, format!("{name}: no publisher hears it well")),
            _ if boards && !well.iter().any(|(_, p, _)| p.board) => (
                Grade::Fair,
                format!("{name}: only instances hear it well, a spot for a board"),
            ),
            [(only, _, _)] => (Grade::Fair, format!("{name}: only {only} hears it well")),
            _ => continue,
        };
        problems.push(problem);
    }
    if !unheard.is_empty() {
        problems.push((Grade::Poor, format!("not heard: {}", unheard.join(", "))));
    }

    let firmwares = count(columns.iter().filter_map(|(_, p)| p.firmware.as_deref()));
    let n: usize = firmwares.values().sum();
    // Without a majority, no board stands out
    let common = firmwares.iter().find(|(_, k)| **k * 2 > n).map(|(f, _)| *f);
    for (name, p) in columns.iter().filter(|(_, p)| p.board) {
        let name = label(name);
        if p.restarts > 0 {
            let times = match p.restarts {
                1 => "once".to_string(),
                n => format!("{n} times"),
            };
            problems.push((Grade::Poor, format!("{name}: restarted {times}")));
        }
        if let Some(rssi) = p.wifi_rssi.get() {
            let grade = Grade::wifi(rssi);
            if grade > Grade::Good {
                problems.push((grade, format!("{name}: Wi-Fi {rssi} dBm")));
            }
        }
        if let (Some(firmware), Some(common)) = (&p.firmware, common)
            && firmware != common
        {
            problems.push((
                Grade::Fair,
                format!("{name}: {firmware}, most boards {common}"),
            ));
        }
    }
    problems.sort_by_key(|(grade, _)| Reverse(*grade));
    problems
}

impl Publisher {
    fn add<'a>(&mut self, tags: impl Iterator<Item = (&'a String, Option<i8>)>) {
        self.periods += 1;
        for (id, rssi) in tags {
            let heard = self.tags.entry(id.clone()).or_default();
            heard.periods += 1;
            heard.rssi.extend(rssi.map(i64::from));
        }
    }

    fn board_cycle(&mut self, cycle: &diagnostics::Diagnostics) {
        // A QoS 1 message can arrive twice
        if self.cycle == Some(cycle.cycle) {
            return;
        }
        // The counter restarts on a cold boot or a crash
        if self.cycle.is_some_and(|last| cycle.cycle < last) {
            self.restarts += 1;
        }
        self.board = true;
        self.cycle = Some(cycle.cycle);
        self.firmware = Some(cycle.firmware.clone());
        self.wifi_rssi.extend(cycle.wifi_rssi.map(i64::from));
        self.add(cycle.tags.iter().map(|(id, (rssi, _))| (id, Some(*rssi))));
    }

    fn cell(&self, id: &str) -> Option<Cell> {
        let heard = self.tags.get(id)?;
        Some(Cell {
            rssi: heard.rssi.get(),
            heard: heard.periods * 100 / self.periods.max(1),
        })
    }
}

/// `!-93 40%`: the grade's mark, the RSSI, and the share heard
fn cell_text(cell: Cell) -> String {
    let rssi = cell.rssi.map_or("?".to_string(), |r| r.to_string());
    format!("{}{rssi} {}%", cell.grade().mark(), cell.heard)
}

/// A publisher's name without the `ruuvi2mqtt_` or `ruuvi2mqtt-` that most
/// have
fn label(name: &str) -> &str {
    name.strip_prefix("ruuvi2mqtt")
        .and_then(|rest| rest.strip_prefix(['_', '-']))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name)
}

/// `1 board`, `2 boards`
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{HOST1, HOST2, TAG, diagnostics, discovery, instance_diagnostics};

    const TAG2: &str = "aabbccddee02";
    const TAG3: &str = "aabbccddee03";
    const NEIGHBOR: &str = "aabbccddee09";

    fn site() -> Site {
        let mut site = Site::new("ruuvi2mqtt");
        for (id, name) in [(TAG, "Cellar"), (TAG2, "Attic"), (TAG3, "Sauna")] {
            site.on_message(
                &format!("homeassistant/sensor/ruuvi_{id}/temperature/config"),
                discovery(id, "temperature", name).as_bytes(),
            );
        }
        site
    }

    fn minute(site: &mut Site, host: &str, tags: &str) {
        site.on_message(
            &format!("ruuvi2mqtt/diagnostics/{host}"),
            instance_diagnostics(host, tags).as_bytes(),
        );
    }

    fn cycle(site: &mut Site, cycle: u64, tags: &str) {
        site.on_message(
            "ruuvi2mqtt/diagnostics/c6-board",
            diagnostics("1.5.2-dev (6534f11)", cycle, tags).as_bytes(),
        );
    }

    fn problem_lines(site: &Site) -> Vec<String> {
        let rows: Vec<_> = site.names.iter().collect();
        problems(&site.publishers.iter().collect::<Vec<_>>(), &rows)
            .into_iter()
            .map(|(grade, text)| format!("{}{text}", grade.mark()))
            .collect()
    }

    #[test]
    fn renders_the_table_and_the_problems() {
        let mut site = site();
        for n in 0..10 {
            let attic = if n < 4 {
                format!(r#","{TAG2}":{{"rssi":-93,"readings":1}}"#)
            } else {
                String::new()
            };
            minute(
                &mut site,
                HOST1,
                &format!(r#"{{"{TAG}":{{"rssi":-70,"readings":40}}{attic}}}"#),
            );
            minute(
                &mut site,
                HOST2,
                &format!(r#"{{"{TAG}":{{"rssi":null,"readings":30}}}}"#),
            );
        }
        for n in 1..=4 {
            cycle(
                &mut site,
                n,
                &format!(
                    r#"{{"{TAG}":{{"rssi":-81,"adverts":3}},"{NEIGHBOR}":{{"rssi":-95,"adverts":1}}}}"#
                ),
            );
        }
        // Sent again, then a restart
        cycle(&mut site, 4, "{}");
        cycle(&mut site, 1, "{}");
        assert_eq!(
            site.render(Duration::from_mins(10)),
            "\
Over 10:00, the diagnostics of 2 instances and 1 board.

Problems:
  ! Attic: no publisher hears it well
  ! not heard: Sauna
  ! c6-board: restarted once

           host1     host2  c6-board
        instance  instance     board
Attic   !-93 40%         -         -
Cellar  -70 100%    ? 100%  ~-81 80%
Sauna          -         -         -

A cell has the tag's RSSI, the mean of the strongest per minute (instance) or
cycle (board), and the share of those that heard it. ~ is fair, ! poor.
1 other tag, without a discovery config.
"
        );
    }

    #[test]
    fn tags_only_one_publisher_or_only_instances_hear_well() {
        let mut site = site();
        minute(
            &mut site,
            HOST1,
            &format!(r#"{{"{TAG}":{{"rssi":-70,"readings":40}}}}"#),
        );
        cycle(
            &mut site,
            1,
            &format!(r#"{{"{TAG2}":{{"rssi":-60,"adverts":9}}}}"#),
        );
        assert_eq!(
            problem_lines(&site),
            [
                "!not heard: Sauna",
                "~Cellar: only instances hear it well, a spot for a board",
                "~Attic: only c6-board hears it well",
            ]
        );
    }

    #[test]
    fn boards_on_weak_wifi_or_other_firmware() {
        let mut site = Site::new("ruuvi2mqtt");
        for (host, firmware, wifi) in [
            ("hall", "1.5.2-dev (6534f11)", -61),
            ("garage", "1.5.2-dev (6534f11)", -80),
            ("attic", "1.5.1", -70),
        ] {
            let payload = diagnostics(firmware, 1, "{}")
                .replace(r#""wifi_rssi":-61"#, &format!(r#""wifi_rssi":{wifi}"#));
            site.on_message(
                &format!("ruuvi2mqtt/diagnostics/{host}"),
                payload.as_bytes(),
            );
        }
        assert_eq!(
            problem_lines(&site),
            [
                "!garage: Wi-Fi -80 dBm",
                "~attic: Wi-Fi -70 dBm",
                "~attic: ruuvi2mqtt-esp32 1.5.1, most boards ruuvi2mqtt-esp32 1.5.2-dev (6534f11)",
            ]
        );
    }

    #[test]
    fn firmware_without_a_majority_is_no_problem() {
        let mut site = Site::new("ruuvi2mqtt");
        for (host, firmware) in [("hall", "1.5.2-dev (6534f11)"), ("attic", "1.5.1")] {
            site.on_message(
                &format!("ruuvi2mqtt/diagnostics/{host}"),
                diagnostics(firmware, 1, "{}").as_bytes(),
            );
        }
        assert_eq!(problem_lines(&site), Vec::<String>::new());
    }

    #[test]
    fn labels_drop_the_common_prefix() {
        assert_eq!(label("ruuvi2mqtt_host1"), "host1");
        assert_eq!(label("ruuvi2mqtt-kitchen"), "kitchen");
        assert_eq!(label("ruuvi2mqtt"), "ruuvi2mqtt");
        assert_eq!(label("kitchen"), "kitchen");
    }
}
