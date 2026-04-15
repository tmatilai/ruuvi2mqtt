use esp_idf_svc::mqtt::client::{EspMqttClient, QoS};
use esp_idf_svc::sys;
use log::info;
use serde::Serialize;

use crate::config;

#[derive(Debug, Serialize)]
pub struct Diagnostics {
    pub firmware: &'static str,
    pub wifi_rssi: Option<i8>,
    pub free_heap: u32,
    pub tags: Vec<String>,
    pub cycle_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Diagnostics {
    pub fn publish(&self, client: &mut EspMqttClient<'static>) {
        let topic = format!(
            "{}/diagnostics/{}",
            config::MQTT_BASE_TOPIC,
            config::DEVICE_HOSTNAME,
        );
        let payload = serde_json::to_string(self).unwrap();
        info!("Diagnostics: {payload}");
        if let Err(e) = client.publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes()) {
            log::error!("Failed to publish diagnostics: {e}");
        }
    }
}

/// Read Wi-Fi RSSI from the connected AP.
pub fn wifi_rssi() -> Option<i8> {
    unsafe {
        let mut ap_info: sys::wifi_ap_record_t = std::mem::zeroed();
        if sys::esp_wifi_sta_get_ap_info(&raw mut ap_info) == sys::ESP_OK {
            Some(ap_info.rssi)
        } else {
            None
        }
    }
}

/// Free heap memory in bytes.
pub fn free_heap() -> u32 {
    unsafe { sys::esp_get_free_heap_size() }
}
