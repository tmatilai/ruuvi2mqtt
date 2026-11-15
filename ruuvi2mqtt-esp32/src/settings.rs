//! The settings that the button changes, in NVS.

use std::ffi::CStr;

use esp_idf_svc::sys::{self, EspError};

const NAMESPACE: &CStr = c"settings";
const LED_LIT: &CStr = c"led_lit";

/// Returns the LED's choice: `Some(true)` for lit, `Some(false)` for dark,
/// `None` without a choice.
pub fn led_lit() -> Option<bool> {
    unsafe {
        let mut handle: sys::nvs_handle_t = 0;
        if sys::nvs_open(
            NAMESPACE.as_ptr(),
            sys::nvs_open_mode_t_NVS_READONLY,
            &raw mut handle,
        ) != sys::ESP_OK
        {
            return None;
        }
        let mut lit: u8 = 0;
        let ret = sys::nvs_get_u8(handle, LED_LIT.as_ptr(), &raw mut lit);
        sys::nvs_close(handle);
        (ret == sys::ESP_OK).then_some(lit == 1)
    }
}

/// Saves the LED's choice: lit or dark.
pub fn set_led_lit(lit: bool) -> Result<(), EspError> {
    write(|handle| unsafe { sys::esp!(sys::nvs_set_u8(handle, LED_LIT.as_ptr(), u8::from(lit))) })
}

/// Drops every setting.
pub fn reset() -> Result<(), EspError> {
    write(|handle| unsafe { sys::esp!(sys::nvs_erase_all(handle)) })
}

fn write(change: impl FnOnce(sys::nvs_handle_t) -> Result<(), EspError>) -> Result<(), EspError> {
    unsafe {
        let mut handle: sys::nvs_handle_t = 0;
        sys::esp!(sys::nvs_open(
            NAMESPACE.as_ptr(),
            sys::nvs_open_mode_t_NVS_READWRITE,
            &raw mut handle,
        ))?;
        let result = change(handle).and_then(|()| sys::esp!(sys::nvs_commit(handle)));
        sys::nvs_close(handle);
        result
    }
}
