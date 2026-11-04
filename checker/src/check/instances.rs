//! The instances' diagnostics in the report. As for the boards, the
//! reception figures don't fail the check.

use std::fmt::Write as _;
use std::time::Duration;

use super::boards::{ratio, report_tag};
use super::{Report, Watch};
use crate::payload::Diagnostics;
use crate::time;

impl Watch {
    pub(super) fn check_instance_diagnostics(&self, r: &mut Report, id: &str, end: Duration) {
        let minutes = self.instance_minutes.get(id).map_or(&[][..], Vec::as_slice);
        let (Some(first), Some(last)) = (minutes.first(), minutes.last()) else {
            let _ = writeln!(r.text, "    no diagnostics");
            r.failures.push(format!("{id}: no diagnostics"));
            return;
        };
        let n = minutes.len();
        let per_minute = |f: fn(&Diagnostics) -> u32| {
            ratio(minutes.iter().map(|m| u64::from(f(m))).sum(), n as u64)
        };
        let _ = writeln!(
            r.text,
            "    {n} minutes of diagnostics, {}, {} MQTT reconnects, readings per minute: {:.1} published, {:.1} throttled",
            last.adapter,
            last.mqtt_reconnects.saturating_sub(first.mqtt_reconnects),
            per_minute(|m| m.readings_published),
            per_minute(|m| m.readings_throttled),
        );
        for tag in &self.config.tags {
            let seen: Vec<(Option<i8>, u32)> = minutes
                .iter()
                .filter_map(|m| m.tags.get(tag))
                .copied()
                .collect();
            report_tag(r, &self.label(tag), &seen, n, "minute", "readings");
        }
        let times: Vec<Duration> = minutes.iter().map(|m| m.at).collect();
        let gap = time::max_gap(&times, Duration::ZERO, end);
        if gap > self.config.max_gap {
            r.failures
                .push(format!("{id}: no diagnostics for {}", time::format(gap)));
        }
    }
}
