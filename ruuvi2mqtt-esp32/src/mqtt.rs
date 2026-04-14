use esp_idf_svc::mqtt::client::{
    EspMqttClient, EspMqttConnection, EventPayload, MqttClientConfiguration, QoS,
};
use esp_idf_svc::tls::X509;
use log::info;

use crate::config;

/// Create an MQTT client and its event-loop connection handle.
///
/// Call [`run_event_loop`] in a separate thread to keep the connection alive.
pub fn connect() -> anyhow::Result<(EspMqttClient<'static>, EspMqttConnection)> {
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
    Ok((client, connection))
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

/// Drive the MQTT event loop to keep the connection alive.
///
/// Must run in its own thread.
pub fn run_event_loop(mut connection: EspMqttConnection) {
    while let Ok(event) = connection.next() {
        match event.payload() {
            EventPayload::Connected(_) => {
                info!("Connected to MQTT");
            }
            other => {
                log::debug!("MQTT event: {other:?}");
            }
        }
    }
    log::warn!("MQTT event loop ended");
}

/// Publish sensor data as JSON to `{base_topic}/{mac}`.
///
/// `mac` must be the 12-hex-character address without delimiters.
pub fn publish(
    client: &mut EspMqttClient<'static>,
    mac: &str,
    payload: &str,
) -> anyhow::Result<()> {
    let topic = format!("{}/{}", config::MQTT_BASE_TOPIC, mac);
    client.publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())?;
    Ok(())
}
