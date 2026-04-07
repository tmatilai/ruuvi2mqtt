mod config;
mod devices;
mod homeassistant;
mod mqtt;
mod ruuvi;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::{CliOptions, Config};
use crate::devices::{Devices, ThrottleResult};
use crate::homeassistant::SensorData;
use crate::mqtt::Mqtt;
use crate::ruuvi::{BDAddr, RuuviListener};

type EventSender = mpsc::Sender<crate::Event>;

#[derive(Debug)]
pub enum Event {
    RuuviUpdate(ruuvi::SensorData),
    MqttDeviceUpdate(BDAddr),
    MqttConnect,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = CliOptions::read();

    init_logger(options.log_level);

    log::info!("{}", config::version_info().trim_end());
    log::debug!("{options:?}");

    let config = Config::load(&options)?;
    log::debug!("{config:?}");

    let mut devices = Devices::new(&config.devices, config.mqtt.throttle);

    let (tx, mut rx) = mpsc::channel(32);
    let mut mqtt = Mqtt::init(tx.clone(), &config.mqtt)?;
    RuuviListener::new(tx, config.mqtt.throttle / 100)
        .await?
        .start()
        .await?;

    while let Some(event) = rx.recv().await {
        use Event::{MqttConnect, MqttDeviceUpdate, RuuviUpdate};

        log::trace!("Received event: {event:?}");
        match event {
            MqttConnect => {
                log::info!("Connected to Mqtt. Publishing devices.");
                for device in homeassistant::Device::all(&config) {
                    mqtt.publish_device(device);
                }
            }
            MqttDeviceUpdate(bdaddr) => {
                if let Some(device) = devices.mark_published(&bdaddr) {
                    log::debug!("Updated from Mqtt: '{}' [{}]", device.name, bdaddr);
                }
            }
            RuuviUpdate(sensor) => {
                let device_name = devices.get(&sensor.bdaddr).map(|d| d.name.as_str());
                match devices.should_publish(&sensor.bdaddr) {
                    ThrottleResult::UnknownDevice => {
                        log::debug!("Unknown device: [{}]", sensor.bdaddr);
                    }
                    ThrottleResult::Throttle => {
                        log::debug!(
                            "Throttled: '{}' [{}]",
                            device_name.unwrap_or("?"),
                            sensor.bdaddr,
                        );
                    }
                    ThrottleResult::Update => {
                        log::info!(
                            "Updating: '{}' [{}]",
                            device_name.unwrap_or("?"),
                            sensor.bdaddr,
                        );
                        let data = SensorData::new(&sensor, &config.mqtt.base_topic);
                        mqtt.publish_sensor_data(data);
                    }
                }
            }
        }
    }
    Ok(())
}

fn init_logger(log_level: log::LevelFilter) {
    env_logger::Builder::new()
        .filter_level(log_level)
        .parse_default_env()
        .init();
}
