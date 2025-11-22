use std::time::Duration;

use anyhow::{Context, Result};
use rumqttc::{
    AsyncClient, ConnectReturnCode, Event as MqttEvent, EventLoop as MqttEventLoop, Incoming,
    MqttOptions, QoS,
};
use tokio::time::sleep;

use crate::Event::{self, MqttConnect, MqttDeviceUpdate};
use crate::EventSender;
use crate::config;
use crate::homeassistant::{Device, SensorData};
use crate::ruuvi::BDAddr;

pub struct Mqtt {
    client: AsyncClient,
}

#[derive(Clone)]
pub struct EventLoop {
    tx: EventSender,
    state_topic_prefix: String,
}

impl Mqtt {
    pub async fn init(tx: EventSender, config: &config::Mqtt) -> Result<Self> {
        let (client, eventloop) = AsyncClient::new(Self::options(config)?, 10);

        let state_topic_prefix = format!("{}/", config.base_topic);
        tokio::spawn(async move {
            EventLoop::new(tx, state_topic_prefix).run(eventloop).await;
        });

        let state_topic = format!("{}/#", config.base_topic);
        log::debug!("Subscribing to: {state_topic}");
        client
            .subscribe(state_topic, QoS::AtMostOnce)
            .await
            .context("Failed to subscribe")?;

        Ok(Self { client })
    }

    fn options(config: &config::Mqtt) -> Result<MqttOptions> {
        let mut options = MqttOptions::new(&config.client_id, &config.server, config.port);
        options.set_keep_alive(Duration::from_secs(15));
        if let Some(user) = &config.user {
            let password = config
                .password
                .as_ref()
                .context("MQTT password not specified")?;
            options.set_credentials(user, password);
        }

        // TODO: TLS

        Ok(options)
    }

    pub fn publish_device(&mut self, device: Device<'static>) {
        let client = self.client.clone();
        tokio::spawn(async move {
            log::debug!("Publishing: {} -> {:?}", device.topic, device);
            let payload = serde_json::to_vec(&device).unwrap();
            match client
                .publish(device.topic, QoS::AtLeastOnce, true, payload)
                .await
            {
                Ok(()) => log::trace!("OK!"),
                Err(err) => log::error!("Failed to publish: {err}"),
            }
        });
    }

    pub fn publish_sensor_data(&mut self, data: SensorData) {
        let client = self.client.clone();
        tokio::spawn(async move {
            log::debug!("Publishing: {} -> {:?}", data.topic, data);
            let payload = serde_json::to_vec(&data).unwrap();
            match client
                .publish(data.topic, QoS::AtLeastOnce, false, payload)
                .await
            {
                Ok(()) => log::trace!("OK!"),
                Err(err) => log::error!("Failed to publish: {err}"),
            }
        });
    }
}

impl EventLoop {
    pub const fn new(tx: EventSender, state_topic_prefix: String) -> Self {
        Self {
            tx,
            state_topic_prefix,
        }
    }

    pub async fn run(self, mut eventloop: MqttEventLoop) {
        log::info!("Starting MQTT evenloop");
        loop {
            match eventloop.poll().await {
                Ok(event) => {
                    let e = self.clone();
                    tokio::spawn(async move {
                        log::trace!("Event: {event:?}");
                        e.on_event(event).await;
                    });
                }
                Err(err) => {
                    log::error!("Eventloop error: {err}");
                    sleep(Duration::from_secs(10)).await;
                }
            }
        }
    }

    async fn on_event(self, event: MqttEvent) {
        match event {
            MqttEvent::Incoming(Incoming::ConnAck(conn)) => {
                if conn.code == ConnectReturnCode::Success {
                    self.send_event(MqttConnect).await;
                }
            }
            MqttEvent::Incoming(Incoming::Publish(msg)) => {
                if let Some(suffix) = msg.topic.strip_prefix(&self.state_topic_prefix)
                    && let Ok(bdaddr) = BDAddr::from_str_no_delim(suffix)
                {
                    self.send_event(MqttDeviceUpdate(bdaddr)).await;
                }
            }
            _ => {}
        }
    }

    async fn send_event(&self, event: Event) {
        self.tx.send(event).await.expect("Failed to send event");
    }
}
