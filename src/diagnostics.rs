use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;
use tokio::time::{Instant, MissedTickBehavior, interval_at};

use crate::Event::PublishDiagnostics;
use crate::EventSender;
use crate::config;
use crate::ruuvi::BDAddr;

const INTERVAL: Duration = Duration::from_mins(1);

/// What the instance counts for its diagnostics. The per-minute counts start
/// over with each message.
pub struct Diagnostics {
    started: Instant,
    adapter: String,
    mqtt_connects: u32,
    minute: Minute,
}

#[derive(Default)]
struct Minute {
    tags: BTreeMap<String, TagStats>,
    readings_published: u32,
    readings_throttled: u32,
}

/// One tag's readings in a minute.
#[derive(Debug, Serialize)]
struct TagStats {
    /// The strongest, in dBm
    rssi: Option<i16>,
    readings: u32,
}

/// The message on `<base_topic>/diagnostics/<client_id>`. The fields that
/// the ESP32 boards' diagnostics also have keep their names.
///
/// The test bed's checker parses it (`checker/src/payload.rs`), so update
/// that too.
#[derive(Debug, Serialize)]
struct Message<'a> {
    version: &'static str,
    client_id: &'a str,
    adapter: &'a str,
    uptime_s: u64,
    mqtt_reconnects: u32,
    readings_published: u32,
    readings_throttled: u32,
    tags: &'a BTreeMap<String, TagStats>,
}

impl Diagnostics {
    pub fn new(adapter: String) -> Self {
        Self {
            started: Instant::now(),
            adapter,
            mqtt_connects: 0,
            minute: Minute::default(),
        }
    }

    pub fn reading(&mut self, bdaddr: BDAddr, rssi: Option<i16>) {
        let tag = self
            .minute
            .tags
            .entry(bdaddr.to_string_no_delim())
            .or_insert(TagStats {
                rssi: None,
                readings: 0,
            });
        tag.rssi = tag.rssi.max(rssi);
        tag.readings += 1;
    }

    pub fn published(&mut self) {
        self.minute.readings_published += 1;
    }

    pub fn throttled(&mut self) {
        self.minute.readings_throttled += 1;
    }

    pub fn mqtt_connected(&mut self) {
        self.mqtt_connects += 1;
    }

    /// The message, as JSON
    pub fn take(&mut self, client_id: &str) -> Vec<u8> {
        let minute = std::mem::take(&mut self.minute);
        let message = Message {
            version: config::VERSION,
            client_id,
            adapter: &self.adapter,
            uptime_s: self.started.elapsed().as_secs(),
            mqtt_reconnects: self.mqtt_connects.saturating_sub(1),
            readings_published: minute.readings_published,
            readings_throttled: minute.readings_throttled,
            tags: &minute.tags,
        };
        serde_json::to_vec(&message).expect("diagnostics serialize to JSON")
    }
}

/// Asks the main loop for the diagnostics once a minute.
pub async fn tick(tx: EventSender) {
    let mut interval = interval_at(Instant::now() + INTERVAL, INTERVAL);
    // A burst after a stall would send nearly empty minutes.
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if tx.send(PublishDiagnostics).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: [u8; 6] = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01];

    #[test]
    fn json() {
        let mut d = Diagnostics::new("hci0: Example BT Adapter (1234:5678)".into());
        d.mqtt_connected();
        d.mqtt_connected();
        d.reading(BDAddr::from(TAG), Some(-80));
        d.reading(BDAddr::from(TAG), None);
        d.reading(BDAddr::from(TAG), Some(-72));
        d.published();
        d.throttled();
        d.throttled();
        let json = String::from_utf8(d.take("ruuvi2mqtt_host1")).unwrap();
        assert_eq!(
            json,
            format!(
                concat!(
                    r#"{{"version":"{}","client_id":"ruuvi2mqtt_host1","#,
                    r#""adapter":"hci0: Example BT Adapter (1234:5678)","uptime_s":0,"#,
                    r#""mqtt_reconnects":1,"readings_published":1,"readings_throttled":2,"#,
                    r#""tags":{{"aabbccddee01":{{"rssi":-72,"readings":3}}}}}}"#
                ),
                config::VERSION
            )
        );
    }

    #[test]
    fn each_minute_starts_over() {
        let mut d = Diagnostics::new("hci0".into());
        d.mqtt_connected();
        d.reading(BDAddr::from(TAG), None);
        d.published();
        d.take("host1");
        let json = String::from_utf8(d.take("host1")).unwrap();
        assert!(
            json.ends_with(
                r#""mqtt_reconnects":0,"readings_published":0,"readings_throttled":0,"tags":{}}"#
            ),
            "{json}"
        );
    }

    #[test]
    fn rssi_unknown_until_reported() {
        let mut d = Diagnostics::new("hci0".into());
        d.reading(BDAddr::from(TAG), None);
        let json = String::from_utf8(d.take("host1")).unwrap();
        assert!(
            json.ends_with(r#""tags":{"aabbccddee01":{"rssi":null,"readings":1}}}"#),
            "{json}"
        );
    }
}
