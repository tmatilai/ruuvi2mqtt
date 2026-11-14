//! Plain GPIO outputs.

use esp_idf_svc::sys::{self, gpio_num_t, EspError};

/// Makes `pin` an output at the level.
pub fn output(pin: gpio_num_t, high: bool) -> Result<(), EspError> {
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
