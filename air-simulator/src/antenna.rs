//! The RF switch of the Seeed XIAO ESP32-C6, as in the firmware's
//! `antenna.rs`. Unpowered, it leaves the board barely heard.

use anyhow::{Result, bail};
use esp_idf_svc::sys::{self, EspError, gpio_num_t};
use log::info;

/// Low powers the RF switch.
const SWITCH_POWER: gpio_num_t = 3;

/// Low selects the internal antenna, high the external one.
const SELECT: gpio_num_t = 14;

/// Selects the `ANTENNA`, before BLE starts. Without it, the pins stay
/// untouched: other boards use them for other things.
pub fn select() -> Result<()> {
    let external = match option_env!("ANTENNA").filter(|v| !v.is_empty()) {
        None => return Ok(()),
        Some("internal") => false,
        Some("external") => true,
        Some(other) => bail!("bad ANTENNA '{other}'"),
    };
    if sys::CONFIG_IDF_TARGET[..] != b"esp32c6\0"[..] {
        bail!("ANTENNA is only for the Seeed XIAO ESP32-C6");
    }
    // The firmware holds the pin before deep sleep, and a flash without a
    // power cycle may keep the hold.
    sys::esp!(unsafe { sys::gpio_hold_dis(SWITCH_POWER) })?;
    output(SWITCH_POWER, false)?;
    output(SELECT, external)?;
    info!(
        "Antenna: {}",
        if external { "external" } else { "internal" }
    );
    Ok(())
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
