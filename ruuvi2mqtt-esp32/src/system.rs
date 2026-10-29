//! Values for the diagnostics that need ESP-IDF.

use std::ffi::CStr;

use esp_idf_svc::sys;

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

/// The ESP-IDF target, e.g. `esp32c6`.
pub fn chip() -> &'static str {
    CStr::from_bytes_with_nul(sys::CONFIG_IDF_TARGET)
        .ok()
        .and_then(|s| s.to_str().ok())
        .unwrap_or("unknown")
}
