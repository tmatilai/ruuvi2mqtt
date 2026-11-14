//! Each publisher's diagnostics over the last day, in 10-minute buckets, for
//! the status page. The recorder keeps it in its state file.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value, json};

use crate::diagnostics::{Battery, parse_battery};
use crate::payload::printable;
use crate::site::{Heard, Period, Publisher};
use crate::stats::Mean;

pub const BUCKET: u64 = 600;
pub const DAY: u64 = 24 * 3600;

#[derive(Default)]
pub struct History {
    /// By the name in their diagnostics topic
    entries: BTreeMap<String, Entry>,
}

#[derive(Default)]
pub struct Entry {
    pub client_id: String,
    /// An instance's Bluetooth adapter, from its last minute
    pub adapter: Option<String>,
    /// A board's battery, from its last cycle. `None` also when the read failed.
    pub battery: Option<Battery>,
    /// When its last diagnostics came, in Unix seconds
    pub last: u64,
    /// By the bucket's start, oldest first
    buckets: VecDeque<(u64, Publisher)>,
}

impl History {
    pub fn on_diagnostics(&mut self, now: u64, name: &str, payload: &[u8]) {
        let Some(period) = Period::parse(name, payload) else {
            return;
        };
        let entry = self.entries.entry(printable(name)).or_default();
        entry.client_id = period.client_id().to_string();
        match &period {
            Period::Instance(minute) => entry.adapter = Some(minute.adapter.clone()),
            Period::Board(cycle) => entry.battery = cycle.battery.flatten(),
        }
        entry.last = now;
        let start = now - now % BUCKET;
        if entry.buckets.back().is_none_or(|(s, _)| *s != start) {
            // The cycle and firmware carry over, so that a restart across
            // buckets counts
            let previous = entry.buckets.back().map(|(_, p)| p);
            let bucket = Publisher {
                board: previous.is_some_and(|p| p.board),
                firmware: previous.and_then(|p| p.firmware.clone()),
                cycle: previous.and_then(|p| p.cycle),
                ..Publisher::default()
            };
            entry.buckets.push_back((start, bucket));
        }
        if let Some((_, bucket)) = entry.buckets.back_mut() {
            bucket.add_period(&period);
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter()
    }

    /// Each publisher's buckets that overlap the last `secs`, merged
    pub fn window(&self, now: u64, secs: u64) -> BTreeMap<String, Publisher> {
        let from = now.saturating_sub(secs);
        self.entries
            .iter()
            .filter_map(|(name, entry)| {
                let mut buckets = entry
                    .buckets
                    .iter()
                    .filter(|(start, _)| start + BUCKET > from);
                let mut merged = buckets.next()?.1.clone();
                for (_, bucket) in buckets {
                    merged.merge(bucket);
                }
                Some((name.clone(), merged))
            })
            .collect()
    }

    /// Drops the buckets older than a day, and the publishers without any
    pub fn prune(&mut self, now: u64) {
        let from = now.saturating_sub(DAY);
        self.entries.retain(|_, entry| {
            while entry
                .buckets
                .front()
                .is_some_and(|(start, _)| start + BUCKET <= from)
            {
                entry.buckets.pop_front();
            }
            !entry.buckets.is_empty()
        });
    }

    pub fn state(&self) -> Value {
        let entries: Map<String, Value> = self
            .entries
            .iter()
            .map(|(name, e)| {
                let buckets: Vec<Value> = e.buckets.iter().map(bucket_state).collect();
                let entry = json!({
                    "client_id": e.client_id,
                    "adapter": e.adapter,
                    "battery": e.battery.map(|b| json!({
                        "mv": b.mv,
                        "level": b.level,
                        "rate": b.rate,
                        "usb": b.usb,
                    })),
                    "last": e.last,
                    "buckets": buckets,
                });
                (name.clone(), entry)
            })
            .collect();
        Value::Object(entries)
    }

    /// From the recorder's state. A state without it starts empty.
    pub fn load(state: &Value) -> Self {
        let entries = state
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, e)| {
                let entry = Entry {
                    client_id: e["client_id"].as_str().map(printable).unwrap_or_default(),
                    adapter: e["adapter"].as_str().map(printable),
                    battery: parse_battery(e).ok().flatten().flatten(),
                    last: e["last"].as_u64().unwrap_or(0),
                    buckets: e["buckets"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(load_bucket)
                        .collect(),
                };
                (printable(name), entry)
            })
            .collect();
        Self { entries }
    }
}

/// `{"start", "board", "periods", ..., "tags": {"<id>": [periods, rssi sum,
/// rssi count]}}`
fn bucket_state((start, p): &(u64, Publisher)) -> Value {
    let tags: Map<String, Value> = p
        .tags
        .iter()
        .map(|(id, heard)| {
            let [sum, n] = heard.rssi.parts();
            (id.clone(), json!([heard.periods, sum, n]))
        })
        .collect();
    json!({
        "start": start,
        "board": p.board,
        "periods": p.periods,
        "firmware": p.firmware,
        "cycle": p.cycle,
        "restarts": p.restarts,
        "wifi_rssi": p.wifi_rssi.parts(),
        "tags": tags,
    })
}

fn load_bucket(b: &Value) -> Option<(u64, Publisher)> {
    let pair = |v: &Value| Some([v[0].as_i64()?, v[1].as_i64()?]);
    let tags = b["tags"]
        .as_object()?
        .iter()
        .filter_map(|(id, t)| {
            let heard = Heard {
                periods: t[0].as_u64()?,
                rssi: Mean::from_parts([t[1].as_i64()?, t[2].as_i64()?]),
            };
            Some((id.clone(), heard))
        })
        .collect();
    let publisher = Publisher {
        board: b["board"].as_bool()?,
        periods: b["periods"].as_u64()?,
        tags,
        firmware: b["firmware"].as_str().map(printable),
        cycle: b["cycle"].as_u64().and_then(|c| u32::try_from(c).ok()),
        restarts: b["restarts"].as_u64()?,
        wifi_rssi: Mean::from_parts(pair(&b["wifi_rssi"])?),
    };
    Some((b["start"].as_u64()?, publisher))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{HOST1, TAG, diagnostics, instance_diagnostics};

    /// A bucket's start
    const T0: u64 = 1_800_000_000 - 1_800_000_000 % BUCKET;

    fn minute(h: &mut History, at: u64, rssi: i64) {
        let tags = format!(r#"{{"{TAG}":{{"rssi":{rssi},"readings":1}}}}"#);
        h.on_diagnostics(at, HOST1, instance_diagnostics(HOST1, &tags).as_bytes());
    }

    #[test]
    fn windows_merge_the_buckets() {
        let mut h = History::default();
        // One minute heard in each bucket, one not in the last
        for (at, rssi) in [(T0 + 60, -70), (T0 + BUCKET, -80), (T0 + 2 * BUCKET, -90)] {
            minute(&mut h, at, rssi);
        }
        h.on_diagnostics(
            T0 + 2 * BUCKET + 60,
            HOST1,
            instance_diagnostics(HOST1, "{}").as_bytes(),
        );
        let now = T0 + 2 * BUCKET + 120;
        let all = &h.window(now, 3600)[HOST1];
        assert_eq!(all.periods, 4);
        let cell = all.cell(TAG).unwrap();
        assert_eq!((cell.rssi, cell.heard), (Some(-80), 75));
        // Only the last bucket overlaps the last minute
        let last = &h.window(now, 60)[HOST1];
        assert_eq!(last.periods, 2);
        assert_eq!(last.cell(TAG).unwrap().heard, 50);
        let (_, entry) = h.entries().next().unwrap();
        assert_eq!(entry.client_id, HOST1);
        assert_eq!(entry.last, T0 + 2 * BUCKET + 60);
        assert_eq!(
            entry.adapter.as_deref(),
            Some("hci0: Example BT Adapter (1234:5678)")
        );
    }

    #[test]
    fn a_restart_across_buckets_counts() {
        let mut h = History::default();
        for (at, cycle) in [(T0, 5), (T0 + 60, 6), (T0 + BUCKET, 1)] {
            h.on_diagnostics(at, "c6", diagnostics("1.5.2", cycle, "{}").as_bytes());
        }
        let p = &h.window(T0 + BUCKET, 3600)["c6"];
        assert!(p.board);
        assert_eq!(p.restarts, 1);
        assert_eq!(p.periods, 3);
        assert_eq!(p.firmware.as_deref(), Some("ruuvi2mqtt-esp32 1.5.2"));
    }

    #[test]
    fn prunes_after_a_day() {
        let mut h = History::default();
        minute(&mut h, T0, -70);
        minute(&mut h, T0 + BUCKET, -70);
        h.prune(T0 + DAY + BUCKET);
        assert_eq!(h.window(T0 + DAY + BUCKET, DAY)[HOST1].periods, 1);
        h.prune(T0 + DAY + 2 * BUCKET);
        assert_eq!(h.entries().count(), 0);
    }

    #[test]
    fn loads_its_state() {
        let mut h = History::default();
        minute(&mut h, T0 + 60, -71);
        let battery = diagnostics("1.5.2", 3, "{}").replace(
            r#""free_heap""#,
            r#""battery":{"mv":3950,"level":87.5,"rate":-1.5,"usb":false},"free_heap""#,
        );
        h.on_diagnostics(T0 + 90, "c6", battery.as_bytes());
        let state = h.state();
        let loaded = History::load(&state);
        assert_eq!(loaded.state(), state);
        let now = T0 + 120;
        let cell = loaded.window(now, 3600)[HOST1].cell(TAG).unwrap();
        assert_eq!((cell.rssi, cell.heard), (Some(-71), 100));
        let (_, c6) = loaded.entries().find(|(name, _)| *name == "c6").unwrap();
        assert_eq!(c6.battery.map(|b| (b.mv, b.usb)), Some((3950, Some(false))));
        assert_eq!(History::load(&Value::Null).entries().count(), 0);
    }
}
