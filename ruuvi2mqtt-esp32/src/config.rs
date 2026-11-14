//! The compile-time configuration, from environment variables. The README
//! describes each one.

use ruuvi2mqtt_esp32::battery::Gauge;

/// Like `option_env!`, but an empty string is `None`. That way an empty
/// variable on the command line, such as `make flash WIFI_IP=`, unsets it.
macro_rules! option_env_non_empty {
    ($name:expr) => {
        match option_env!($name) {
            Some(v) if !v.is_empty() => Some(v),
            _ => None,
        }
    };
}

/// Like `option_env!` but fails the build on an empty string. An empty
/// credential is more likely a failed secret lookup than a choice.
macro_rules! option_env_reject_empty {
    ($name:literal) => {
        match option_env!($name) {
            Some(v) if v.is_empty() => panic!(concat!($name, " is set but empty")),
            v => v,
        }
    };
}

// ---------------------------------------------------------------------------
// Wi-Fi
// ---------------------------------------------------------------------------

/// The SSID of the Wi-Fi network.
pub const WIFI_SSID: &str = env!("WIFI_SSID");

/// The password of the Wi-Fi network. Required unless `WIFI_OPEN`.
pub const WIFI_PASS: Option<&str> = option_env_reject_empty!("WIFI_PASS");

/// `WIFI_OPEN=true` joins an open network. A missing `WIFI_PASS` alone fails
/// the build, as the board would then join any access point with the SSID.
pub const WIFI_OPEN: bool = konst::eq_str(WIFI_OPEN_VALUE, "true");

const WIFI_OPEN_VALUE: &str = match option_env_non_empty!("WIFI_OPEN") {
    Some(v) => v,
    None => "false",
};

/// The static IP address, such as `192.168.1.50`. It turns DHCP off and
/// requires `WIFI_GATEWAY`.
pub const WIFI_IP: Option<&str> = option_env_non_empty!("WIFI_IP");

/// The gateway of the static IP.
pub const WIFI_GATEWAY: Option<&str> = option_env_non_empty!("WIFI_GATEWAY");

/// The subnet mask of the static IP. Default: `255.255.255.0`.
pub const WIFI_NETMASK: Option<&str> = option_env_non_empty!("WIFI_NETMASK");

/// The DNS server of the static IP. Default: `WIFI_GATEWAY`.
pub const WIFI_DNS: Option<&str> = option_env_non_empty!("WIFI_DNS");

/// Wi-Fi band on dual-band chips such as the ESP32-C5: `2.4` or `5`.
/// Unset uses both.
pub const WIFI_BAND: Option<&str> = option_env_non_empty!("WIFI_BAND");

/// Whether Wi-Fi scans the 5 GHz band.
pub const WIFI_SCANS_5G: bool = match WIFI_BAND {
    Some(v) => !konst::eq_str(v, "2.4"),
    None => cfg!(esp_idf_soc_wifi_support_5g),
};

/// The board's name for DHCP and mDNS, and the default MQTT client ID.
pub const DEVICE_HOSTNAME: &str = match option_env_non_empty!("DEVICE_HOSTNAME") {
    Some(v) => v,
    None => "ruuvi2mqtt-esp32",
};

// ---------------------------------------------------------------------------
// MQTT
// ---------------------------------------------------------------------------

/// The MQTT broker's hostname or IP address.
pub const MQTT_SERVER: &str = env!("MQTT_SERVER");

/// `MQTT_TLS=true` connects with TLS. Without `MQTT_CA_FILE`, ESP-IDF's CA
/// certificate bundle verifies the broker.
pub const MQTT_TLS: bool = konst::eq_str(
    match option_env_non_empty!("MQTT_TLS") {
        Some(v) => v,
        None => "false",
    },
    "true",
);

/// The path of a PEM CA certificate, embedded at compile time.
pub const MQTT_CA_FILE: Option<&str> = option_env_non_empty!("MQTT_CA_FILE");

/// `MQTT_TLS_INSECURE=true` skips the certificate's hostname check, for
/// testing. The certificate chain is still verified.
pub const MQTT_TLS_INSECURE: bool = konst::eq_str(
    match option_env_non_empty!("MQTT_TLS_INSECURE") {
        Some(v) => v,
        None => "false",
    },
    "true",
);

/// The MQTT broker's port. Default: 8883 with TLS, 1883 without.
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

/// The MQTT client ID, also the `publisher` in each reading. Default:
/// `DEVICE_HOSTNAME`.
pub const MQTT_CLIENT_ID: &str = match option_env_non_empty!("MQTT_CLIENT_ID") {
    Some(v) => v,
    None => DEVICE_HOSTNAME,
};

/// The MQTT username. Unset logs in anonymously.
pub const MQTT_USER: Option<&str> = option_env_reject_empty!("MQTT_USER");

/// The MQTT password. Requires `MQTT_USER`.
pub const MQTT_PASSWORD: Option<&str> = option_env_reject_empty!("MQTT_PASSWORD");

// ---------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------

/// The topic prefix. A reading goes to `<base_topic>/<mac>`.
pub const MQTT_BASE_TOPIC: &str = match option_env_non_empty!("MQTT_BASE_TOPIC") {
    Some(v) => v,
    None => "ruuvi2mqtt",
};

// ---------------------------------------------------------------------------
// BLE scanning
// ---------------------------------------------------------------------------

/// When to scan: `continuous` (default) scans all the time and stays
/// connected. `deep-sleep` scans while Wi-Fi connects and deep sleeps between
/// cycles.
pub const BLE_SCAN_MODE: &str = match option_env_non_empty!("BLE_SCAN_MODE") {
    Some(v) => v,
    None => "continuous",
};

/// `BLE_SCAN_MODE=continuous`.
pub const BLE_SCAN_CONTINUOUS: bool = konst::eq_str(BLE_SCAN_MODE, "continuous");

/// The length of each BLE scan, in seconds. In the continuous mode, a cycle
/// is one scan, so this is how often the readings are published.
///
/// In the deep-sleep mode, the scan runs while Wi-Fi connects, so a longer
/// one doesn't always add to the time awake. The Wi-Fi and MQTT connect time
/// in the boot log is a good value, but at least 3 seconds, so that each tag
/// gets a few chances to be heard.
pub const BLE_SCAN_DURATION: i32 = match option_env_non_empty!("BLE_SCAN_DURATION") {
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None if BLE_SCAN_CONTINUOUS => 60,
    None => 5,
};

/// The deep sleep between the cycles, in seconds. The CPU, the
/// radios and the main RAM power off, and the chip reboots on wake. The
/// continuous mode waits this long awake after a failure, then restarts. A
/// cycle timeout waits a cycle longer, and a cycle that succeeds cancels the
/// restart.
pub const BLE_SLEEP_DURATION: i32 = match option_env_non_empty!("BLE_SLEEP_DURATION") {
    Some(v) => konst::result::unwrap!(i32::from_str_radix(v, 10)),
    None => 60,
};

/// Whether `sleep()` in main.rs waits awake while a computer is on the USB
/// port. The continuous mode always stays awake, so it rejects `false`.
pub const USB_STAY_AWAKE: bool = konst::eq_str(USB_STAY_AWAKE_VALUE, "true");

const USB_STAY_AWAKE_VALUE: &str = match option_env_non_empty!("USB_STAY_AWAKE") {
    Some(v) => v,
    None => "true",
};

/// Ruuvi manufacturer-specific data company identifier (little-endian 0x0499).
pub const RUUVI_MANUFACTURER_ID: u16 = 0x0499;

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

/// The board's battery gauge: `max17048` or `m5pm1`. Unset reads none.
pub const BATTERY: Option<&str> = option_env_non_empty!("BATTERY");

/// `BATTERY` as a gauge.
pub const BATTERY_GAUGE: Option<Gauge> = match BATTERY {
    Some(v) if konst::eq_str(v, "max17048") => Some(Gauge::Max17048),
    Some(v) if konst::eq_str(v, "m5pm1") => Some(Gauge::M5pm1),
    _ => None,
};

/// The gauge's I2C pins. Required with `BATTERY`: they depend on the board.
pub const BATTERY_SDA: Option<u8> = match option_env_non_empty!("BATTERY_SDA") {
    Some(v) => Some(konst::result::unwrap!(u8::from_str_radix(v, 10))),
    None => None,
};
pub const BATTERY_SCL: Option<u8> = match option_env_non_empty!("BATTERY_SCL") {
    Some(v) => Some(konst::result::unwrap!(u8::from_str_radix(v, 10))),
    None => None,
};

/// A pin that powers the gauge's I2C bus when high, turned on for the read.
pub const BATTERY_POWER_GPIO: Option<u8> = match option_env_non_empty!("BATTERY_POWER_GPIO") {
    Some(v) => Some(konst::result::unwrap!(u8::from_str_radix(v, 10))),
    None => None,
};

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

/// The LED's mode.
///
/// - `on`, the default when `LED_GPIO` is set: in the continuous mode, the
///   LED breathes slowly and dimly. In the deep-sleep mode, it lights while
///   the board is awake. It blinks while something fails.
/// - `errors`: the LED blinks only while something fails.
/// - `off`: the LED is turned off at boot.
///
/// Without `LED_GPIO` and `LED_MODE`, the LED's pin is not touched.
pub const LED_MODE: Option<&str> = match option_env_non_empty!("LED_MODE") {
    Some(v) => Some(v),
    None if LED_GPIO.is_some() => Some("on"),
    None => None,
};

/// The LED's type: `ws2812` or `gpio`. Default: `ws2812`.
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

/// The LED's GPIO pin number. Required with `LED_MODE`: the pin depends
/// on the board, and a wrong one may drive something else.
pub const LED_GPIO: Option<u32> = match option_env_non_empty!("LED_GPIO") {
    Some(v) => Some(konst::result::unwrap!(u32::from_str_radix(v, 10))),
    None => None,
};

/// The GPIO pin that powers the LED when high, for boards that switch the LED's
/// power.
pub const LED_POWER_GPIO: Option<u32> = match option_env_non_empty!("LED_POWER_GPIO") {
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

/// The log level of the firmware's own code.
pub const LOG_LEVEL: &str = match option_env_non_empty!("LOG_LEVEL") {
    Some(v) => v,
    None => "info",
};

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------
// Invalid values fail the build here instead of panicking on the device.

const _: () = assert!(!WIFI_SSID.is_empty(), "WIFI_SSID is empty");
const _: () = assert!(WIFI_SSID.len() <= 32, "WIFI_SSID is longer than 32 bytes");
const _: () = assert!(
    WIFI_OPEN || konst::eq_str(WIFI_OPEN_VALUE, "false"),
    "WIFI_OPEN must be true or false"
);
const _: () = assert!(
    WIFI_OPEN || WIFI_PASS.is_some(),
    "WIFI_PASS must be set, or WIFI_OPEN=true for an open network"
);
const _: () = assert!(
    !WIFI_OPEN || WIFI_PASS.is_none(),
    "WIFI_PASS must be unset when WIFI_OPEN=true"
);
const _: () = assert!(
    is_wpa2_key_or_unset(WIFI_PASS),
    "WIFI_PASS must be 8 to 63 characters or 64 hex digits"
);
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
const _: () = assert!(!MQTT_SERVER.is_empty(), "MQTT_SERVER is empty");
const _: () = assert!(
    MQTT_PASSWORD.is_none() || MQTT_USER.is_some(),
    "MQTT_USER must be set when MQTT_PASSWORD is set"
);
const _: () = assert!(
    BLE_SCAN_DURATION > 0 && BLE_SCAN_DURATION <= i32::MAX / 1000,
    "BLE_SCAN_DURATION must be positive (seconds)"
);
const _: () = assert!(
    BLE_SCAN_CONTINUOUS || konst::eq_str(BLE_SCAN_MODE, "deep-sleep"),
    "BLE_SCAN_MODE must be continuous or deep-sleep"
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
    USB_STAY_AWAKE || !BLE_SCAN_CONTINUOUS,
    "USB_STAY_AWAKE=false needs BLE_SCAN_MODE=deep-sleep"
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
    BATTERY.is_none() || BATTERY_GAUGE.is_some(),
    "BATTERY must be max17048 or m5pm1"
);
const _: () = assert!(
    BATTERY.is_none() || (BATTERY_SDA.is_some() && BATTERY_SCL.is_some()),
    "BATTERY_SDA and BATTERY_SCL must be set with BATTERY"
);
const _: () = assert!(
    BATTERY.is_some()
        || (BATTERY_SDA.is_none() && BATTERY_SCL.is_none() && BATTERY_POWER_GPIO.is_none()),
    "BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO are only for BATTERY"
);
const _: () = assert!(
    battery_pins_valid(),
    "BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO must be valid GPIO numbers for this chip, differ from each other, and not be LED_GPIO. Only BATTERY_POWER_GPIO may be LED_POWER_GPIO"
);

const fn battery_pins_valid() -> bool {
    let pins = [BATTERY_SDA, BATTERY_SCL, BATTERY_POWER_GPIO];
    let mut i = 0;
    while i < pins.len() {
        if let Some(pin) = pins[i] {
            #[allow(clippy::cast_sign_loss)] // GPIO_NUM_MAX is positive
            if pin as u32 >= esp_idf_svc::sys::gpio_num_t_GPIO_NUM_MAX as u32 {
                return false;
            }
            if let Some(led) = LED_GPIO {
                if pin as u32 == led {
                    return false;
                }
            }
            // The LED and the gauge may share only their power pin.
            if let Some(power) = LED_POWER_GPIO {
                if i < 2 && pin as u32 == power {
                    return false;
                }
            }
            let mut j = i + 1;
            while j < pins.len() {
                if let Some(other) = pins[j] {
                    if pin == other {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}
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
    LED_POWER_GPIO.is_none() || LED_GPIO.is_some(),
    "LED_POWER_GPIO is only for an LED with LED_GPIO"
);
const _: () = assert!(
    match (LED_POWER_GPIO, LED_GPIO) {
        (Some(power), Some(gpio)) => {
            power < esp_idf_svc::sys::gpio_num_t_GPIO_NUM_MAX as u32 && power != gpio
        }
        _ => true,
    },
    "LED_POWER_GPIO is not a valid GPIO number for this chip, or is LED_GPIO"
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

/// True when `v` is unset, a WPA2 passphrase of 8 to 63 characters, or a key
/// of 64 hex digits.
const fn is_wpa2_key_or_unset(v: Option<&str>) -> bool {
    let Some(s) = v else { return true };
    let bytes = s.as_bytes();
    if bytes.len() >= 8 && bytes.len() <= 63 {
        return true;
    }
    if bytes.len() != 64 {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_hexdigit() {
            return false;
        }
        i += 1;
    }
    true
}

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
