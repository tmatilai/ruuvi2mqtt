//! The connection to the broker, for both the check and the recorder.

use std::io::BufRead;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rumqttc::{
    AsyncClient, ConnectionError, Event, EventLoop, Incoming, MqttOptions, Publish, QoS,
    TlsConfiguration, Transport,
};
use tokio::time::{Instant, sleep_until};

use crate::cli::Broker;

const RETRY: Duration = Duration::from_secs(5);

pub enum Received {
    Connected,
    Message(Publish),
    Lost(ConnectionError),
}

/// Subscribes to everything on the broker, and reconnects.
pub struct Session {
    client: AsyncClient,
    eventloop: EventLoop,
    retry_at: Option<Instant>,
}

impl Session {
    pub fn new(broker: &Broker, client_id: &str) -> Result<Self> {
        let (client, eventloop) = AsyncClient::new(mqtt_options(broker, client_id)?, 10);
        Ok(Self {
            client,
            eventloop,
            retry_at: None,
        })
    }

    /// Cancel safe, for `select!` with a timer.
    pub async fn next(&mut self) -> Result<Received> {
        if let Some(at) = self.retry_at {
            sleep_until(at).await;
            self.retry_at = None;
        }
        loop {
            match self.eventloop.poll().await {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                    self.client
                        .try_subscribe("#", QoS::AtMostOnce)
                        .context("Failed to subscribe")?;
                    return Ok(Received::Connected);
                }
                Ok(Event::Incoming(Incoming::Publish(msg))) => return Ok(Received::Message(msg)),
                Ok(_) => {}
                Err(err) => {
                    self.retry_at = Some(Instant::now() + RETRY);
                    return Ok(Received::Lost(err));
                }
            }
        }
    }
}

fn mqtt_options(broker: &Broker, client_id: &str) -> Result<MqttOptions> {
    let port = broker
        .port
        .unwrap_or(if broker.ca_file.is_some() { 8883 } else { 1883 });
    let mut options = MqttOptions::new(client_id, &broker.server, port);
    options.set_keep_alive(Duration::from_secs(15));
    // clap requires TLS with a user
    if let Some(user) = &broker.user {
        options.set_credentials(user, read_password()?);
    }
    if let Some(ca_file) = &broker.ca_file {
        let ca = std::fs::read(ca_file)
            .with_context(|| format!("Failed to read {}", ca_file.display()))?;
        options.set_transport(Transport::Tls(TlsConfiguration::Simple {
            ca,
            alpn: None,
            client_auth: None,
        }));
    }
    Ok(options)
}

fn read_password() -> Result<String> {
    let mut password = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut password)
        .context("Failed to read the password from stdin")?;
    let password = password.trim_end_matches(['\r', '\n']);
    if password.is_empty() {
        bail!("No password on stdin");
    }
    Ok(password.to_string())
}
