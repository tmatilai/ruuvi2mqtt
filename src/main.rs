#![allow(dead_code)]
#![allow(unused_variables)]

mod config;
mod devices;
mod homeassistant;
mod mqtt;
mod ruuvi;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::devices::{Devices, TryUpdate};
use crate::homeassistant::SensorData;
use crate::mqtt::Mqtt;
use crate::ruuvi::{BDAddr, RuuviListener};

type EventSender = mpsc::Sender<crate::Event>;

const PROGRAM: &str = concat!(env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub enum Event {
    RuuviUpdate(ruuvi::SensorData),
    MqttDeviceUpdate(BDAddr),
    MqttConnect,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logger();

    log::info!("{}", PROGRAM);

    let config = Config::load()?;
    log::debug!("{:?}", config);

    let mut devices = Devices::new(&config.devices, config.mqtt.throttle);

    let (tx, mut rx) = mpsc::channel(32);
    let mut mqtt = Mqtt::init(tx.clone(), &config.mqtt).await?;
    RuuviListener::new(tx, config.mqtt.throttle / 100)
        .await?
        .start()
        .await?;

    while let Some(event) = rx.recv().await {
        use Event::*;

        log::trace!("Received event: {:?}", event);
        match event {
            MqttConnect => {
                log::info!("Connected to Mqtt. Publishing devices.");
                for device in homeassistant::Device::all(&config) {
                    mqtt.publish_device(device).await;
                }
            }
            MqttDeviceUpdate(bdaddr) => {
                if let Some(device) = devices.update(&bdaddr) {
                    log::debug!("Updated from Mqtt: '{}' [{}]", device.name, bdaddr);
                }
            }
            RuuviUpdate(sensor) => match devices.try_update(&sensor.bdaddr) {
                TryUpdate::UnknownDevice => {
                    log::debug!("Unknown device: [{}]", sensor.bdaddr);
                }
                TryUpdate::Throttle(device) => {
                    log::debug!("Throttled: '{}' [{}]", device.name, sensor.bdaddr);
                }
                TryUpdate::Update(device) => {
                    log::info!("Updating: '{}' [{}]", device.name, sensor.bdaddr);
                    let data = SensorData::new(device.name, sensor, &config.mqtt.base_topic);
                    mqtt.publish_sensor_data(data).await;
                }
            },
        }
    }
    Ok(())
}

fn init_logger() {
    let env = env_logger::Env::default().filter_or("LOG_LEVEL", "ruuvi2mqtt=INFO");
    env_logger::init_from_env(env);
}
