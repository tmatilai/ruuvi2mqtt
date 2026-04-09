// Compile-time configuration for ruuvi2mqtt-esp32.
//
// Most settings are read from environment variables at compile time.

/// Like `option_env!` but treats empty strings as `None`, so that setting a
/// variable to empty on the command line (e.g. `make flash WIFI_IP=`) is
/// equivalent to unsetting it.
macro_rules! option_env_non_empty {
    ($name:expr) => {
        match option_env!($name) {
            Some(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    };
}

// ---------------------------------------------------------------------------
// Wi-Fi
// ---------------------------------------------------------------------------

/// SSID of the Wi-Fi network to connect to.
pub const WIFI_SSID: &str = env!("WIFI_SSID");

/// Password of the Wi-Fi network.
pub const WIFI_PASS: &str = env!("WIFI_PASS");

/// Optional static IP address (e.g. `"192.168.1.50"`). If set, DHCP is
/// disabled and `WIFI_GATEWAY` must also be provided.
pub const WIFI_IP: Option<&str> = option_env_non_empty!("WIFI_IP");

/// Gateway address for static IP (e.g. `"192.168.1.1"`).
pub const WIFI_GATEWAY: Option<&str> = option_env_non_empty!("WIFI_GATEWAY");

/// Subnet mask for static IP (e.g. `"255.255.255.0"`). Defaults to
/// `255.255.255.0` when `WIFI_IP` is set.
pub const WIFI_NETMASK: Option<&str> = option_env_non_empty!("WIFI_NETMASK");

/// DNS server for static IP. Defaults to `WIFI_GATEWAY` when not set.
pub const WIFI_DNS: Option<&str> = option_env_non_empty!("WIFI_DNS");

/// Device hostname. Used as the default MQTT client ID and announced via
/// DHCP (has no network effect when using static IP).
pub const DEVICE_HOSTNAME: &str = match option_env_non_empty!("DEVICE_HOSTNAME") {
    Some(v) => v,
    None => "ruuvi2mqtt-esp32",
};

// ---------------------------------------------------------------------------
// MQTT
// ---------------------------------------------------------------------------

/// Hostname or IP address of the MQTT broker (matches Linux `mqtt.server`).
pub const MQTT_SERVER: &str = env!("MQTT_SERVER");

/// Enable TLS/SSL for the MQTT connection (matches Linux `mqtt.tls`).
///
/// Set to "true" to use `mqtts://`. When enabled without `MQTT_CA_FILE`,
/// the ESP-IDF built-in CA certificate bundle is used for verification.
pub const MQTT_TLS: bool = konst::eq_str(
    match option_env_non_empty!("MQTT_TLS") {
        Some(v) => v,
        None => "false",
    },
    "true",
);

/// Path to a custom CA certificate (PEM format) to embed at compile time
/// (matches Linux `mqtt.ca_file`).
///
/// When set, the file is read by the build script and made available as
/// `MQTT_CA_PEM` via `include_bytes!`.
pub const MQTT_CA_FILE: Option<&str> = option_env_non_empty!("MQTT_CA_FILE");

/// Skip TLS certificate verification (matches Linux `mqtt.tls_insecure`).
///
/// WARNING: Only use for testing — disables certificate checks.
pub const MQTT_TLS_INSECURE: bool = konst::eq_str(
    match option_env_non_empty!("MQTT_TLS_INSECURE") {
        Some(v) => v,
        None => "false",
    },
    "true",
);

/// MQTT broker port (matches Linux `mqtt.port`).
/// Default: 8883 when TLS is enabled, 1883 otherwise.
pub const MQTT_PORT: u16 = match option_env_non_empty!("MQTT_PORT") {
    Some(v) => konst::unwrap_ctx!(konst::primitive::parse_u16(v)),
    None => {
        if MQTT_TLS {
            8883
        } else {
            1883
        }
    }
};

/// Optional MQTT client identifier override (matches Linux `mqtt.client_id`).
///
/// If not set, the runtime code derives the client ID from `DEVICE_HOSTNAME`.
pub const MQTT_CLIENT_ID: Option<&str> = option_env_non_empty!("MQTT_CLIENT_ID");

/// Optional MQTT username (matches Linux `mqtt.user`).
pub const MQTT_USER: &str = match option_env_non_empty!("MQTT_USER") {
    Some(v) => v,
    None => "",
};

/// Optional MQTT password (matches Linux `mqtt.password`).
pub const MQTT_PASSWORD: &str = match option_env_non_empty!("MQTT_PASSWORD") {
    Some(v) => v,
    None => "",
};

// ---------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------

/// Base topic prefix (matches Linux `mqtt.base_topic`).
/// Messages will be published to `{MQTT_BASE_TOPIC}/{MAC_ADDRESS}`.
pub const MQTT_BASE_TOPIC: &str = match option_env_non_empty!("MQTT_BASE_TOPIC") {
    Some(v) => v,
    None => "ruuvi2mqtt",
};

// ---------------------------------------------------------------------------
// BLE scanning
// ---------------------------------------------------------------------------

/// Duration (seconds) of each BLE scan pass. BLE scanning runs in parallel
/// with Wi-Fi connection, so increasing this does not necessarily add to the
/// total active time. For best results, set this close to your typical
/// Wi-Fi + MQTT connect time (check the boot log) but at least 3 seconds
/// to have the chance to catch all RuuviTags (~2.5s advertisement interval).
#[allow(clippy::doc_markdown)] // RuuviTag is a product name, not a code identifier
pub const BLE_SCAN_DURATION: i32 = match option_env_non_empty!("BLE_SCAN_DURATION") {
    Some(v) => konst::unwrap_ctx!(konst::primitive::parse_i32(v)),
    None => 5,
};

/// Deep sleep duration (seconds) between scan-publish cycles. The chip fully
/// powers off (CPU, RAM, radios) and reboots on wake.
pub const BLE_SLEEP_DURATION: i32 = match option_env_non_empty!("BLE_SLEEP_DURATION") {
    Some(v) => konst::unwrap_ctx!(konst::primitive::parse_i32(v)),
    None => 60,
};

/// Ruuvi manufacturer-specific data company identifier (little-endian 0x0499).
pub const RUUVI_MANUFACTURER_ID: u16 = 0x0499;

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Log level filter.
pub const LOG_LEVEL: &str = match option_env_non_empty!("LOG_LEVEL") {
    Some(v) => v,
    None => "info",
};
