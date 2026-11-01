//! The topics that the app and the ESP32 boards publish, for both the check
//! and the recorder.

use crate::payload::{AIR_ENTITIES, TAG_ENTITIES};

#[derive(Debug, PartialEq)]
pub enum Topic<'a> {
    Status(&'a str),
    Diagnostics(&'a str),
    Reading(&'a str),
    Discovery {
        id: &'a str,
        entity: &'static str,
    },
    /// `binary_sensor/<id>/status/config`, maybe an instance's
    StatusDiscovery,
    Other,
}

impl<'a> Topic<'a> {
    pub fn parse(base_topic: &str, topic: &'a str) -> Self {
        if let Some(rest) = topic
            .strip_prefix(base_topic)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            if let Some(client_id) = rest.strip_prefix("status/") {
                return name(client_id).map_or(Self::Other, Self::Status);
            }
            if let Some(hostname) = rest.strip_prefix("diagnostics/") {
                return name(hostname).map_or(Self::Other, Self::Diagnostics);
            }
            if is_tag_id(rest) {
                return Self::Reading(rest);
            }
        } else if let Some(rest) = topic.strip_prefix("homeassistant/") {
            if let Some((id, entity)) = parse_discovery(rest) {
                return Self::Discovery { id, entity };
            }
            if let ["binary_sensor", _, "status", "config"] =
                rest.split('/').collect::<Vec<_>>()[..]
            {
                return Self::StatusDiscovery;
            }
        }
        Self::Other
    }
}

/// A client ID or hostname in a topic. One with control characters would
/// break the report's lines.
fn name(s: &str) -> Option<&str> {
    (!s.is_empty() && !s.contains('/') && !s.contains(char::is_control)).then_some(s)
}

/// Returns the tag's ID and the entity from
/// `<component>/ruuvi_<id>/<entity>/config`.
fn parse_discovery(rest: &str) -> Option<(&str, &'static str)> {
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
pub fn is_tag_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(topic: &str) -> Topic<'_> {
        Topic::parse("ruuvi2mqtt", topic)
    }

    #[test]
    fn parses_topics() {
        assert_eq!(parse("ruuvi2mqtt/status/host1"), Topic::Status("host1"));
        assert_eq!(
            parse("ruuvi2mqtt/diagnostics/c6-board"),
            Topic::Diagnostics("c6-board")
        );
        assert_eq!(
            parse("ruuvi2mqtt/aabbccddee01"),
            Topic::Reading("aabbccddee01")
        );
        assert_eq!(
            parse("homeassistant/binary_sensor/ruuvi_aabbccddee01/battery_low/config"),
            Topic::Discovery {
                id: "aabbccddee01",
                entity: "battery_low"
            }
        );
        assert_eq!(
            parse("homeassistant/binary_sensor/ruuvi2mqtt_host1/status/config"),
            Topic::StatusDiscovery
        );
    }

    #[test]
    fn other_topics() {
        for topic in [
            "ruuvi2mqtt/AABBCCDDEE01",
            "ruuvi2mqtt/status/host1\nok: all good",
            "ruuvi2mqtt/diagnostics/\u{1b}[2J",
            "homeassistant/sensor/ruuvi_aabbccddee01/battery_low/config",
            "zigbee2mqtt/bridge/state",
        ] {
            assert_eq!(parse(topic), Topic::Other, "{topic}");
        }
    }
}
