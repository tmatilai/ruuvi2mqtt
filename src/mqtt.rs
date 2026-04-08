use std::time::Duration;

use std::sync::Arc;

use anyhow::{Context, Result};
use rumqttc::tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use rumqttc::tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rumqttc::tokio_rustls::rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use rumqttc::{
    AsyncClient, ConnectReturnCode, Event as MqttEvent, EventLoop as MqttEventLoop, Incoming,
    MqttOptions, QoS, TlsConfiguration, Transport,
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
    client: AsyncClient,
    state_topic_prefix: String,
}

impl Mqtt {
    pub fn init(tx: EventSender, config: &config::Mqtt) -> Result<Self> {
        let (client, eventloop) = AsyncClient::new(Self::options(config)?, 10);

        let state_topic_prefix = format!("{}/", config.base_topic);
        let event_loop_client = client.clone();
        tokio::spawn(async move {
            EventLoop::new(tx, event_loop_client, state_topic_prefix)
                .run(eventloop)
                .await;
        });

        Ok(Self { client })
    }

    fn options(config: &config::Mqtt) -> Result<MqttOptions> {
        let mut options = MqttOptions::new(&config.client_id, &config.server, config.port());
        options.set_keep_alive(Duration::from_secs(15));
        if let Some(user) = &config.user {
            let password = config
                .password
                .as_ref()
                .context("MQTT password not specified")?;
            options.set_credentials(user, password);
        }

        if !config.tls && config.ca_file.is_some() {
            log::warn!("ca_file is set but TLS is disabled; the CA file will be ignored");
        }

        if config.tls {
            let transport = if config.tls_insecure {
                log::warn!("TLS certificate verification is disabled (tls_insecure: true)");
                let tls_config = ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoVerifier))
                    .with_no_client_auth();
                Transport::Tls(TlsConfiguration::Rustls(Arc::new(tls_config)))
            } else if let Some(ca_file) = &config.ca_file {
                let ca = std::fs::read(ca_file)
                    .with_context(|| format!("Failed to read CA file: {}", ca_file.display()))?;
                Transport::Tls(TlsConfiguration::Simple {
                    ca,
                    alpn: None,
                    client_auth: None,
                })
            } else {
                let native_certs = rustls_native_certs::load_native_certs();
                if !native_certs.errors.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Failed to load platform TLS certificates: {:?}",
                        native_certs.errors
                    ));
                }
                let mut root_cert_store = RootCertStore::empty();
                for cert in native_certs.certs {
                    root_cert_store
                        .add(cert)
                        .context("Failed to add platform TLS certificate")?;
                }
                let tls_config = ClientConfig::builder()
                    .with_root_certificates(root_cert_store)
                    .with_no_client_auth();
                Transport::Tls(TlsConfiguration::Rustls(Arc::new(tls_config)))
            };
            options.set_transport(transport);
        }

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

#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rumqttc::tokio_rustls::rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rumqttc::tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rumqttc::tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

impl EventLoop {
    pub fn new(tx: EventSender, client: AsyncClient, state_topic_prefix: String) -> Self {
        Self {
            tx,
            client,
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
                    let topic = format!("{}#", self.state_topic_prefix);
                    log::debug!("Subscribing to: {topic}");
                    if let Err(err) = self.client.subscribe(&topic, QoS::AtMostOnce).await {
                        log::error!("Failed to subscribe: {err}");
                    }
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
