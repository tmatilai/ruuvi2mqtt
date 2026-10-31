//! The ESP32 boards' diagnostics. They mirror
//! `ruuvi2mqtt-esp32/src/diagnostics.rs`; update them when that changes.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value;

use crate::payload::{client_id, printable};
use crate::topic::is_tag_id;

/// One cycle's diagnostics from an ESP32 board. The times are in ms from the
/// start of the board's cycle.
pub struct Diagnostics {
    /// When the checker got them
    pub at: Duration,
    /// From the topic
    pub hostname: String,
    /// The `publisher` of the board's readings
    pub client_id: String,
    pub firmware: String,
    pub chip: String,
    /// The board's cycle counter, which restarts on a cold boot or crash
    pub cycle: u32,
    pub settings: String,
    pub wifi_cache: String,
    pub scan_start_ms: u32,
    pub scan_end_ms: u32,
    pub wifi_ms: u32,
    pub mqtt_ms: u32,
    pub cycle_ms: u32,
    /// Tag ID to (the strongest RSSI, advertisements)
    pub tags: BTreeMap<String, (i8, u32)>,
    /// Ruuvi advertisements before and after Wi-Fi connected
    pub ruuvi_adverts: [u32; 2],
    /// Other advertisements, the same way
    pub other_adverts: [u32; 2],
    pub error: Option<String>,
}

impl Diagnostics {
    /// The scan time in ms before and after Wi-Fi connected
    pub fn scan_windows(&self) -> [u32; 2] {
        let split = self.wifi_ms.clamp(self.scan_start_ms, self.scan_end_ms);
        [split - self.scan_start_ms, self.scan_end_ms - split]
    }
}

pub fn parse(at: Duration, hostname: &str, payload: &[u8]) -> Result<Diagnostics, String> {
    let d: Value = serde_json::from_slice(payload).map_err(|err| format!("not JSON: {err}"))?;
    let string = |key: &str| match &d[key] {
        Value::String(s) if !s.is_empty() => Ok(printable(s)),
        other => Err(format!("{key}: {other}")),
    };
    let number = |key: &str| number(&d, key);
    let pair = |key: &str| {
        Ok::<_, String>([
            number(&format!("{key}_before_wifi"))?,
            number(&format!("{key}_after_wifi"))?,
        ])
    };
    let diagnostics = Diagnostics {
        at,
        hostname: hostname.to_string(),
        client_id: client_id(&d["client_id"])
            .ok_or_else(|| format!("client_id: {}", d["client_id"]))?
            .to_string(),
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
        tags: parse_tags(&d["tags"])?,
        ruuvi_adverts: pair("adverts")?,
        other_adverts: pair("other_adverts")?,
        error: d["error"].as_str().map(printable),
    };
    if diagnostics.scan_start_ms > diagnostics.scan_end_ms {
        return Err(format!(
            "scan_start_ms {} after scan_end_ms {}",
            diagnostics.scan_start_ms, diagnostics.scan_end_ms
        ));
    }
    Ok(diagnostics)
}

/// A `u32`, as the firmware sends
fn number(v: &Value, key: &str) -> Result<u32, String> {
    v[key]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("{key}: {}", v[key]))
}

/// `deep-sleep 5s`, with `, external antenna` if set
fn parse_settings(s: &Value) -> Result<String, String> {
    let mode = s["scan_mode"]
        .as_str()
        .ok_or_else(|| format!("settings: {s}"))?;
    let mut settings = format!("{} {}s", printable(mode), number(s, "scan_duration")?);
    if let Some(antenna) = s["antenna"].as_str() {
        let _ = write!(settings, ", {} antenna", printable(antenna));
    }
    Ok(settings)
}

fn parse_tags(tags: &Value) -> Result<BTreeMap<String, (i8, u32)>, String> {
    let Value::Object(tags) = tags else {
        return Err(format!("tags: {tags}"));
    };
    tags.iter()
        .map(|(id, tag)| {
            let rssi = tag["rssi"].as_i64().and_then(|r| i8::try_from(r).ok());
            let adverts = tag["adverts"].as_u64().and_then(|a| u32::try_from(a).ok());
            match (rssi, adverts) {
                (Some(rssi), Some(adverts)) if is_tag_id(id) => Ok((id.clone(), (rssi, adverts))),
                _ => Err(format!("tag {}: {tag}", printable(id))),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::diagnostics;

    #[test]
    fn scan_ending_before_wifi_has_no_time_after_wifi() {
        let payload = diagnostics("1.5.2-dev", 1, "{}")
            .replace(
                r#""deep-sleep","scan_duration":5"#,
                r#""deep-sleep","scan_duration":8,"antenna":"external""#,
            )
            .replace(r#""wifi_ms":1200"#, r#""wifi_ms":9500"#);
        let d = parse(Duration::ZERO, "c6", payload.as_bytes()).unwrap();
        assert_eq!(d.settings, "deep-sleep 8s, external antenna");
        assert_eq!(d.scan_windows(), [5000, 0]);
    }

    #[test]
    fn rejects_values_the_firmware_cant_send() {
        let parse = |payload: String| parse(Duration::ZERO, "c6", payload.as_bytes()).err();
        assert_eq!(
            parse(diagnostics("1.5.2-dev", 1, "{}").replace(r#""cycle":1"#, r#""cycle":-1"#)),
            Some("cycle: -1".into())
        );
        assert_eq!(
            parse(diagnostics(
                "1.5.2-dev",
                1,
                r#"{"aabbccddee01":{"rssi":-300,"adverts":2}}"#
            )),
            Some(r#"tag aabbccddee01: {"adverts":2,"rssi":-300}"#.into())
        );
        assert_eq!(
            parse(diagnostics(
                "1.5.2-dev",
                1,
                r#"{"x\n":{"rssi":-70,"adverts":2}}"#
            )),
            Some(r#"tag x\n: {"adverts":2,"rssi":-70}"#.into())
        );
        assert_eq!(
            parse(diagnostics("1.5.2-dev", 1, "{}").replace("ruuvi2mqtt-c6", r"c6\u001b")),
            Some(r#"client_id: "c6\u001b""#.into())
        );
    }
}
