use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use derive_more::Debug;
use rand::Rng;
use serde::Deserialize;
use serde_with::{formats::Flexible, serde_as, DisplayFromStr, DurationSeconds};
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
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    pub user: Option<String>,
    #[debug("{}", fmt_secret(password.as_ref()))]
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
        serde_yaml::from_str(&config_str)
            .with_context(|| format!("Failed to load {}", config_file.display()))
    }
}

impl CliOptions {
    pub fn read() -> Self {
        Self::parse()
    }
}

pub fn version_info() -> String {
    CliOptions::command().render_long_version()
}

const fn default_mqtt_port() -> u16 {
    1883
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

fn fmt_secret(value: Option<&String>) -> &str {
    match value {
        None => "None",
        Some(_) => "Some(<REDACTED>)",
    }
}

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    CliOptions::command().debug_assert();
}
