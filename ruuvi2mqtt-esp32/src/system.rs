//! Board values that need ESP-IDF.

use std::ffi::CStr;

use esp_idf_svc::partition::EspPartition;
use esp_idf_svc::sys;
use ruuvi2mqtt_esp32::config;

/// The connected access point.
pub fn ap_record() -> Option<sys::wifi_ap_record_t> {
    unsafe {
        let mut ap_info: sys::wifi_ap_record_t = std::mem::zeroed();
        if sys::esp_wifi_sta_get_ap_info(&raw mut ap_info) == sys::ESP_OK {
            Some(ap_info)
        } else {
            None
        }
    }
}

/// Free heap memory in bytes.
pub fn free_heap() -> u32 {
    unsafe { sys::esp_get_free_heap_size() }
}

/// The ESP-IDF target, such as `esp32c6`.
pub fn chip() -> &'static str {
    CStr::from_bytes_with_nul(sys::CONFIG_IDF_TARGET)
        .ok()
        .and_then(|s| s.to_str().ok())
        .unwrap_or("unknown")
}

/// The contents of the `config` partition.
pub fn config_partition() -> anyhow::Result<Vec<u8>> {
    // SAFETY: only this function opens the partition, once at boot.
    let mut partition = unsafe { EspPartition::new("config") }?
        .ok_or_else(|| anyhow::anyhow!("No config partition. Flash the board with `make flash`"))?;
    let mut bytes = vec![0; config::PARTITION_SIZE.min(partition.size())];
    partition.read(0, &mut bytes)?;
    Ok(bytes)
}
