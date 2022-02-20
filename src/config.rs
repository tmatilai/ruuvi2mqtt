use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use derivative::Derivative;
use rand::Rng;
use serde::Deserialize;
use serde_with::{formats::Flexible, serde_as, DisplayFromStr, DurationSeconds};
use sysinfo::{System, SystemExt};

use crate::ruuvi::BDAddr;

#[serde_as]
#[derive(Debug, Deserialize)]
pub struct Config {
    pub mqtt: Mqtt,
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub devices: HashMap<BDAddr, Device>,
}

#[serde_as]
#[derive(Derivative, Deserialize)]
#[derivative(Debug)]
pub struct Mqtt {
    pub server: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    pub user: Option<String>,
    #[derivative(Debug(format_with = "fmt_secret"))]
    pub password: Option<String>,
    #[serde(default = "default_mqtt_client_id")]
    pub client_id: String,
    #[serde_as(as = "DurationSeconds<u32, Flexible>")]
    #[serde(default = "default_mqtt_throttle")]
    pub throttle: Duration,
    #[serde(default = "default_mqtt_base_topic")]
    pub base_topic: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub name: String,
}

impl Config {
    pub fn config_file() -> String {
        env::var("CONFIG_FILE").unwrap_or_else(|_| "ruuvi2mqtt.yaml".to_string())
    }

    pub fn load() -> Result<Self> {
        let config_file = Self::config_file();
        log::debug!("Config file: {}", config_file);

        let config_str = fs::read_to_string(&config_file)
            .with_context(|| format!("Failed to read {}", config_file))?;
        serde_yaml::from_str(&config_str).with_context(|| format!("Failed to load {}", config_file))
    }
}

fn default_mqtt_port() -> u16 {
    1883
}

fn default_mqtt_throttle() -> Duration {
    let throttle = rand::thread_rng().gen_range(50..70);
    Duration::new(throttle, 0)
}

fn default_mqtt_client_id() -> String {
    let suffix = System::new().host_name().unwrap_or_else(|| {
        log::warn!("Failed to read hostname. Generating random suffix for the client_id.");
        format!("{:03}", rand::thread_rng().gen_range(1..1000) as u8)
    });
    format!("ruuvi2mqtt_{}", suffix)
}

fn default_mqtt_base_topic() -> String {
    String::from("ruuvi2mqtt")
}

fn fmt_secret(value: &Option<String>, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
    match value {
        None => formatter.write_str("None"),
        Some(_) => formatter.write_str("Some(<REDACTED>)"),
    }
}
