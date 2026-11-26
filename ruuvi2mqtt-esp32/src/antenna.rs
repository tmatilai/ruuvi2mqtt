//! The RF switch of the Seeed XIAO ESP32-C6, which selects the internal
//! ceramic antenna or the external U.FL one:
//! <https://wiki.seeedstudio.com/xiao_esp32c6_getting_started/>

use esp_idf_svc::sys::{self, gpio_num_t};
use log::{info, warn};
use ruuvi2mqtt_esp32::config;

use crate::gpio::output;

#[allow(clippy::cast_possible_wrap)] // GPIO numbers are small
const SWITCH_POWER: gpio_num_t = config::ANTENNA_SWITCH_POWER as gpio_num_t;
#[allow(clippy::cast_possible_wrap)]
const SELECT: gpio_num_t = config::ANTENNA_SELECT as gpio_num_t;

/// Selects the `ANTENNA`. Before Wi-Fi or BLE start, so that both use it.
///
/// Without `ANTENNA`, the pins stay untouched: other boards use them for
/// other things.
pub fn select() {
    let Some(antenna) = config::get().antenna.as_deref() else {
        return;
    };
    let result = (|| {
        // Released from the hold that power_off() set before deep sleep.
        sys::esp!(unsafe { sys::gpio_hold_dis(SWITCH_POWER) })?;
        output(SWITCH_POWER, false)?;
        output(SELECT, config::get().antenna_external)
    })();
    match result {
        Ok(()) => info!("Antenna: {antenna}"),
        Err(e) => warn!("Failed to select the {antenna} antenna: {e}"),
    }
}

/// Powers the RF switch off for deep sleep. It draws more than the chip
/// does in deep sleep.
pub fn power_off() {
    if config::get().antenna.is_none() {
        return;
    }
    let result = (|| {
        output(SELECT, false)?;
        output(SWITCH_POWER, true)?;
        // Deep sleep releases the pin unless it is held.
        sys::esp!(unsafe { sys::gpio_hold_en(SWITCH_POWER) })
    })();
    if let Err(e) = result {
        warn!("Failed to power off the antenna switch: {e}");
    }
}
