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

/// Wi-Fi band on dual-band chips such as the ESP32-C5: `2.4` or `5`.
/// Unset uses both.
pub const WIFI_BAND: Option<&str> = option_env_non_empty!("WIFI_BAND");

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

/// Skip the TLS hostname check (matches Linux `mqtt.tls_insecure`). The
/// certificate chain is still verified.
///
/// WARNING: Only use for testing.
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

/// MQTT client identifier (matches Linux `mqtt.client_id`). Also sent as
/// `publisher` in each reading. Default: `DEVICE_HOSTNAME`.
pub const MQTT_CLIENT_ID: &str = match option_env_non_empty!("MQTT_CLIENT_ID") {
    Some(v) => v,
    None => DEVICE_HOSTNAME,
};

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

/// When to scan: `continuous` (default) scans all the time and stays
/// connected. `parallel` scans while Wi-Fi connects and deep sleeps between
/// cycles.
pub const BLE_SCAN_MODE: &str = match option_env_non_empty!("BLE_SCAN_MODE") {
    Some(v) => v,
    None => "continuous",
};

/// `BLE_SCAN_MODE=continuous`.
pub const BLE_SCAN_CONTINUOUS: bool = konst::eq_str(BLE_SCAN_MODE, "continuous");

/// Duration (seconds) of each BLE scan pass. In the continuous mode, a cycle
/// is one scan, so this is how often the readings are published.
///
/// In the parallel scan mode, BLE scanning runs during the Wi-Fi connection,
/// so increasing this does not necessarily add to the total active time. For
/// best results, set this close to your typical Wi-Fi + MQTT connect time
/// (check the boot log) but at least 3 seconds to have the chance to catch
/// all RuuviTags (~2.5s advertisement interval).
#[allow(clippy::doc_markdown)] // RuuviTag is a product name, not a code identifier
pub const BLE_SCAN_DURATION: i32 = match option_env_non_empty!("BLE_SCAN_DURATION") {
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None if BLE_SCAN_CONTINUOUS => 60,
    None => 5,
};

/// Deep sleep duration (seconds) between scan-publish cycles. The chip fully
/// powers off (CPU, RAM, radios) and reboots on wake. The continuous mode
/// waits this long awake after a failure, then restarts. A cycle timeout
/// waits a cycle longer, and a cycle that succeeds cancels the restart.
pub const BLE_SLEEP_DURATION: i32 = match option_env_non_empty!("BLE_SLEEP_DURATION") {
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None => 60,
};

/// Whether `sleep()` in main.rs waits awake while a computer is on the USB
/// port.
pub const USB_STAY_AWAKE: bool = konst::eq_str(USB_STAY_AWAKE_VALUE, "true");

const USB_STAY_AWAKE_VALUE: &str = match option_env_non_empty!("USB_STAY_AWAKE") {
    Some(v) => v,
    None => "true",
};

/// Ruuvi manufacturer-specific data company identifier (little-endian 0x0499).
pub const RUUVI_MANUFACTURER_ID: u16 = 0x0499;

// ---------------------------------------------------------------------------
// Antenna
// ---------------------------------------------------------------------------

/// The antenna of the Seeed XIAO ESP32-C6: `internal` (ceramic) or
/// `external` (U.FL). Unset, its RF switch gets no power, and the board
/// hears little.
pub const ANTENNA: Option<&str> = option_env_non_empty!("ANTENNA");

/// `ANTENNA=external`.
pub const ANTENNA_EXTERNAL: bool = match ANTENNA {
    Some(v) => konst::eq_str(v, "external"),
    None => false,
};

// ---------------------------------------------------------------------------
// LED
// ---------------------------------------------------------------------------

/// LED operating mode.
///
/// - `on`, the default when `LED_GPIO` is set: scanning continuously,
///   breathe slowly and dimly. In the deep-sleep mode, light while awake.
///   Blink while something fails.
/// - `errors`: blink only while something fails.
/// - `off`: explicitly turn the LED off at boot.
///
/// Without `LED_GPIO` and `LED_MODE`, the LED's pin is not touched.
pub const LED_MODE: Option<&str> = match option_env_non_empty!("LED_MODE") {
    Some(v) => Some(v),
    None if LED_GPIO.is_some() => Some("on"),
    None => None,
};

/// LED hardware type: `ws2812` or `gpio`. Default: `ws2812`.
pub const LED_TYPE: &str = match option_env_non_empty!("LED_TYPE") {
    Some(v) => v,
    None => "ws2812",
};

/// `LED_ACTIVE_LOW=true`: a `gpio` LED that lights when its pin is low.
pub const LED_ACTIVE_LOW: bool = konst::eq_str(LED_ACTIVE_LOW_VALUE, "true");

const LED_ACTIVE_LOW_VALUE: &str = match option_env_non_empty!("LED_ACTIVE_LOW") {
    Some(v) => v,
    None => "false",
};

/// GPIO pin number for the LED. Required with `LED_MODE`: the pin depends
/// on the board, and a wrong one may drive something else.
pub const LED_GPIO: Option<u32> = match option_env_non_empty!("LED_GPIO") {
    Some(v) => Some(konst::result::unwrap!(u32::from_str_radix(v, 10))),
    None => None,
};

/// WS2812 color while on, as hex `RRGGBB`. Low values keep it dim.
/// Default: `000020`, a dim blue.
const LED_COLOR_HEX: &str = match option_env_non_empty!("LED_COLOR") {
    Some(v) => v,
    None => "000020",
};

/// `LED_COLOR` as `0xRRGGBB`.
pub const LED_COLOR: u32 = konst::result::unwrap!(u32::from_str_radix(LED_COLOR_HEX, 16));

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
    match WIFI_BAND {
        Some(v) => konst::eq_str(v, "2.4") || konst::eq_str(v, "5"),
        None => true,
    },
    "WIFI_BAND must be 2.4 or 5"
);
const _: () = assert!(
    WIFI_BAND.is_none() || cfg!(esp_idf_soc_wifi_support_5g),
    "WIFI_BAND is only for dual-band chips such as the ESP32-C5"
);
const _: () = assert!(
    BLE_SCAN_DURATION > 0 && BLE_SCAN_DURATION <= i32::MAX / 1000,
    "BLE_SCAN_DURATION must be positive (seconds)"
);
const _: () = assert!(
    BLE_SCAN_CONTINUOUS || konst::eq_str(BLE_SCAN_MODE, "parallel"),
    "BLE_SCAN_MODE must be continuous or parallel"
);
const _: () = assert!(
    BLE_SLEEP_DURATION > 0,
    "BLE_SLEEP_DURATION must be positive (seconds)"
);
const _: () = assert!(
    USB_STAY_AWAKE || konst::eq_str(USB_STAY_AWAKE_VALUE, "false"),
    "USB_STAY_AWAKE must be true or false"
);
const _: () = assert!(
    match ANTENNA {
        Some(v) => ANTENNA_EXTERNAL || konst::eq_str(v, "internal"),
        None => true,
    },
    "ANTENNA must be internal or external"
);
const _: () = assert!(
    ANTENNA.is_none() || konst::slice::eq_bytes(esp_idf_svc::sys::CONFIG_IDF_TARGET, b"esp32c6\0"),
    "ANTENNA is only for the Seeed XIAO ESP32-C6"
);
const _: () = assert!(
    match LED_MODE {
        Some(v) => konst::eq_str(v, "on") || konst::eq_str(v, "errors") || konst::eq_str(v, "off"),
        None => true,
    },
    "LED_MODE must be on, errors or off"
);
const _: () = assert!(
    konst::eq_str(LED_TYPE, "ws2812") || konst::eq_str(LED_TYPE, "gpio"),
    "LED_TYPE must be ws2812 or gpio"
);
const _: () = assert!(
    LED_MODE.is_none() || LED_GPIO.is_some(),
    "LED_GPIO must be set when LED_MODE is set"
);
const _: () = assert!(
    match LED_GPIO {
        Some(gpio) => gpio < esp_idf_svc::sys::gpio_num_t_GPIO_NUM_MAX as u32,
        None => true,
    },
    "LED_GPIO is not a valid GPIO number for this chip"
);
const _: () = assert!(
    LED_ACTIVE_LOW || konst::eq_str(LED_ACTIVE_LOW_VALUE, "false"),
    "LED_ACTIVE_LOW must be true or false"
);
const _: () = assert!(
    !LED_ACTIVE_LOW || konst::eq_str(LED_TYPE, "gpio"),
    "LED_ACTIVE_LOW is only for LED_TYPE=gpio"
);
const _: () = assert!(
    LED_COLOR_HEX.len() == 6,
    "LED_COLOR must be six hex digits, RRGGBB"
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
