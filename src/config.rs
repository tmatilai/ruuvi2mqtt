use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};
use derive_more::Debug;
use rand::RngExt;
use serde::Deserialize;
use serde_with::{DisplayFromStr, DurationSeconds, formats::Flexible, serde_as};
use sysinfo::System;

use crate::ruuvi::BDAddr;

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct Config {
    pub mqtt: Mqtt,
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub devices: HashMap<BDAddr, Device>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct Mqtt {
    pub server: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub tls_insecure: bool,
    pub ca_file: Option<PathBuf>,
    pub user: Option<String>,
    #[debug("{}", fmt_secret(password.as_ref()))]
    pub password: Option<String>,
    #[serde(default = "default_mqtt_client_id")]
    pub client_id: String,
    #[serde(default = "default_mqtt_base_topic")]
    pub base_topic: String,
    #[serde_as(as = "DurationSeconds<u32, Flexible>")]
    #[serde(default = "default_mqtt_throttle")]
    pub throttle: Duration,
}

impl Mqtt {
    pub fn port(&self) -> u16 {
        self.port.unwrap_or(if self.tls { 8883 } else { 1883 })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub name: String,
}

#[derive(Debug, Parser)]
#[command(version)]
pub struct CliOptions {
    /// Configuration file
    #[arg(long, env = "CONFIG_FILE", default_value = "ruuvi2mqtt.yaml")]
    pub config: PathBuf,
    #[arg(long, env, default_value = "INFO")]
    pub log_level: log::LevelFilter,
}

impl Config {
    pub fn load(options: &CliOptions) -> Result<Self> {
        let config_file = &options.config;

        let config_str = fs::read_to_string(config_file)
            .with_context(|| format!("Failed to read {}", config_file.display()))?;
        warn_if_readable_by_others(config_file);
        let mut config: Self = serde_norway::from_str(&config_str)
            .with_context(|| format!("Failed to load {}", config_file.display()))?;
        config.mqtt.base_topic = validate_base_topic(&config.mqtt.base_topic)
            .with_context(|| format!("Invalid mqtt.base_topic in {}", config_file.display()))?;
        Ok(config)
    }
}

/// The base topic is used both for publishing and for a `<base_topic>/#`
/// subscription, so wildcards and a leading `$` would break it.
fn validate_base_topic(topic: &str) -> Result<String> {
    let topic = topic.trim_end_matches('/');
    if topic.is_empty() {
        bail!("must not be empty");
    }
    if topic.contains(['#', '+']) {
        bail!("must not contain wildcards ('#' or '+')");
    }
    if topic.starts_with('$') {
        bail!("must not start with '$'");
    }
    Ok(topic.to_string())
}

impl CliOptions {
    pub fn read() -> Self {
        Self::parse()
    }
}

pub fn version_info() -> String {
    CliOptions::command().render_long_version()
}

/// The config file may contain the MQTT password, so warn if other users can read it.
#[cfg(unix)]
fn warn_if_readable_by_others(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = fs::metadata(path)
        && is_readable_by_others(metadata.permissions().mode())
    {
        log::warn!(
            "{} is readable by other users; consider restricting it with `chmod 600`",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn warn_if_readable_by_others(_path: &Path) {}

#[cfg(unix)]
const fn is_readable_by_others(mode: u32) -> bool {
    mode & 0o044 != 0
}

fn default_mqtt_throttle() -> Duration {
    let throttle = rand::rng().random_range(50..70);
    Duration::new(throttle, 0)
}

fn default_mqtt_client_id() -> String {
    let suffix = System::host_name().unwrap_or_else(|| {
        log::warn!("Failed to read hostname. Generating random suffix for the client_id.");
        format!("{:03}", rand::rng().random::<u8>())
    });
    format!("ruuvi2mqtt_{suffix}")
}

fn default_mqtt_base_topic() -> String {
    String::from("ruuvi2mqtt")
}

#[allow(dead_code)] // used via derive_more #[debug(...)] attribute
fn fmt_secret(value: Option<&String>) -> &str {
    match value {
        None => "None",
        Some(_) => "Some(<REDACTED>)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        CliOptions::command().debug_assert();
    }

    #[cfg(unix)]
    #[test]
    fn readable_by_others() {
        assert!(!is_readable_by_others(0o600));
        assert!(!is_readable_by_others(0o100_600));
        assert!(is_readable_by_others(0o640));
        assert!(is_readable_by_others(0o604));
        assert!(is_readable_by_others(0o100_644));
    }

    #[test]
    fn base_topic_valid() {
        assert_eq!(validate_base_topic("ruuvi2mqtt").unwrap(), "ruuvi2mqtt");
        assert_eq!(validate_base_topic("home/ruuvi").unwrap(), "home/ruuvi");
        assert_eq!(validate_base_topic("ruuvi/").unwrap(), "ruuvi");
        assert_eq!(validate_base_topic("ruuvi//").unwrap(), "ruuvi");
        assert_eq!(validate_base_topic("a$b").unwrap(), "a$b");
    }

    #[test]
    fn base_topic_invalid() {
        assert!(validate_base_topic("").is_err());
        assert!(validate_base_topic("/").is_err());
        assert!(validate_base_topic("ruuvi/#").is_err());
        assert!(validate_base_topic("ruuvi/+/x").is_err());
        assert!(validate_base_topic("$SYS/ruuvi").is_err());
    }

    #[test]
    fn mqtt_port_defaults_without_tls() {
        let mqtt = Mqtt {
            server: "localhost".into(),
            port: None,
            tls: false,
            tls_insecure: false,
            ca_file: None,
            user: None,
            password: None,
            client_id: "test".into(),
            base_topic: "test".into(),
            throttle: Duration::from_mins(1),
        };
        assert_eq!(mqtt.port(), 1883);
    }

    #[test]
    fn mqtt_port_defaults_with_tls() {
        let mqtt = Mqtt {
            server: "localhost".into(),
            port: None,
            tls: true,
            tls_insecure: false,
            ca_file: None,
            user: None,
            password: None,
            client_id: "test".into(),
            base_topic: "test".into(),
            throttle: Duration::from_mins(1),
        };
        assert_eq!(mqtt.port(), 8883);
    }

    #[test]
    fn mqtt_port_explicit_override() {
        let mqtt = Mqtt {
            server: "localhost".into(),
            port: Some(9999),
            tls: true,
            tls_insecure: false,
            ca_file: None,
            user: None,
            password: None,
            client_id: "test".into(),
            base_topic: "test".into(),
            throttle: Duration::from_mins(1),
        };
        assert_eq!(mqtt.port(), 9999);
    }
}
