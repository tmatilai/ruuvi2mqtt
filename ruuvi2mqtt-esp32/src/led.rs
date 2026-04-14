use esp_idf_svc::hal::{
    gpio::AnyOutputPin,
    rmt::{
        config::{TransmitConfig, TxChannelConfig},
        encoder::{BytesEncoder, BytesEncoderConfig},
        PinState, Pulse, PulseTicks, Symbol, TxChannelDriver,
    },
    units::FromValueType,
};
use log::info;

use crate::config;

/// LED controller — supports WS2812 (addressable RGB), plain GPIO, or no-op.
pub struct Led {
    inner: Inner,
}

enum Inner {
    Noop,
    Ws2812(TxChannelDriver<'static>),
    Gpio { pin: i32 },
}

impl Led {
    /// Initialise the LED driver based on compile-time configuration.
    ///
    /// Returns a no-op controller when `LED_MODE` is unset.
    pub fn new() -> anyhow::Result<Self> {
        if config::LED_MODE.is_none() {
            return Ok(Self { inner: Inner::Noop });
        }

        info!(
            "LED: type={}, gpio={}, mode={}",
            config::LED_TYPE,
            config::LED_GPIO,
            config::LED_MODE.unwrap_or("none"),
        );

        let inner = match config::LED_TYPE {
            "ws2812" => {
                #[allow(clippy::cast_possible_truncation)] // GPIO numbers always fit in u8
                let pin = unsafe { AnyOutputPin::steal(config::LED_GPIO as u8) };
                let tx_config = TxChannelConfig {
                    resolution: 10.MHz().into(),
                    ..Default::default()
                };
                let tx = TxChannelDriver::new(pin, &tx_config)?;
                Inner::Ws2812(tx)
            }
            "gpio" => {
                #[allow(clippy::cast_possible_wrap)] // GPIO numbers are always positive
                let pin = config::LED_GPIO as i32;
                unsafe {
                    esp_idf_svc::sys::gpio_set_direction(
                        pin,
                        esp_idf_svc::sys::gpio_mode_t_GPIO_MODE_OUTPUT,
                    );
                }
                Inner::Gpio { pin }
            }
            other => {
                log::warn!("Unknown LED_TYPE '{other}', disabling LED");
                Inner::Noop
            }
        };

        Ok(Self { inner })
    }

    /// Apply the configured `LED_MODE`: turn on, explicitly off, or do nothing.
    pub fn apply_mode(&mut self) {
        match config::LED_MODE {
            Some("on") => self.on(),
            Some(_) => self.off(), // "off" or any unrecognised value
            None => {}
        }
    }

    /// Turn the LED on (using `LED_COLOR` for WS2812, high for GPIO).
    pub fn on(&mut self) {
        match &mut self.inner {
            Inner::Noop => {}
            Inner::Ws2812(tx) => {
                // WS2812 expects GRB byte order; use white.
                if let Err(e) = ws2812_send(tx, [255, 255, 255]) {
                    log::warn!("LED on failed: {e}");
                }
            }
            Inner::Gpio { pin } => unsafe {
                esp_idf_svc::sys::gpio_set_level(*pin, 1);
            },
        }
    }

    /// Turn the LED off.
    pub fn off(&mut self) {
        match &mut self.inner {
            Inner::Noop => {}
            Inner::Ws2812(tx) => {
                if let Err(e) = ws2812_send(tx, [0, 0, 0]) {
                    log::warn!("LED off failed: {e}");
                }
            }
            Inner::Gpio { pin } => unsafe {
                esp_idf_svc::sys::gpio_set_level(*pin, 0);
            },
        }
    }
}

/// Send 3 bytes (GRB) to a single WS2812 LED via the RMT peripheral.
///
/// WS2812 timing at 10 MHz resolution (1 tick = 100 ns):
///   bit 0: 300 ns high (3 ticks), 900 ns low (9 ticks)
///   bit 1: 900 ns high (9 ticks), 300 ns low (3 ticks)
fn ws2812_send(
    tx: &mut TxChannelDriver<'static>,
    grb: [u8; 3],
) -> Result<(), esp_idf_svc::sys::EspError> {
    let t0h = PulseTicks::new(3).unwrap();
    let t0l = PulseTicks::new(9).unwrap();
    let t1h = PulseTicks::new(9).unwrap();
    let t1l = PulseTicks::new(3).unwrap();

    let encoder_config = BytesEncoderConfig {
        bit0: Symbol::new(
            Pulse::new(PinState::High, t0h),
            Pulse::new(PinState::Low, t0l),
        ),
        bit1: Symbol::new(
            Pulse::new(PinState::High, t1h),
            Pulse::new(PinState::Low, t1l),
        ),
        msb_first: true,
        ..Default::default()
    };
    let encoder = BytesEncoder::with_config(&encoder_config)?;
    tx.send_and_wait(encoder, &grb, &TransmitConfig::default())
}
