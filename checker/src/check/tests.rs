use super::*;
use crate::fixtures::{
    AIR, BOARD, COMMIT, HOST1, HOST2, TAG, air_reading, air_reading_e1, diagnostics,
    instance_discovery, online, reading,
};
use crate::payload::{AIR_ENTITIES, TAG_ENTITIES};

fn watch() -> Watch {
    Watch::new(Config {
        base_topic: "ruuvi2mqtt".into(),
        commit: COMMIT.into(),
        instances: vec![HOST1.into(), HOST2.into()],
        boards: Vec::new(),
        board_cycle: Duration::from_secs(67),
        tags: vec![TAG.into()],
        airs: Vec::new(),
        gaps: BTreeMap::new(),
        first_reading: Duration::from_mins(5),
        max_gap: Duration::from_mins(10),
        throttle: Duration::from_mins(1),
        max_duplicates: 10,
    })
}

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

/// Statuses and discovery for both instances, discovery for TAG, and a
/// reading every minute for `minutes`, the first `duplicates` of them twice
fn healthy_with(watch: &mut Watch, minutes: u64, duplicates: u64) {
    for instance in [HOST1, HOST2] {
        let topic = format!("ruuvi2mqtt/status/{instance}");
        watch.on_message(
            secs(0),
            &topic,
            online("1.5.2-dev (6534f11)").as_bytes(),
            true,
        );
        watch.on_message(
            secs(0),
            &instance_discovery_topic(instance),
            instance_discovery(instance).as_bytes(),
            true,
        );
    }
    discovery(watch, TAG, "Cellar", &TAG_ENTITIES);
    let topic = format!("ruuvi2mqtt/{TAG}");
    for m in 0..minutes {
        watch.on_message(secs(30 + m * 60), &topic, reading(HOST2).as_bytes(), false);
        if m < duplicates {
            watch.on_message(secs(31 + m * 60), &topic, reading(HOST1).as_bytes(), false);
        }
    }
}

fn discovery(watch: &mut Watch, id: &str, name: &str, entities: &[(&str, &str)]) {
    for (component, entity) in entities {
        let topic = format!("homeassistant/{component}/ruuvi_{id}/{entity}/config");
        let config = crate::fixtures::discovery(id, entity, name);
        watch.on_message(secs(0), &topic, config.as_bytes(), true);
    }
}

fn instance_discovery_topic(client_id: &str) -> String {
    format!("homeassistant/binary_sensor/{client_id}/status/config")
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
    assert!(report.text.contains("Cellar (aabbccddee01): 15 readings"));
}

#[test]
fn wrong_or_dirty_version_fails() {
    for version in ["1.5.2-dev (abcdef0)", "1.5.2-dev (6534f11-dirty)", "1.5.1"] {
        let mut w = watch();
        healthy(&mut w, 15);
        w.on_message(
            secs(0),
            "ruuvi2mqtt/status/ruuvi2mqtt_host1",
            online(version).as_bytes(),
            true,
        );
        let report = w.finish(Duration::from_mins(15));
        assert_eq!(report.failures.len(), 1, "{version}: {}", report.text);
        assert!(report.summary.starts_with("FAIL: ruuvi2mqtt_host1: "));
    }
}

#[test]
fn instance_without_discovery_fails() {
    let mut w = watch();
    healthy(&mut w, 15);
    w.on_message(secs(60), &instance_discovery_topic(HOST2), b"", true);
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.failures, ["ruuvi2mqtt_host2: no discovery config"]);
}

#[test]
fn status_change_fails() {
    let mut w = watch();
    healthy(&mut w, 15);
    let topic = "ruuvi2mqtt/status/ruuvi2mqtt_host2";
    let note = w.on_message(secs(300), topic, br#"{"state":"offline"}"#, false);
    assert_eq!(
        note.as_deref(),
        Some(r#"+5:00 ruuvi2mqtt_host2: {"state":"offline"}"#)
    );
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(
        report.summary,
        "FAIL: ruuvi2mqtt_host2: the status changed 1 times"
    );
}

#[test]
fn retained_status_again_is_no_change() {
    let mut w = watch();
    healthy(&mut w, 15);
    // After the checker's own reconnect
    let topic = "ruuvi2mqtt/status/ruuvi2mqtt_host2";
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
    w.statuses.remove(HOST1);
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.summary, "FAIL: ruuvi2mqtt_host1: no status");
}

#[test]
fn missing_tag_fails() {
    let mut w = watch();
    healthy(&mut w, 0);
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.summary, "FAIL: Cellar (aabbccddee01): no readings");
}

#[test]
fn late_first_reading_and_gap_fail() {
    let mut w = watch();
    healthy(&mut w, 0);
    w.on_message(
        secs(400),
        &format!("ruuvi2mqtt/{TAG}"),
        reading(HOST1).as_bytes(),
        false,
    );
    let report = w.finish(Duration::from_mins(20));
    assert_eq!(
        report.failures,
        [
            "Cellar (aabbccddee01): first reading only at +6:40",
            "Cellar (aabbccddee01): no reading for 13:20",
        ]
    );
}

#[test]
fn gap_before_the_watch_extends_the_limits() {
    // Readings at 11:40 and 25:00: too late and too far apart by default
    let with_gap = |gap: Option<u64>, end: u64| {
        let mut w = watch();
        if let Some(gap) = gap {
            w.config.gaps.insert(TAG.into(), secs(gap));
        }
        healthy(&mut w, 0);
        let topic = format!("ruuvi2mqtt/{TAG}");
        for at in [700, 1500] {
            w.on_message(secs(at), &topic, reading(HOST1).as_bytes(), false);
        }
        w.finish(secs(end))
    };
    let report = with_gap(None, 1500);
    assert_eq!(
        report.failures,
        [
            "Cellar (aabbccddee01): first reading only at +11:40",
            "Cellar (aabbccddee01): no reading for 13:20",
        ]
    );
    // A gap of 6:40 before allows twice that
    let report = with_gap(Some(400), 1500);
    assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
    assert!(
        report
            .text
            .contains(", first +11:40, max gap 13:20 (6:40 before), 0 duplicates,"),
        "{}",
        report.text
    );
    let report = with_gap(Some(400), 2301);
    assert_eq!(
        report.summary,
        "FAIL: Cellar (aabbccddee01): no reading for 13:21"
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
        "FAIL: Cellar (aabbccddee01): 2 of 17 readings within the throttle"
    );
}

#[test]
fn invalid_reading_fails() {
    let mut w = watch();
    healthy(&mut w, 15);
    let topic = format!("ruuvi2mqtt/{TAG}");
    let bad = reading(HOST2).replace("45.5", "145.5");
    let note = w.on_message(secs(100), &topic, bad.as_bytes(), false);
    assert_eq!(
        note.as_deref(),
        Some("+1:40 aabbccddee01: humidity 145.5 out of range")
    );
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.summary, "FAIL: 1 invalid readings");
}

#[test]
fn healthy_air_passes() {
    let mut w = watch();
    w.config.tags.push(AIR.into());
    w.config.airs.push(AIR.into());
    healthy(&mut w, 15);
    discovery(&mut w, AIR, "Living room", &AIR_ENTITIES);
    let topic = format!("ruuvi2mqtt/{AIR}");
    // E1 from one instance, format 6 only from the other
    for m in 0..15 {
        let reading = if m < 10 {
            air_reading_e1(HOST2)
        } else {
            air_reading(HOST1)
        };
        w.on_message(secs(40 + m * 60), &topic, reading.as_bytes(), false);
    }
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
    assert!(
        report
            .text
            .contains("Living room (c4d2a1b3e5f6): 15 readings")
    );
    assert!(report.text.contains(
        ", from ruuvi2mqtt_host1 5 (0 E1), ruuvi2mqtt_host2 10 (10 E1), discovery 13/13\n"
    ));
    assert!(
        report
            .text
            .contains(", from ruuvi2mqtt_host2 15, discovery 6/6\n")
    );
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
        "FAIL: Cellar (aabbccddee01): 5 of 6 discovery configs"
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
        "FAIL: Cellar (aabbccddee01): unexpected discovery configs: co2"
    );
}

#[test]
fn other_tags_and_topics_are_reported() {
    let mut w = watch();
    healthy(&mut w, 15);
    w.on_message(
        secs(100),
        "ruuvi2mqtt/aabbccddee02",
        reading(HOST1).as_bytes(),
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
            .contains("\nOther tags:\n  aabbccddee02: 1 readings\n")
    );
    assert!(report.text.contains("\nOther statuses:\n  conn-plain: "));
    assert!(report.text.contains("\n1 messages on other topics\n"));
}

/// `minutes` of healthy instances, and a board cycle every 67 s, each
/// with a reading of TAG
fn with_board(watch: &mut Watch, minutes: u64, firmware: &str) {
    with_board_in(watch, minutes, firmware, "deep-sleep");
}

fn with_board_in(watch: &mut Watch, minutes: u64, firmware: &str, mode: &str) {
    healthy(watch, minutes);
    let topic = format!("ruuvi2mqtt/{TAG}");
    let tags = format!(r#"{{"{TAG}":{{"rssi":-74,"adverts":2,"first_ms":1210}}}}"#);
    for n in 0..minutes * 60 / 67 {
        let at = 31 + n * 67;
        watch.on_message(secs(at), &topic, reading(BOARD).as_bytes(), false);
        let payload = diagnostics(firmware, 10 + n, &tags).replace("deep-sleep", mode);
        watch.on_message(
            secs(at),
            "ruuvi2mqtt/diagnostics/c6-board",
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
        "  ruuvi2mqtt-c6 (c6-board, esp32c6): 13 cycles, ruuvi2mqtt-esp32 1.5.2-dev (abcdef0)\n",
        "    settings: deep-sleep 5s\n",
        "    hit rate 100% (13 of 13 tag cycles)\n",
        "    Cellar (aabbccddee01): 13 of 13 cycles, RSSI -74 (-74..-74), 2.0 adverts per cycle heard\n",
        "    adverts/s before Wi-Fi connected: Ruuvi 1.0, other 10.0; after: Ruuvi 2.0, other 20.0\n",
        "    mean ms: Wi-Fi 1200, MQTT 2000, scan 200..5200, cycle 5300 (max 5300)\n",
        "    Wi-Fi cache 13 hit; cycle counter: 0 missed, 0 restarts\n",
    )), "{}", report.text);
    assert!(
        report
            .text
            .contains("from ruuvi2mqtt-c6 13, ruuvi2mqtt_host2 15,")
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

/// A checked board that scans continuously, with its retained status
fn continuous_board(minutes: u64) -> Watch {
    let mut w = watch_board();
    let status = online("1.5.2-dev (6534f11)");
    w.on_message(
        secs(0),
        "ruuvi2mqtt/status/ruuvi2mqtt-c6",
        status.as_bytes(),
        true,
    );
    with_board_in(&mut w, minutes, "1.5.2-dev (6534f11)", "continuous");
    w
}

#[test]
fn continuous_board_with_status_passes() {
    let report = continuous_board(15).finish(Duration::from_mins(15));
    assert_eq!(report.failures, Vec::<String>::new(), "{}", report.text);
    assert!(
        report.text.contains(concat!(
            "    status: {\"state\":\"online\",\"version\":\"1.5.2-dev (6534f11)\"}\n",
            "    settings: continuous 5s\n",
        )),
        "{}",
        report.text
    );
    assert!(!report.text.contains("Other statuses"), "{}", report.text);
}

#[test]
fn continuous_board_without_status_fails() {
    let mut w = continuous_board(15);
    w.statuses.remove(BOARD);
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.summary, "FAIL: ruuvi2mqtt-c6: no status");
}

#[test]
fn continuous_board_going_offline_fails() {
    let mut w = continuous_board(15);
    let topic = "ruuvi2mqtt/status/ruuvi2mqtt-c6";
    let note = w.on_message(secs(300), topic, br#"{"state":"offline"}"#, true);
    assert_eq!(
        note.as_deref(),
        Some(r#"+5:00 ruuvi2mqtt-c6: {"state":"offline"}"#)
    );
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(
        report.summary,
        "FAIL: ruuvi2mqtt-c6: the status changed 1 times"
    );
    assert!(
        report.text.contains(concat!(
            "    status: {\"state\":\"online\",\"version\":\"1.5.2-dev (6534f11)\"}\n",
            "      +5:00 {\"state\":\"offline\"}\n",
        )),
        "{}",
        report.text
    );
}

#[test]
fn missed_cycles_and_restarts_are_counted() {
    let mut w = watch();
    healthy(&mut w, 5);
    let topic = "ruuvi2mqtt/diagnostics/c6-board";
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
fn invalid_diagnostics_fail() {
    let mut w = watch();
    healthy(&mut w, 15);
    let topic = "ruuvi2mqtt/diagnostics/esp32";
    let note = w.on_message(secs(100), topic, br#"{"firmware":"x"}"#, false);
    assert_eq!(note.as_deref(), Some("+1:40 esp32: client_id: null"));
    let bad_tag = diagnostics("1.5.2-dev", 1, r#"{"aabbccddee01":{"rssi":"weak"}}"#);
    let note = w.on_message(secs(160), topic, bad_tag.as_bytes(), false);
    assert_eq!(
        note.as_deref(),
        Some(r#"+2:40 esp32: tag aabbccddee01: {"rssi":"weak"}"#)
    );
    let report = w.finish(Duration::from_mins(15));
    assert_eq!(report.summary, "FAIL: 2 invalid diagnostics");
}

#[test]
fn text_from_the_broker_stays_on_its_line() {
    let mut w = watch();
    healthy(&mut w, 15);
    // A JSON escape in a name, and a raw newline in a status
    discovery(&mut w, TAG, r"Cellar\nok: 1 tags", &TAG_ENTITIES);
    let topic = "ruuvi2mqtt/status/ruuvi2mqtt_host2";
    let note = w.on_message(secs(300), topic, b"offline\nok: 1 tags", false);
    assert_eq!(
        note.as_deref(),
        Some(r"+5:00 ruuvi2mqtt_host2: offline\nok: 1 tags")
    );
    let report = w.finish(Duration::from_mins(15));
    assert!(
        report
            .text
            .contains(r"  Cellar\nok: 1 tags (aabbccddee01): 15 readings"),
        "{}",
        report.text
    );
    assert!(
        !report.text.lines().any(|l| l.starts_with("ok")),
        "{}",
        report.text
    );
}
