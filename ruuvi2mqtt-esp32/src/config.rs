//! The board's configuration. `make` checks the environment variables with
//! [`Config::parse`] and writes them to the `config` partition with
//! [`encode`]. The firmware reads them back at boot with [`decode`] and the
//! same [`Config::parse`]. The README describes each setting.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::OnceLock;

use crate::battery::Gauge;

/// The size of the `config` partition in `partitions.csv`.
pub const PARTITION_SIZE: usize = 0x4000;

/// The environment variables that the configuration reads. `MQTT_CA_FILE` is
/// read as a file into `MQTT_CA`.
pub const NAMES: &[&str] = &[
    "WIFI_SSID",
    "WIFI_PASS",
    "WIFI_OPEN",
    "WIFI_IP",
    "WIFI_GATEWAY",
    "WIFI_NETMASK",
    "WIFI_DNS",
    "WIFI_BAND",
    "DEVICE_HOSTNAME",
    "MQTT_SERVER",
    "MQTT_TLS",
    "MQTT_TLS_INSECURE",
    "MQTT_PORT",
    "MQTT_CLIENT_ID",
    "MQTT_USER",
    "MQTT_PASSWORD",
    "MQTT_BASE_TOPIC",
    "BLE_SCAN_MODE",
    "BLE_SCAN_DURATION",
    "BLE_SLEEP_DURATION",
    "USB_STAY_AWAKE",
    "BATTERY",
    "BATTERY_SDA",
    "BATTERY_SCL",
    "BATTERY_POWER_GPIO",
    "ANTENNA",
    "LED_MODE",
    "LED_TYPE",
    "LED_ACTIVE_LOW",
    "LED_GPIO",
    "LED_POWER_GPIO",
    "LED_COLOR",
    "BUTTON_GPIO",
    "LOG_LEVEL",
];

/// The RF switch pins of the Seeed XIAO ESP32-C6 that `ANTENNA` drives. Low
/// on `ANTENNA_SWITCH_POWER` powers the switch.
pub const ANTENNA_SWITCH_POWER: u32 = 3;

/// Low selects the internal antenna, high the external one.
pub const ANTENNA_SELECT: u32 = 14;

/// What the checks need to know about a chip.
#[derive(Debug, PartialEq, Eq)]
pub struct Chip {
    /// The `CHIP` value, such as `esp32c6`.
    pub name: &'static str,
    /// ESP-IDF's `GPIO_NUM_MAX`.
    pub gpio_count: u32,
    /// Whether the chip has 5 GHz Wi-Fi.
    pub wifi_5g: bool,
}

/// The chips that the Makefile builds for.
pub const CHIPS: &[Chip] = &[
    Chip {
        name: "esp32",
        gpio_count: 40,
        wifi_5g: false,
    },
    Chip {
        name: "esp32c3",
        gpio_count: 22,
        wifi_5g: false,
    },
    Chip {
        name: "esp32c5",
        gpio_count: 29,
        wifi_5g: true,
    },
    Chip {
        name: "esp32c6",
        gpio_count: 31,
        wifi_5g: false,
    },
    Chip {
        name: "esp32s3",
        gpio_count: 49,
        wifi_5g: false,
    },
];

impl Chip {
    /// Returns the chip with the `CHIP` value `name`.
    #[must_use]
    pub const fn find(name: &str) -> Option<&'static Chip> {
        let mut i = 0;
        while i < CHIPS.len() {
            if konst::eq_str(CHIPS[i].name, name) {
                return Some(&CHIPS[i]);
            }
            i += 1;
        }
        None
    }
}

/// The parsed configuration, with the defaults filled in. A field holds the
/// setting of the same name.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    pub wifi_ssid: String,
    pub wifi_pass: Option<String>,
    pub wifi_open: bool,
    pub wifi_ip: Option<String>,
    pub wifi_gateway: Option<String>,
    pub wifi_netmask: Option<String>,
    pub wifi_dns: Option<String>,
    pub wifi_band: Option<String>,
    /// Whether Wi-Fi scans the 5 GHz band.
    pub wifi_scans_5g: bool,
    pub device_hostname: String,

    pub mqtt_server: String,
    pub mqtt_tls: bool,
    /// The PEM CA certificate from `MQTT_CA_FILE`, NUL-terminated for ESP-IDF.
    pub mqtt_ca: Option<CString>,
    pub mqtt_tls_insecure: bool,
    pub mqtt_port: u16,
    pub mqtt_client_id: String,
    pub mqtt_user: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_base_topic: String,

    pub ble_scan_mode: String,
    /// `BLE_SCAN_MODE=continuous`.
    pub ble_scan_continuous: bool,
    pub ble_scan_duration: i32,
    pub ble_sleep_duration: i32,
    pub usb_stay_awake: bool,

    /// From `BATTERY`.
    pub battery_gauge: Option<Gauge>,
    pub battery_sda: Option<u8>,
    pub battery_scl: Option<u8>,
    pub battery_power_gpio: Option<u8>,

    pub antenna: Option<String>,
    /// `ANTENNA=external`.
    pub antenna_external: bool,

    pub led_mode: Option<String>,
    pub led_type: String,
    pub led_active_low: bool,
    pub led_gpio: Option<u32>,
    pub led_power_gpio: Option<u32>,
    /// `LED_COLOR` as `0xRRGGBB`.
    pub led_color: u32,

    pub button_gpio: Option<u32>,

    pub log_level: log::LevelFilter,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Returns the configuration that [`set`] stored.
///
/// # Panics
///
/// Before [`set`].
pub fn get() -> &'static Config {
    CONFIG.get().expect("the configuration is not loaded")
}

/// Returns the configuration that [`set`] stored, if any.
pub fn try_get() -> Option<&'static Config> {
    CONFIG.get()
}

/// Stores the configuration for [`get`]. Only the first call stores it.
pub fn set(config: Config) {
    let _ = CONFIG.set(config);
}

/// Returns the partition's contents for `values`: their JSON, padded with
/// 0xFF to [`PARTITION_SIZE`], the value of erased flash.
///
/// # Errors
///
/// When the JSON is larger than the partition.
pub fn encode(values: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(values).map_err(|e| e.to_string())?;
    if bytes.len() > PARTITION_SIZE {
        return Err(format!(
            "The configuration is {} bytes, more than the partition's {PARTITION_SIZE}",
            bytes.len()
        ));
    }
    bytes.resize(PARTITION_SIZE, 0xFF);
    Ok(bytes)
}

/// Returns the values in the partition's contents. The JSON ends at the
/// first 0xFF, which UTF-8 never contains.
///
/// # Errors
///
/// When the partition is empty or holds no valid JSON object of strings.
pub fn decode(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let end = bytes.iter().position(|&b| b == 0xFF).unwrap_or(bytes.len());
    if end == 0 {
        return Err("The config partition is empty. Write it with `make config`".to_string());
    }
    serde_json::from_slice(&bytes[..end])
        .map_err(|e| format!("The config partition holds no valid configuration: {e}"))
}

/// The values being parsed, and the errors found so far.
struct Values<'a> {
    values: &'a BTreeMap<String, String>,
    errors: Vec<String>,
}

impl Values<'_> {
    /// Returns the value of `name`. An empty value is unset, so that
    /// `make flash WIFI_IP=` unsets it.
    fn get(&self, name: &str) -> Option<String> {
        self.values.get(name).filter(|v| !v.is_empty()).cloned()
    }

    /// Returns the value of `name`, an error when it is set but empty. An
    /// empty credential is more likely a failed secret lookup than a choice.
    fn credential(&mut self, name: &str) -> Option<String> {
        match self.values.get(name) {
            Some(v) if v.is_empty() => {
                self.error(format!("{name} is set but empty"));
                None
            }
            v => v.cloned(),
        }
    }

    /// Returns `name` as `true` or `false`.
    fn bool(&mut self, name: &str, default: bool) -> bool {
        match self.get(name).as_deref() {
            None => default,
            Some("true") => true,
            Some("false") => false,
            Some(_) => {
                self.error(format!("{name} must be true or false"));
                default
            }
        }
    }

    /// Returns `name` as a decimal number.
    fn number<T: FromStr>(&mut self, name: &str) -> Option<T>
    where
        T::Err: Display,
    {
        let v = self.get(name)?;
        v.parse()
            .inspect_err(|e| self.error(format!("{name} must be a number: {e}")))
            .ok()
    }

    /// Returns `name` if it is one of `choices`.
    fn choice(&mut self, name: &str, choices: &[&str]) -> Option<String> {
        let v = self.get(name)?;
        if choices.contains(&v.as_str()) {
            Some(v)
        } else {
            self.error(format!("{name} must be {}", or_list(choices)));
            None
        }
    }

    fn error(&mut self, message: String) {
        self.errors.push(message);
    }

    fn check(&mut self, ok: bool, message: &str) {
        if !ok {
            self.error(message.to_string());
        }
    }
}

/// Returns `["a", "b", "c"]` as "a, b or c".
fn or_list(choices: &[&str]) -> String {
    match choices {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

impl Config {
    /// Parses and checks `values` for a board with `chip`.
    ///
    /// # Errors
    ///
    /// Each rule that the values break, as a message that names the rule.
    #[allow(clippy::too_many_lines)]
    pub fn parse(values: &BTreeMap<String, String>, chip: &Chip) -> Result<Self, Vec<String>> {
        let mut v = Values {
            values,
            errors: Vec::new(),
        };

        match v.get("CHIP") {
            Some(name) if name == chip.name => {}
            Some(name) => v.error(format!(
                "The configuration is for an {name}, but this board is an {}",
                chip.name
            )),
            None => v.error("CHIP is missing".to_string()),
        }

        // Wi-Fi
        let wifi_ssid = v.get("WIFI_SSID").unwrap_or_default();
        v.check(!wifi_ssid.is_empty(), "WIFI_SSID must be set");
        v.check(wifi_ssid.len() <= 32, "WIFI_SSID is longer than 32 bytes");
        let wifi_pass = v.credential("WIFI_PASS");
        let wifi_open = v.bool("WIFI_OPEN", false);
        v.check(
            wifi_open || wifi_pass.is_some(),
            "WIFI_PASS must be set, or WIFI_OPEN=true for an open network",
        );
        v.check(
            !wifi_open || wifi_pass.is_none(),
            "WIFI_PASS must be unset when WIFI_OPEN=true",
        );
        v.check(
            wifi_pass.as_deref().is_none_or(is_wpa2_key),
            "WIFI_PASS must be 8 to 63 characters or 64 hex digits",
        );
        let wifi_ip = v.get("WIFI_IP");
        let wifi_gateway = v.get("WIFI_GATEWAY");
        let wifi_netmask = v.get("WIFI_NETMASK");
        let wifi_dns = v.get("WIFI_DNS");
        v.check(
            wifi_ip.is_none() || wifi_gateway.is_some(),
            "WIFI_GATEWAY must be set when WIFI_IP is set",
        );
        for (name, value) in [
            ("WIFI_IP", &wifi_ip),
            ("WIFI_GATEWAY", &wifi_gateway),
            ("WIFI_NETMASK", &wifi_netmask),
            ("WIFI_DNS", &wifi_dns),
        ] {
            if value
                .as_deref()
                .is_some_and(|ip| ip.parse::<std::net::Ipv4Addr>().is_err())
            {
                v.error(format!("{name} is not a valid IPv4 address"));
            }
        }
        let wifi_band = v.choice("WIFI_BAND", &["2.4", "5"]);
        if wifi_band.is_some() && !chip.wifi_5g {
            v.error("WIFI_BAND is only for dual-band chips such as the ESP32-C5".to_string());
        }
        let wifi_scans_5g = match wifi_band.as_deref() {
            Some(band) => band != "2.4",
            None => chip.wifi_5g,
        };
        let device_hostname = v
            .get("DEVICE_HOSTNAME")
            .unwrap_or_else(|| "ruuvi2mqtt-esp32".to_string());

        // MQTT
        let mqtt_server = v.get("MQTT_SERVER").unwrap_or_default();
        v.check(!mqtt_server.is_empty(), "MQTT_SERVER must be set");
        let mqtt_tls = v.bool("MQTT_TLS", false);
        let mqtt_ca = v.get("MQTT_CA").and_then(|pem| {
            let cstr = pem
                .contains("-----BEGIN CERTIFICATE-----")
                .then(|| CString::new(pem).ok())
                .flatten();
            v.check(cstr.is_some(), "MQTT_CA_FILE must be a PEM certificate");
            cstr
        });
        let mqtt_tls_insecure = v.bool("MQTT_TLS_INSECURE", false);
        let mqtt_port = v
            .number("MQTT_PORT")
            .unwrap_or(if mqtt_tls { 8883 } else { 1883 });
        let mqtt_client_id = v
            .get("MQTT_CLIENT_ID")
            .unwrap_or_else(|| device_hostname.clone());
        let mqtt_user = v.credential("MQTT_USER");
        let mqtt_password = v.credential("MQTT_PASSWORD");
        v.check(
            mqtt_password.is_none() || mqtt_user.is_some(),
            "MQTT_USER must be set when MQTT_PASSWORD is set",
        );
        let mqtt_base_topic = v
            .get("MQTT_BASE_TOPIC")
            .unwrap_or_else(|| "ruuvi2mqtt".to_string());

        // BLE scanning
        let ble_scan_mode = v
            .choice("BLE_SCAN_MODE", &["continuous", "deep-sleep"])
            .unwrap_or_else(|| "continuous".to_string());
        let ble_scan_continuous = ble_scan_mode == "continuous";
        let ble_scan_duration = v
            .number("BLE_SCAN_DURATION")
            .unwrap_or(if ble_scan_continuous { 60 } else { 5 });
        v.check(
            ble_scan_duration > 0 && ble_scan_duration <= i32::MAX / 1000,
            "BLE_SCAN_DURATION must be positive (seconds)",
        );
        let ble_sleep_duration = v.number("BLE_SLEEP_DURATION").unwrap_or(60);
        v.check(
            ble_sleep_duration > 0,
            "BLE_SLEEP_DURATION must be positive (seconds)",
        );
        let usb_stay_awake = v.bool("USB_STAY_AWAKE", true);
        v.check(
            usb_stay_awake || !ble_scan_continuous,
            "USB_STAY_AWAKE=false needs BLE_SCAN_MODE=deep-sleep",
        );

        // Battery
        let battery_gauge =
            v.choice("BATTERY", &["max17048", "m5pm1"])
                .map(|gauge| match gauge.as_str() {
                    "max17048" => Gauge::Max17048,
                    _ => Gauge::M5pm1,
                });
        let battery_set = v.get("BATTERY").is_some();
        let battery_sda = v.number::<u8>("BATTERY_SDA");
        let battery_scl = v.number::<u8>("BATTERY_SCL");
        let battery_power_gpio = v.number::<u8>("BATTERY_POWER_GPIO");
        v.check(
            !battery_set || (battery_sda.is_some() && battery_scl.is_some()),
            "BATTERY_SDA and BATTERY_SCL must be set with BATTERY",
        );
        v.check(
            battery_set
                || (battery_sda.is_none() && battery_scl.is_none() && battery_power_gpio.is_none()),
            "BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO are only for BATTERY",
        );

        // Antenna
        let antenna = v.choice("ANTENNA", &["internal", "external"]);
        v.check(
            antenna.is_none() || chip.name == "esp32c6",
            "ANTENNA is only for the Seeed XIAO ESP32-C6",
        );
        let antenna_external = antenna.as_deref() == Some("external");

        // LED
        let led_gpio = v.number::<u32>("LED_GPIO");
        let led_mode = v
            .choice("LED_MODE", &["on", "errors", "off"])
            .or_else(|| led_gpio.map(|_| "on".to_string()));
        let led_type = v
            .choice("LED_TYPE", &["ws2812", "gpio"])
            .unwrap_or_else(|| "ws2812".to_string());
        v.check(
            led_mode.is_none() || led_gpio.is_some(),
            "LED_GPIO must be set when LED_MODE is set",
        );
        let led_power_gpio = v.number::<u32>("LED_POWER_GPIO");
        v.check(
            led_power_gpio.is_none() || led_gpio.is_some(),
            "LED_POWER_GPIO is only for an LED with LED_GPIO",
        );
        let led_active_low = v.bool("LED_ACTIVE_LOW", false);
        v.check(
            !led_active_low || led_type == "gpio",
            "LED_ACTIVE_LOW is only for LED_TYPE=gpio",
        );
        let led_color_hex = v.get("LED_COLOR").unwrap_or_else(|| "000020".to_string());
        let led_color = if led_color_hex.len() == 6 {
            u32::from_str_radix(&led_color_hex, 16).ok()
        } else {
            None
        };
        v.check(
            led_color.is_some(),
            "LED_COLOR must be six hex digits, RRGGBB",
        );

        // Button
        let button_gpio = v.number::<u32>("BUTTON_GPIO");
        v.check(
            button_gpio.is_none() || led_gpio.is_some(),
            "BUTTON_GPIO needs LED_GPIO: the LED shows what the button does",
        );

        // The pins
        for (name, pin) in [
            ("LED_GPIO", led_gpio),
            ("LED_POWER_GPIO", led_power_gpio),
            ("BUTTON_GPIO", button_gpio),
            ("BATTERY_SDA", battery_sda.map(u32::from)),
            ("BATTERY_SCL", battery_scl.map(u32::from)),
            ("BATTERY_POWER_GPIO", battery_power_gpio.map(u32::from)),
        ] {
            if pin.is_some_and(|pin| pin >= chip.gpio_count) {
                v.error(format!(
                    "{name} is not a valid GPIO number for the {}: it must be less than {}",
                    chip.name, chip.gpio_count
                ));
            }
        }
        v.check(
            led_power_gpio.is_none() || led_power_gpio != led_gpio,
            "LED_POWER_GPIO must differ from LED_GPIO",
        );
        // The LED and the gauge may share only their power pin.
        let battery_pins = [battery_sda, battery_scl, battery_power_gpio].map(|p| p.map(u32::from));
        v.check(
            pins_differ(&battery_pins)
                && battery_pins.iter().flatten().all(|&pin| Some(pin) != led_gpio)
                && battery_pins[..2]
                    .iter()
                    .flatten()
                    .all(|&pin| Some(pin) != led_power_gpio),
            "BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO must differ from each other, and not be LED_GPIO. Only BATTERY_POWER_GPIO may be LED_POWER_GPIO",
        );
        if let Some(button) = button_gpio {
            let antenna_pins = if antenna.is_some() {
                [Some(ANTENNA_SWITCH_POWER), Some(ANTENNA_SELECT)]
            } else {
                [None, None]
            };
            let others = [led_gpio, led_power_gpio]
                .into_iter()
                .chain(battery_pins)
                .chain(antenna_pins);
            v.check(
                others.flatten().all(|pin| pin != button),
                "BUTTON_GPIO must differ from LED_GPIO, LED_POWER_GPIO, BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO, and with ANTENNA from the RF switch's GPIO 3 and 14",
            );
        }

        // Logging
        let log_level = v
            .choice(
                "LOG_LEVEL",
                &["trace", "debug", "info", "warn", "error", "off"],
            )
            .and_then(|level| level.parse().ok())
            .unwrap_or(log::LevelFilter::Info);

        if !v.errors.is_empty() {
            return Err(v.errors);
        }
        Ok(Config {
            wifi_ssid,
            wifi_pass,
            wifi_open,
            wifi_ip,
            wifi_gateway,
            wifi_netmask,
            wifi_dns,
            wifi_band,
            wifi_scans_5g,
            device_hostname,
            mqtt_server,
            mqtt_tls,
            mqtt_ca,
            mqtt_tls_insecure,
            mqtt_port,
            mqtt_client_id,
            mqtt_user,
            mqtt_password,
            mqtt_base_topic,
            ble_scan_mode,
            ble_scan_continuous,
            ble_scan_duration,
            ble_sleep_duration,
            usb_stay_awake,
            battery_gauge,
            battery_sda,
            battery_scl,
            battery_power_gpio,
            antenna,
            antenna_external,
            led_mode,
            led_type,
            led_active_low,
            led_gpio,
            led_power_gpio,
            led_color: led_color.unwrap_or_default(),
            button_gpio,
            log_level,
        })
    }
}

/// True when the set pins differ from each other.
fn pins_differ(pins: &[Option<u32>]) -> bool {
    let set: Vec<u32> = pins.iter().flatten().copied().collect();
    set.iter()
        .enumerate()
        .all(|(i, pin)| !set[i + 1..].contains(pin))
}

/// True for a WPA2 passphrase of 8 to 63 characters, or a key of 64 hex
/// digits.
fn is_wpa2_key(v: &str) -> bool {
    (8..=63).contains(&v.len()) || (v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const C6: &Chip = Chip::find("esp32c6").unwrap();
    const C5: &Chip = Chip::find("esp32c5").unwrap();

    /// The smallest valid values, plus `extra`. An empty value in `extra`
    /// removes the name.
    fn values(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut values: BTreeMap<String, String> = [
            ("CHIP", "esp32c6"),
            ("WIFI_SSID", "example"),
            ("WIFI_PASS", "password"),
            ("MQTT_SERVER", "broker.example"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        for (name, value) in extra {
            values.insert((*name).to_string(), (*value).to_string());
        }
        values
    }

    fn errors(extra: &[(&str, &str)]) -> Vec<String> {
        Config::parse(&values(extra), C6).unwrap_err()
    }

    #[test]
    fn defaults() {
        let config = Config::parse(&values(&[]), C6).unwrap();
        assert_eq!(config.device_hostname, "ruuvi2mqtt-esp32");
        assert_eq!(config.mqtt_client_id, "ruuvi2mqtt-esp32");
        assert_eq!(config.mqtt_port, 1883);
        assert_eq!(config.mqtt_base_topic, "ruuvi2mqtt");
        assert!(config.ble_scan_continuous);
        assert_eq!(config.ble_scan_duration, 60);
        assert_eq!(config.ble_sleep_duration, 60);
        assert!(config.usb_stay_awake);
        assert_eq!(config.led_mode, None);
        assert_eq!(config.led_type, "ws2812");
        assert_eq!(config.led_color, 0x00_0020);
        assert_eq!(config.log_level, log::LevelFilter::Info);
        assert!(!config.wifi_scans_5g);
    }

    #[test]
    fn derived_defaults() {
        let config = Config::parse(
            &values(&[
                ("DEVICE_HOSTNAME", "kitchen"),
                ("MQTT_TLS", "true"),
                ("BLE_SCAN_MODE", "deep-sleep"),
                ("LED_GPIO", "8"),
                ("LED_COLOR", "ff8000"),
                ("LOG_LEVEL", "debug"),
            ]),
            C6,
        )
        .unwrap();
        assert_eq!(config.mqtt_client_id, "kitchen");
        assert_eq!(config.mqtt_port, 8883);
        assert!(!config.ble_scan_continuous);
        assert_eq!(config.ble_scan_duration, 5);
        assert_eq!(config.led_mode.as_deref(), Some("on"));
        assert_eq!(config.led_color, 0xFF_8000);
        assert_eq!(config.log_level, log::LevelFilter::Debug);
    }

    #[test]
    fn an_empty_value_is_unset() {
        let config = Config::parse(&values(&[("WIFI_IP", "")]), C6).unwrap();
        assert_eq!(config.wifi_ip, None);
    }

    #[test]
    fn mqtt_ca() {
        let pem = "-----BEGIN CERTIFICATE-----\nAB\n";
        let config = Config::parse(&values(&[("MQTT_CA", pem)]), C6).unwrap();
        assert_eq!(config.mqtt_ca.unwrap().to_bytes(), pem.as_bytes());
        for bad in ["AB\n", "-----BEGIN CERTIFICATE-----\0"] {
            assert_eq!(
                errors(&[("MQTT_CA", bad)]),
                ["MQTT_CA_FILE must be a PEM certificate"]
            );
        }
    }

    #[test]
    fn an_empty_credential_is_an_error() {
        assert_eq!(errors(&[("MQTT_USER", "")]), ["MQTT_USER is set but empty"]);
    }

    #[test]
    fn the_chip_must_match() {
        assert_eq!(
            Config::parse(&values(&[]), C5).unwrap_err(),
            ["The configuration is for an esp32c6, but this board is an esp32c5"]
        );
        let mut without = values(&[]);
        without.remove("CHIP");
        assert_eq!(
            Config::parse(&without, C6).unwrap_err(),
            ["CHIP is missing"]
        );
    }

    #[test]
    fn reports_every_error() {
        assert_eq!(
            errors(&[
                ("WIFI_PASS", "short"),
                ("MQTT_PORT", "x"),
                ("LED_MODE", "blink")
            ]),
            [
                "WIFI_PASS must be 8 to 63 characters or 64 hex digits",
                "MQTT_PORT must be a number: invalid digit found in string",
                "LED_MODE must be on, errors or off",
            ]
        );
    }

    #[test]
    fn wifi_rules() {
        let mut open = values(&[("WIFI_OPEN", "true")]);
        open.remove("WIFI_PASS");
        assert!(Config::parse(&open, C6).is_ok());
        assert_eq!(
            errors(&[("WIFI_OPEN", "true")]),
            ["WIFI_PASS must be unset when WIFI_OPEN=true"]
        );
        assert_eq!(
            errors(&[("WIFI_IP", "192.168.1.300")]),
            [
                "WIFI_GATEWAY must be set when WIFI_IP is set",
                "WIFI_IP is not a valid IPv4 address"
            ]
        );
        assert!(Config::parse(&values(&[("WIFI_PASS", &"a".repeat(64))]), C6).is_ok());
        assert_eq!(
            errors(&[("WIFI_PASS", &"g".repeat(64))]),
            ["WIFI_PASS must be 8 to 63 characters or 64 hex digits"]
        );
    }

    #[test]
    fn wifi_band_needs_a_dual_band_chip() {
        assert_eq!(
            errors(&[("WIFI_BAND", "5")]),
            ["WIFI_BAND is only for dual-band chips such as the ESP32-C5"]
        );
        let c5 = |extra: &[(&str, &str)]| {
            let mut values = values(extra);
            values.insert("CHIP".to_string(), "esp32c5".to_string());
            Config::parse(&values, C5).unwrap()
        };
        assert!(c5(&[]).wifi_scans_5g);
        assert!(!c5(&[("WIFI_BAND", "2.4")]).wifi_scans_5g);
    }

    #[test]
    fn pin_rules() {
        assert_eq!(
            errors(&[("LED_GPIO", "31")]),
            ["LED_GPIO is not a valid GPIO number for the esp32c6: it must be less than 31"]
        );
        assert_eq!(
            errors(&[("LED_MODE", "on")]),
            ["LED_GPIO must be set when LED_MODE is set"]
        );
        assert_eq!(
            errors(&[("LED_GPIO", "8"), ("BUTTON_GPIO", "8")]),
            ["BUTTON_GPIO must differ from LED_GPIO, LED_POWER_GPIO, BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO, and with ANTENNA from the RF switch's GPIO 3 and 14"]
        );
        assert!(Config::parse(&values(&[("LED_GPIO", "8"), ("BUTTON_GPIO", "3")]), C6).is_ok());
        assert_eq!(
            errors(&[
                ("LED_GPIO", "8"),
                ("BUTTON_GPIO", "3"),
                ("ANTENNA", "internal")
            ])
            .len(),
            1
        );
    }

    #[test]
    fn battery_rules() {
        assert_eq!(
            errors(&[("BATTERY", "max17048")]),
            ["BATTERY_SDA and BATTERY_SCL must be set with BATTERY"]
        );
        let shared_power = Config::parse(
            &values(&[
                ("BATTERY", "max17048"),
                ("BATTERY_SDA", "22"),
                ("BATTERY_SCL", "23"),
                ("BATTERY_POWER_GPIO", "20"),
                ("LED_GPIO", "9"),
                ("LED_POWER_GPIO", "20"),
            ]),
            C6,
        )
        .unwrap();
        assert_eq!(shared_power.battery_gauge, Some(Gauge::Max17048));
        assert_eq!(
            errors(&[
                ("BATTERY", "m5pm1"),
                ("BATTERY_SDA", "22"),
                ("BATTERY_SCL", "22"),
            ]),
            ["BATTERY_SDA, BATTERY_SCL and BATTERY_POWER_GPIO must differ from each other, and not be LED_GPIO. Only BATTERY_POWER_GPIO may be LED_POWER_GPIO"]
        );
    }

    #[test]
    fn encode_and_decode() {
        let values = values(&[("MQTT_CA", "-----BEGIN CERTIFICATE-----\nAB\n")]);
        let bytes = encode(&values).unwrap();
        assert_eq!(bytes.len(), PARTITION_SIZE);
        assert_eq!(bytes.last(), Some(&0xFF));
        assert_eq!(decode(&bytes).unwrap(), values);
    }

    #[test]
    fn decode_rejects_an_erased_partition() {
        assert_eq!(
            decode(&[0xFF; 16]).unwrap_err(),
            "The config partition is empty. Write it with `make config`"
        );
    }

    #[test]
    fn encode_rejects_too_much() {
        let values = values(&[("MQTT_CA", &"x".repeat(PARTITION_SIZE))]);
        assert!(encode(&values)
            .unwrap_err()
            .starts_with("The configuration is"));
    }

    #[test]
    fn chips() {
        assert_eq!(Chip::find("esp32s3").unwrap().gpio_count, 49);
        assert_eq!(Chip::find("esp32h2"), None);
    }
}
