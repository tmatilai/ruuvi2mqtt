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
    Some(v) => konst::result::unwrap!(u16::from_str_radix(v, 10)),
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
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None => 5,
};

/// Deep sleep duration (seconds) between scan-publish cycles. The chip fully
/// powers off (CPU, RAM, radios) and reboots on wake.
pub const BLE_SLEEP_DURATION: i32 = match option_env_non_empty!("BLE_SLEEP_DURATION") {
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None => 60,
};

/// Ruuvi manufacturer-specific data company identifier (little-endian 0x0499).
pub const RUUVI_MANUFACTURER_ID: u16 = 0x0499;

// ---------------------------------------------------------------------------
// LED
// ---------------------------------------------------------------------------

/// LED operating mode.
///
/// - Unset (default): do nothing — LED GPIO is not touched.
/// - `on`: turn on while awake, off before deep sleep.
/// - `off`: explicitly turn the LED off at boot.
pub const LED_MODE: Option<&str> = option_env_non_empty!("LED_MODE");

/// LED hardware type: `ws2812` or `gpio`. Default: `ws2812`.
pub const LED_TYPE: &str = match option_env_non_empty!("LED_TYPE") {
    Some(v) => v,
    None => "ws2812",
};

/// GPIO pin number for the LED. Default: `15`.
pub const LED_GPIO: u32 = match option_env_non_empty!("LED_GPIO") {
    Some(v) => konst::result::unwrap!(u32::from_str_radix(v, 10)),
    None => 15,
};

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Log level filter.
pub const LOG_LEVEL: &str = match option_env_non_empty!("LOG_LEVEL") {
    Some(v) => v,
    None => "info",
};

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------
// Invalid values fail the build here instead of panicking on the device.

const _: () = assert!(WIFI_SSID.len() <= 32, "WIFI_SSID is longer than 32 bytes");
const _: () = assert!(WIFI_PASS.len() <= 64, "WIFI_PASS is longer than 64 bytes");
const _: () = assert!(
    WIFI_IP.is_none() || WIFI_GATEWAY.is_some(),
    "WIFI_GATEWAY must be set when WIFI_IP is set"
);
const _: () = assert!(
    is_ip4_or_unset(WIFI_IP),
    "WIFI_IP is not a valid IPv4 address"
);
const _: () = assert!(
    is_ip4_or_unset(WIFI_GATEWAY),
    "WIFI_GATEWAY is not a valid IPv4 address"
);
const _: () = assert!(
    is_ip4_or_unset(WIFI_NETMASK),
    "WIFI_NETMASK is not a valid IPv4 address"
);
const _: () = assert!(
    is_ip4_or_unset(WIFI_DNS),
    "WIFI_DNS is not a valid IPv4 address"
);
const _: () = assert!(
    BLE_SCAN_DURATION > 0 && BLE_SCAN_DURATION <= i32::MAX / 1000,
    "BLE_SCAN_DURATION must be positive (seconds)"
);
const _: () = assert!(
    BLE_SLEEP_DURATION > 0,
    "BLE_SLEEP_DURATION must be positive (seconds)"
);
const _: () = assert!(
    LED_GPIO < esp_idf_svc::sys::gpio_num_t_GPIO_NUM_MAX as u32,
    "LED_GPIO is not a valid GPIO number for this chip"
);

/// True when `v` is unset or a dotted-decimal IPv4 address.
const fn is_ip4_or_unset(v: Option<&str>) -> bool {
    let Some(s) = v else { return true };
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut dots = 0;
    let mut digits = 0;
    let mut octet: u32 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                digits += 1;
                octet = octet * 10 + (bytes[i] - b'0') as u32;
                if digits > 3 || octet > 255 {
                    return false;
                }
            }
            b'.' => {
                if digits == 0 {
                    return false;
                }
                dots += 1;
                digits = 0;
                octet = 0;
            }
            _ => return false,
        }
        i += 1;
    }
    dots == 3 && digits > 0
}
