mod config;
mod devices;
mod homeassistant;
mod mqtt;
mod ruuvi;

use anyhow::{Result, bail};
use tokio::sync::mpsc;

use crate::config::{CliOptions, Config};
use crate::devices::{Devices, ThrottleResult};
use crate::homeassistant::SensorData;
use crate::mqtt::Mqtt;
use crate::ruuvi::{BDAddr, FormatFilter, RuuviListener};

type EventSender = mpsc::Sender<crate::Event>;

#[derive(Debug)]
pub enum Event {
    RuuviUpdate(ruuvi::SensorData),
    MqttDeviceUpdate(BDAddr),
    MqttConnect,
    BleStopped,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = CliOptions::read();

    init_logger(options.log_level);

    log::info!("{}", config::version_info().trim_end());
    log::debug!("{options:?}");

    let config = Config::load(&options)?;
    log::debug!("{config:?}");

    if options.check_config {
        Mqtt::check(&config.mqtt)?;
        println!("{}: OK", options.config.display());
        return Ok(());
    }

    let mut devices = Devices::new(&config.devices, config.mqtt.throttle);
    let mut formats = FormatFilter::default();

    let (tx, mut rx) = mpsc::channel(32);
    let mut mqtt = Mqtt::init(tx.clone(), &config.mqtt)?;
    RuuviListener::new(tx).await?.start().await?;

    while let Some(event) = rx.recv().await {
        use Event::{BleStopped, MqttConnect, MqttDeviceUpdate, RuuviUpdate};

        log::trace!("Received event: {event:?}");
        match event {
            MqttConnect => {
                log::info!("Connected to Mqtt. Publishing status and devices.");
                mqtt.publish_status();
                let instance = homeassistant::Instance::new(&config.mqtt);
                mqtt.publish_discovery(instance.topic.clone(), &instance);
                for device in homeassistant::Device::all(&config) {
                    mqtt.publish_discovery(device.topic.clone(), &device);
                }
                for topic in homeassistant::stale_topics(&config) {
                    mqtt.clear_retained(topic);
                }
            }
            MqttDeviceUpdate(bdaddr) => {
                if let Some(device) = devices.mark_published(&bdaddr) {
                    log::debug!("Updated from Mqtt: '{}' [{}]", device.name, bdaddr);
                }
            }
            RuuviUpdate(sensor) => {
                if !formats.keep(&sensor) {
                    log::debug!("Dropped format 6 after E1: [{}]", sensor.bdaddr);
                    continue;
                }
                let result = devices.try_publish(&sensor.bdaddr);
                let device_name = devices.get(&sensor.bdaddr).map(|d| d.name.as_str());
                match result {
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
                        let data = SensorData::new(&sensor, &config.mqtt);
                        mqtt.publish_sensor_data(data);
                    }
                }
            }
            BleStopped => bail!("BLE scanning stopped; exiting"),
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
