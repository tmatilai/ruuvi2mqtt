use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context};
use esp_idf_svc::mqtt::client::{
    Details, EspMqttClient, EspMqttConnection, EventPayload, MessageId, MqttClientConfiguration,
    QoS,
};
use esp_idf_svc::tls::X509;
use log::{info, warn};
use ruuvi2mqtt_esp32::status::DISCOVERY_TOPIC;

use crate::config;

/// Events forwarded from the event-loop thread to [`Mqtt`].
#[derive(Clone, Copy)]
enum Event {
    Connected(Instant),
    Disconnected,
    Published(MessageId),
}

/// MQTT client with its event loop running in a background thread.
pub struct Mqtt {
    client: EspMqttClient<'static>,
    events: mpsc::Receiver<Event>,
    /// When the broker accepted the connection, while it is up.
    connected_at: Option<Instant>,
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

        if config::MQTT_TLS {
            if config::MQTT_TLS_INSECURE {
                log::warn!("TLS hostname check is disabled (MQTT_TLS_INSECURE=true)");
            }
        } else if config::MQTT_CA_FILE.is_some() {
            log::warn!("MQTT_CA_FILE is set but TLS is disabled; the CA file is ignored");
        }

        let server_certificate = ca_certificate();

        let cfg = MqttClientConfiguration {
            client_id: Some(config::MQTT_CLIENT_ID),
            username: config::MQTT_USER,
            password: config::MQTT_PASSWORD,
            keep_alive_interval: Some(std::time::Duration::from_secs(15)),
            server_certificate,
            crt_bundle_attach: crt_bundle_attach(),
            skip_cert_common_name_check: config::MQTT_TLS_INSECURE,
            ..Default::default()
        };

        let (client, connection) = EspMqttClient::new(&broker_url, &cfg)?;
        info!("Connecting to MQTT: {broker_url}");
        match (config::MQTT_USER, config::MQTT_PASSWORD) {
            (Some(user), Some(_)) => info!("MQTT login: user '{user}' with a password"),
            (Some(user), None) => info!("MQTT login: user '{user}' without a password"),
            (None, _) => info!("MQTT login: anonymous"),
        }

        let (tx, events) = mpsc::channel();
        thread::Builder::new()
            .stack_size(8192)
            .spawn(move || run_event_loop(connection, &tx))
            .context("Failed to spawn MQTT event-loop thread")?;

        Ok(Self {
            client,
            events,
            connected_at: None,
            pending: Vec::new(),
        })
    }

    /// Wait until the broker has accepted the connection, and return when
    /// it did. The client reconnects by itself.
    pub fn wait_connected(&mut self, timeout: Duration) -> anyhow::Result<Instant> {
        self.wait(timeout, |mqtt| mqtt.connected_at.is_some())?;
        self.connected_at
            .with_context(|| format!("MQTT connect timed out after {}s", timeout.as_secs()))
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

    /// Wait until the broker has acknowledged all publishes. On a timeout,
    /// it stops waiting for them, so that the next wait doesn't.
    pub fn wait_published(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let published = self.wait(timeout, |mqtt| mqtt.pending.is_empty())?;
        let unacknowledged = std::mem::take(&mut self.pending).len();
        ensure!(
            published,
            "{unacknowledged} publishes not acknowledged within {}s",
            timeout.as_secs()
        );
        Ok(())
    }

    /// Process events until `done` holds. Returns `false` on timeout.
    ///
    /// It takes the queued events first, so that `done` sees the current
    /// state.
    fn wait(&mut self, timeout: Duration, done: impl Fn(&Self) -> bool) -> anyhow::Result<bool> {
        let deadline = Instant::now() + timeout;
        while let Ok(event) = self.events.try_recv() {
            self.handle(event);
        }
        while !done(self) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(remaining) {
                Ok(event) => self.handle(event),
                Err(RecvTimeoutError::Timeout) => return Ok(false),
                Err(RecvTimeoutError::Disconnected) => bail!("MQTT event loop ended"),
            }
        }
        Ok(true)
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Connected(at) => {
                self.connected_at = Some(at);
                // The tag names for the status page. A clean session
                // forgets the subscription.
                if config::BLE_SCAN_CONTINUOUS {
                    if let Err(e) = self.client.subscribe(DISCOVERY_TOPIC, QoS::AtMostOnce) {
                        warn!("Failed to subscribe to {DISCOVERY_TOPIC}: {e}");
                    }
                }
            }
            Event::Disconnected => self.connected_at = None,
            Event::Published(id) => self.pending.retain(|&p| p != id),
        }
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
                // The main thread may be waiting for the scan, so it
                // processes the event later.
                Event::Connected(Instant::now())
            }
            EventPayload::Disconnected => {
                info!("Disconnected from MQTT");
                Event::Disconnected
            }
            EventPayload::Published(id) => Event::Published(id),
            EventPayload::Received {
                topic: Some(topic),
                data,
                details: Details::Complete,
                ..
            } => {
                crate::web::discovery(topic, data);
                continue;
            }
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
