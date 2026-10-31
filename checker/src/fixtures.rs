//! Payloads for the unit tests, as the app and the firmware publish them.

pub const COMMIT: &str = "6534f11d0c2e5a7b9f8e1d2c3b4a5968778695a4";
pub const HOST1: &str = "ruuvi2mqtt_host1";
pub const HOST2: &str = "ruuvi2mqtt_host2";
pub const TAG: &str = "aabbccddee01";
pub const AIR: &str = "c4d2a1b3e5f6";
pub const BOARD: &str = "ruuvi2mqtt-c6";

pub fn online(version: &str) -> String {
    format!(r#"{{"state":"online","version":"{version}"}}"#)
}

pub fn reading(publisher: &str) -> String {
    format!(
        r#"{{"humidity":45.5,"pressure":1003.2,"temperature":21.3,"battery":2.9,"battery_low":false,"tx_power":4,"publisher":"{publisher}"}}"#
    )
}

/// A format 6 reading: no PM1, PM4 or PM10
pub fn air_reading(publisher: &str) -> String {
    format!(
        r#"{{"humidity":55.3,"pressure":1011.02,"temperature":29.5,"battery":null,"battery_low":null,"tx_power":null,"co2":201,"pm2_5":11.2,"voc":5,"nox":1,"luminosity":13026.67,"iaqs":81,"calibrating":false,"publisher":"{publisher}"}}"#
    )
}

/// A data format E1 reading
pub fn air_reading_e1(publisher: &str) -> String {
    air_reading(publisher).replace(
        r#""pm2_5":11.2,"#,
        r#""pm1":10.1,"pm2_5":11.2,"pm4":12.3,"pm10":13.4,"#,
    )
}

/// A discovery config for an entity of a tag
pub fn discovery(id: &str, entity: &str, name: &str) -> String {
    format!(
        r#"{{"state_topic":"ruuvi2mqtt/{id}","unique_id":"ruuvi_{id}_{entity}","device":{{"name":"{name}"}}}}"#
    )
}

/// A board's diagnostics: Wi-Fi connects 1 s into the 5 s scan
pub fn diagnostics(firmware: &str, cycle: u64, tags: &str) -> String {
    format!(
        r#"{{"firmware":"ruuvi2mqtt-esp32 {firmware}","client_id":"{BOARD}","chip":"esp32c6","cycle":{cycle},"settings":{{"scan_mode":"deep-sleep","scan_duration":5}},"wifi_rssi":-61,"wifi_channel":6,"wifi_cache":"hit","free_heap":151000,"scan_start_ms":200,"scan_end_ms":5200,"wifi_ms":1200,"mqtt_ms":2000,"cycle_ms":5300,"tags":{tags},"adverts_before_wifi":1,"adverts_after_wifi":8,"other_adverts_before_wifi":10,"other_adverts_after_wifi":80}}"#
    )
}
