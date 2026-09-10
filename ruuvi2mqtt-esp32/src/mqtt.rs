use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context};
use esp_idf_svc::mqtt::client::{
    EspMqttClient, EspMqttConnection, EventPayload, MessageId, MqttClientConfiguration, QoS,
};
use esp_idf_svc::tls::X509;
use log::info;

use crate::config;

/// Events forwarded from the event-loop thread to [`Mqtt`].
enum Event {
    Connected,
    Published(MessageId),
}

/// MQTT client with its event loop running in a background thread.
pub struct Mqtt {
    client: EspMqttClient<'static>,
    events: mpsc::Receiver<Event>,
    connected: bool,
    /// Publishes not yet acknowledged by the broker.
    pending: Vec<MessageId>,
}

impl Mqtt {
    /// Create the client and start connecting in the background.
    ///
    /// Call [`Mqtt::wait_connected`] before publishing.
    pub fn connect() -> anyhow::Result<Self> {
        let scheme = if config::MQTT_TLS { "mqtts" } else { "mqtt" };
        let broker_url = format!("{scheme}://{}:{}", config::MQTT_SERVER, config::MQTT_PORT);
        let client_id = config::MQTT_CLIENT_ID.unwrap_or(config::DEVICE_HOSTNAME);

        if config::MQTT_TLS_INSECURE {
            log::warn!("TLS certificate verification is weakened (MQTT_TLS_INSECURE=true)");
        }

        let server_certificate = ca_certificate();

        let cfg = MqttClientConfiguration {
            client_id: Some(client_id),
            username: if config::MQTT_USER.is_empty() {
                None
            } else {
                Some(config::MQTT_USER)
            },
            password: if config::MQTT_PASSWORD.is_empty() {
                None
            } else {
                Some(config::MQTT_PASSWORD)
            },
            keep_alive_interval: Some(std::time::Duration::from_secs(15)),
            server_certificate,
            crt_bundle_attach: crt_bundle_attach(),
            skip_cert_common_name_check: config::MQTT_TLS_INSECURE,
            ..Default::default()
        };

        let (client, connection) = EspMqttClient::new(&broker_url, &cfg)?;
        info!("Connecting to MQTT: {broker_url}");

        let (tx, events) = mpsc::channel();
        thread::Builder::new()
            .stack_size(8192)
            .spawn(move || run_event_loop(connection, &tx))
            .context("Failed to spawn MQTT event-loop thread")?;

        Ok(Self {
            client,
            events,
            connected: false,
            pending: Vec::new(),
        })
    }

    /// Wait until the broker has accepted the connection.
    pub fn wait_connected(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let connected = self.wait(timeout, |mqtt| mqtt.connected)?;
        ensure!(
            connected,
            "MQTT connect timed out after {}s",
            timeout.as_secs()
        );
        Ok(())
    }

    /// Publish `payload` at least once. Acknowledgement is checked in
    /// [`Mqtt::wait_published`].
    pub fn publish(&mut self, topic: &str, payload: &str) -> anyhow::Result<()> {
        let id = self
            .client
            .publish(topic, QoS::AtLeastOnce, false, payload.as_bytes())?;
        self.pending.push(id);
        Ok(())
    }

    /// Wait until the broker has acknowledged all publishes.
    pub fn wait_published(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let published = self.wait(timeout, |mqtt| mqtt.pending.is_empty())?;
        ensure!(
            published,
            "{} publishes not acknowledged within {}s",
            self.pending.len(),
            timeout.as_secs()
        );
        Ok(())
    }

    /// Process events until `done` holds. Returns `false` on timeout.
    fn wait(&mut self, timeout: Duration, done: impl Fn(&Self) -> bool) -> anyhow::Result<bool> {
        let deadline = Instant::now() + timeout;
        while !done(self) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(remaining) {
                Ok(Event::Connected) => self.connected = true,
                Ok(Event::Published(id)) => self.pending.retain(|&p| p != id),
                Err(RecvTimeoutError::Timeout) => return Ok(false),
                Err(RecvTimeoutError::Disconnected) => bail!("MQTT event loop ended"),
            }
        }
        Ok(true)
    }
}

/// Return the embedded CA certificate, if `MQTT_CA_FILE` was set at build time.
fn ca_certificate() -> Option<X509<'static>> {
    #[cfg(not(mqtt_ca_file))]
    {
        None
    }
    #[cfg(mqtt_ca_file)]
    {
        static CA_PEM: &[u8] = include_bytes!(env!("MQTT_CA_PEM_PATH"));
        Some(X509::pem_until_nul(CA_PEM))
    }
}

/// Use the ESP-IDF built-in CA certificate bundle when TLS is enabled
/// without a custom CA certificate.
fn crt_bundle_attach(
) -> Option<unsafe extern "C" fn(conf: *mut core::ffi::c_void) -> esp_idf_svc::sys::esp_err_t> {
    if config::MQTT_TLS && config::MQTT_CA_FILE.is_none() {
        Some(esp_idf_svc::sys::esp_crt_bundle_attach)
    } else {
        None
    }
}

/// Drive the MQTT event loop to keep the connection alive, and forward the
/// events [`Mqtt`] waits for.
fn run_event_loop(mut connection: EspMqttConnection, events: &mpsc::Sender<Event>) {
    while let Ok(event) = connection.next() {
        let event = match event.payload() {
            EventPayload::Connected(_) => {
                info!("Connected to MQTT");
                Event::Connected
            }
            EventPayload::Published(id) => Event::Published(id),
            other => {
                log::debug!("MQTT event: {other:?}");
                continue;
            }
        };
        // The receiver is dropped when the cycle ends; nothing to do then.
        let _ = events.send(event);
    }
    log::warn!("MQTT event loop ended");
}
