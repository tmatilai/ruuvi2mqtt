//! Reads the battery gauge that `BATTERY` names, over I2C.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use esp_idf_svc::hal::delay::TickType;
use esp_idf_svc::hal::gpio::AnyIOPin;
use esp_idf_svc::hal::i2c::{I2cConfig as Config, I2cDriver, I2C0};
use esp_idf_svc::hal::units::FromValueType;
use esp_idf_svc::sys::{self, EspError};
use log::{debug, warn};
use ruuvi2mqtt_esp32::battery::{Battery, Gauge};

use crate::{config, gpio};

/// The reading for the diagnostics: `None` without `BATTERY`, `Some(None)`
/// when the read failed.
#[allow(clippy::option_option)]
pub fn read() -> Option<Option<Battery>> {
    let gauge = config::BATTERY_GAUGE?;
    let (sda, scl) = config::BATTERY_SDA.zip(config::BATTERY_SCL)?;
    let battery = powered(|| {
        let mut bus = Bus::new(sda, scl, gauge.address())?;
        match gauge {
            Gauge::Max17048 => max17048(&mut bus),
            Gauge::M5pm1 => m5pm1(&mut bus),
        }
    })
    .inspect(|battery| debug!("Battery: {battery:?}"))
    .inspect_err(|e| warn!("Failed to read the battery: {e}"))
    .ok();
    *LAST.lock().unwrap() = battery;
    Some(battery)
}

/// The last [`read`], for the status page, so that its requests don't wake
/// the gauge.
#[allow(clippy::option_option)]
pub fn last() -> Option<Option<Battery>> {
    config::BATTERY_GAUGE.map(|_| *LAST.lock().unwrap())
}

static LAST: Mutex<Option<Battery>> = Mutex::new(None);

/// Runs `read` with `BATTERY_POWER_GPIO` high.
fn powered<T>(read: impl FnOnce() -> Result<T, EspError>) -> Result<T, EspError> {
    let Some(pin) = config::BATTERY_POWER_GPIO else {
        return read();
    };
    let gpio = sys::gpio_num_t::from(pin);
    gpio::output(gpio, true)?;
    let result = read();
    // The pin may power other things too, such as the Feather's LED, which
    // draws about 1 mA even when dark. The LED code keeps its own power on.
    if config::LED_POWER_GPIO != Some(u32::from(pin)) {
        gpio::output(gpio, false)?;
    }
    result
}

fn max17048(bus: &mut Bus) -> Result<Battery, EspError> {
    Ok(Battery::max17048(
        bus.read(0x02)?,
        bus.read(0x04)?,
        bus.read(0x16)?,
    ))
}

fn m5pm1(bus: &mut Bus) -> Result<Battery, EspError> {
    // The chip's I2C sleeps when idle, and the first transfers fail while it
    // wakes up.
    let mut attempt = 1;
    loop {
        // The battery's and the USB input's voltages.
        let result = bus.read(0x22).and_then(|[b0, b1, v0, v1]| {
            Battery::m5pm1([b0, b1], [v0, v1]).ok_or(INVALID_RESPONSE)
        });
        match result {
            Ok(battery) => return Ok(battery),
            Err(e) if attempt < 5 => debug!("M5PM1 read {attempt} failed: {e}"),
            Err(e) => return Err(e),
        }
        attempt += 1;
        thread::sleep(Duration::from_millis(1));
    }
}

const INVALID_RESPONSE: EspError = EspError::from_infallible::<{ sys::ESP_ERR_INVALID_RESPONSE }>();

/// The gauge's I2C bus, for one reading.
///
/// esp-idf-hal's driver is ESP-IDF's legacy one, and an image that links
/// both of ESP-IDF's I2C drivers aborts at boot.
struct Bus {
    driver: I2cDriver<'static>,
    address: u8,
}

/// Per transfer.
const TIMEOUT: TickType = TickType::new_millis(50);

impl Bus {
    fn new(sda: u8, scl: u8, address: u8) -> Result<Self, EspError> {
        let config = Config::new()
            .baudrate(100.kHz().into())
            .sda_enable_pullup(true)
            .scl_enable_pullup(true);
        // SAFETY: nothing else uses I2C0 or the gauge's pins, and the
        // driver is dropped after the reading.
        let driver = unsafe {
            I2cDriver::new(
                I2C0::steal(),
                AnyIOPin::steal(sda),
                AnyIOPin::steal(scl),
                &config,
            )
        }?;
        Ok(Self { driver, address })
    }

    /// `N` bytes from `register` on.
    fn read<const N: usize>(&mut self, register: u8) -> Result<[u8; N], EspError> {
        let mut value = [0; N];
        self.driver
            .write_read(self.address, &[register], &mut value, TIMEOUT.ticks())?;
        Ok(value)
    }
}
