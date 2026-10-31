//! The ESP32 section of the report. The reception figures are only reported,
//! since good values are still unknown. With `--board`, the firmware and the
//! cycle are checked.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Duration;

use super::{Config, MAX_EXAMPLES, Report, Watch};
use crate::diagnostics::Diagnostics;
use crate::payload::check_version;
use crate::time;

impl Watch {
    pub(super) fn check_boards(&self, r: &mut Report, end: Duration) {
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
        let settings = count(cycles.iter().map(|cy| cy.settings.as_str()));
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

        let ms = |f: fn(&Diagnostics) -> u32| -> i64 {
            mean(&cycles.iter().map(|cy| i64::from(f(cy))).collect::<Vec<_>>())
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

        let caches: Vec<String> = count(cycles.iter().map(|cy| cy.wifi_cache.as_str()))
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect();
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
            let seen: Vec<(i8, u32)> = cycles
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
                let rssi: Vec<i64> = seen.iter().map(|(rssi, _)| i64::from(*rssi)).collect();
                let adverts: u64 = seen.iter().map(|(_, a)| u64::from(*a)).sum();
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
    let gap = time::max_gap(&times, Duration::ZERO, end);
    if gap > c.max_gap {
        r.failures
            .push(format!("{id}: no diagnostics for {}", time::format(gap)));
    }
    if let [first, .., last] = times[..] {
        let interval =
            last.saturating_sub(first) / u32::try_from(times.len() - 1).unwrap_or(u32::MAX);
        let low = c.board_cycle.saturating_mul(9) / 10;
        let high = c.board_cycle.saturating_mul(11) / 10;
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
            missed += u64::from(w[1].cycle - w[0].cycle - 1);
        } else {
            restarts += 1;
            missed += u64::from(w[1].cycle.saturating_sub(1));
        }
    }
    (missed, restarts)
}

fn count<'a>(values: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
}

fn sum_pairs(pairs: impl Iterator<Item = [u32; 2]>) -> [u64; 2] {
    pairs.fold([0, 0], |acc, p| {
        [acc[0] + u64::from(p[0]), acc[1] + u64::from(p[1])]
    })
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
        format!("{:.1}", ratio(n, ms) * 1000.0)
    }
}

/// The rounded mean, 0 for none
fn mean(values: &[i64]) -> i64 {
    let n = i64::try_from(values.len()).unwrap_or(i64::MAX).max(1);
    let sum: i64 = values.iter().sum();
    (sum + sum.signum() * n / 2) / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_time_has_no_rate() {
        assert_eq!(per_second(0, 0), "-");
    }
}
