use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::{
    gpio::AnyOutputPin,
    ledc::{
        config::{Resolution, TimerConfig},
        LedcDriver, LedcTimerDriver, CHANNEL0, TIMER0,
    },
    rmt::{
        config::{TransmitConfig, TxChannelConfig},
        encoder::{BytesEncoder, BytesEncoderConfig},
        PinState, Pulse, PulseTicks, Symbol, TxChannelDriver,
    },
    sys::EspError,
    units::FromValueType,
};
use log::{info, warn};
use ruuvi2mqtt_esp32::led_pattern::{ws2812_grb, Pattern};

use crate::config;

/// Set while Wi-Fi is down, for the LED thread of [`start`].
static WIFI_DOWN: AtomicBool = AtomicBool::new(false);

/// Set while the last cycle failed, for the LED thread of [`start`].
static FAILED: AtomicBool = AtomicBool::new(false);

pub fn set_wifi_down(down: bool) {
    WIFI_DOWN.store(down, Ordering::Relaxed);
}

pub fn set_failed(failed: bool) {
    FAILED.store(failed, Ordering::Relaxed);
}

/// Set by [`off`].
static OFF: AtomicBool = AtomicBool::new(false);

/// Set by the LED thread once [`off`] has made the LED dark.
static DARK: AtomicBool = AtomicBool::new(false);

/// Set once the LED thread runs.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Turn the LED off for good, before deep sleep: a WS2812 keeps its color,
/// and a plain LED's pin floats.
pub fn off() {
    if !STARTED.load(Ordering::Relaxed) {
        return;
    }
    OFF.store(true, Ordering::Relaxed);
    let start = Instant::now();
    while !DARK.load(Ordering::Relaxed) && start.elapsed() < 10 * STEP {
        thread::sleep(STEP);
    }
}

/// How often a lit LED changes its brightness.
const STEP: Duration = Duration::from_millis(20);

/// How often a dark LED checks for a failure.
const DARK_STEP: Duration = Duration::from_millis(100);

/// A plain LED at full duty is bright. Its breath and steady light peak at an
/// eighth of it, and its alarm at half.
const GPIO_DIM_DIVISOR: u32 = 8;
const GPIO_ALARM_DIVISOR: u32 = 2;

/// The WS2812's alarm, red at half brightness.
const WS2812_ALARM_COLOR: u32 = 0x80_00_00;

/// Run the LED in a thread of its own, as `LED_MODE` says.
pub fn start() -> anyhow::Result<()> {
    // config.rs fails the build when LED_MODE is set without LED_GPIO.
    let (Some(mode), Some(gpio)) = (config::LED_MODE, config::LED_GPIO) else {
        return Ok(());
    };
    info!(
        "LED: type={}{}, gpio={}, color={:06x}, mode={}",
        config::LED_TYPE,
        if config::LED_ACTIVE_LOW {
            " (active low)"
        } else {
            ""
        },
        gpio,
        config::LED_COLOR,
        mode,
    );
    thread::Builder::new().stack_size(4096).spawn(move || {
        if let Err(e) = Light::new(gpio).and_then(Light::run) {
            warn!("LED stopped: {e}");
        }
    })?;
    STARTED.store(true, Ordering::Relaxed);
    Ok(())
}

/// The LED of [`start`]. A plain LED dims with PWM.
enum Light {
    Ws2812(TxChannelDriver<'static>),
    Gpio(LedcDriver<'static>),
}

impl Light {
    fn new(gpio: u32) -> Result<Self, EspError> {
        // SAFETY: nothing else uses the pin or the LEDC.
        #[allow(clippy::cast_possible_truncation)] // GPIO numbers always fit in u8
        let pin = unsafe { AnyOutputPin::steal(gpio as u8) };
        if config::LED_TYPE == "ws2812" {
            let config = TxChannelConfig {
                resolution: 10.MHz().into(),
                ..Default::default()
            };
            return Ok(Self::Ws2812(TxChannelDriver::new(pin, &config)?));
        }
        let config = TimerConfig::new()
            .frequency(1.kHz().into())
            .resolution(Resolution::Bits10);
        let timer = LedcTimerDriver::new(unsafe { TIMER0::steal() }, &config)?;
        let channel = LedcDriver::new(unsafe { CHANNEL0::steal() }, timer, pin)?;
        Ok(Self::Gpio(channel))
    }

    fn run(mut self) -> Result<(), EspError> {
        let mut pattern = Pattern::Dark;
        let mut since = Instant::now();
        let mut shown = None;
        loop {
            let off = OFF.load(Ordering::Relaxed);
            let failing = WIFI_DOWN.load(Ordering::Relaxed) || FAILED.load(Ordering::Relaxed);
            let next = if off {
                Pattern::Dark
            } else {
                Pattern::for_mode(config::LED_MODE, config::BLE_SCAN_CONTINUOUS, failing)
            };
            if next != pattern {
                pattern = next;
                since = Instant::now();
            }
            #[allow(clippy::cast_possible_truncation)] // below the period
            let ms = (since.elapsed().as_millis() % u128::from(pattern.period_ms())) as u32;
            let level = pattern.level(ms);
            if shown != Some((pattern, level)) {
                self.show(pattern, level)?;
                shown = Some((pattern, level));
            }
            if off {
                DARK.store(true, Ordering::Relaxed);
                // The drivers keep the pin at its level while they live.
                loop {
                    thread::park();
                }
            }
            thread::sleep(if pattern == Pattern::Dark {
                DARK_STEP
            } else {
                STEP
            });
        }
    }

    fn show(&mut self, pattern: Pattern, level: u8) -> Result<(), EspError> {
        let alarm = pattern == Pattern::Alarm;
        match self {
            Self::Ws2812(tx) => {
                let color = if alarm {
                    WS2812_ALARM_COLOR
                } else {
                    config::LED_COLOR
                };
                ws2812_send(tx, ws2812_grb(color, level))
            }
            Self::Gpio(channel) => {
                let max = channel.get_max_duty();
                let peak = max
                    / if alarm {
                        GPIO_ALARM_DIVISOR
                    } else {
                        GPIO_DIM_DIVISOR
                    };
                let duty = peak * u32::from(level) / 255;
                channel.set_duty(if config::LED_ACTIVE_LOW {
                    max - duty
                } else {
                    duty
                })
            }
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
