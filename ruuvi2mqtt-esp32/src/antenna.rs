//! The RF switch of the Seeed XIAO ESP32-C6, which selects the internal
//! ceramic antenna or the external U.FL one:
//! <https://wiki.seeedstudio.com/xiao_esp32c6_getting_started/>

use esp_idf_svc::sys::{self, gpio_num_t, EspError};
use log::{info, warn};

use crate::config;

/// Low powers the RF switch.
const SWITCH_POWER: gpio_num_t = 3;

/// Low selects the internal antenna, high the external one.
const SELECT: gpio_num_t = 14;

/// Selects the `ANTENNA`. Before Wi-Fi or BLE start, so that both use it.
///
/// Without `ANTENNA`, the pins stay untouched: other boards use them for
/// other things.
pub fn select() {
    let Some(antenna) = config::ANTENNA else {
        return;
    };
    let result = (|| {
        // Released from the hold that power_off() set before deep sleep.
        sys::esp!(unsafe { sys::gpio_hold_dis(SWITCH_POWER) })?;
        output(SWITCH_POWER, false)?;
        output(SELECT, config::ANTENNA_EXTERNAL)
    })();
    match result {
        Ok(()) => info!("Antenna: {antenna}"),
        Err(e) => warn!("Failed to select the {antenna} antenna: {e}"),
    }
}

/// Powers the RF switch off for deep sleep. It draws more than the chip
/// does in deep sleep.
pub fn power_off() {
    if config::ANTENNA.is_none() {
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

fn output(pin: gpio_num_t, high: bool) -> Result<(), EspError> {
    unsafe {
        // The level first, so the pin doesn't glitch when it becomes an
        // output.
        sys::esp!(sys::gpio_set_level(pin, u32::from(high)))?;
        sys::esp!(sys::gpio_set_direction(
            pin,
            sys::gpio_mode_t_GPIO_MODE_OUTPUT
        ))
    }
}
