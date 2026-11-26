//! Drives the LED in a thread of its own. `led_pattern.rs` has the patterns.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::{
    delay::TickType,
    gpio::{AnyInputPin, AnyOutputPin, Input, InterruptType, Output, PinDriver, Pull},
    ledc::{
        config::{Resolution, TimerConfig},
        LedcDriver, LedcTimerDriver, CHANNEL0, TIMER0,
    },
    rmt::{
        config::{TransmitConfig, TxChannelConfig},
        encoder::{BytesEncoder, BytesEncoderConfig},
        PinState, Pulse, PulseTicks, Symbol, TxChannelDriver,
    },
    task::notification::Notification,
    units::FromValueType,
};
use esp_idf_svc::sys::EspError;
use log::{info, warn};
use ruuvi2mqtt_esp32::button::{Button, Gesture};
use ruuvi2mqtt_esp32::config;
use ruuvi2mqtt_esp32::led_pattern::{self, ws2812_grb, Pattern};
use ruuvi2mqtt_esp32::status::Grade;

use crate::{settings, web};

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

/// Turns the LED off for good, before deep sleep: a WS2812 keeps its color,
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

/// How often a dark LED checks for a failure. A press wakes it sooner.
const DARK_STEP: Duration = Duration::from_millis(100);

/// How long a click shows the status, in ms.
const STATUS_MS: u64 = 10_000;

/// The LED thread's stack, in bytes. The button's NVS writes run on it too.
fn stack_size() -> usize {
    if config::get().button_gpio.is_some() {
        8192
    } else {
        4096
    }
}

/// Runs the LED in a thread of its own, as `LED_MODE` and the button say.
pub fn start() -> anyhow::Result<()> {
    // Config::parse rejects LED_MODE without LED_GPIO.
    let (Some(mode), Some(gpio)) = (config::get().led_mode.as_deref(), config::get().led_gpio)
    else {
        return Ok(());
    };
    // Without a button, NVS has no choice that counts.
    let choice = config::get().button_gpio.and_then(|_| settings::led_lit());
    info!(
        "LED: type={}{}, gpio={}{}, color={:06x}, mode={}{}",
        config::get().led_type.as_str(),
        if config::get().led_active_low {
            " (active low)"
        } else {
            ""
        },
        gpio,
        config::get()
            .led_power_gpio
            .map(|power| format!(", power gpio={power}"))
            .unwrap_or_default(),
        config::get().led_color,
        led_pattern::mode(mode, choice),
        config::get()
            .button_gpio
            .map(|button| format!(", button gpio={button}"))
            .unwrap_or_default(),
    );
    thread::Builder::new()
        .stack_size(stack_size())
        .spawn(move || {
            if let Err(e) = Light::new(gpio, mode, choice).and_then(Light::run) {
                warn!("LED stopped: {e}");
            }
        })?;
    STARTED.store(true, Ordering::Relaxed);
    Ok(())
}

/// The LED of [`start`].
struct Light {
    driver: Driver,
    /// Keeps `LED_POWER_GPIO` high while it lives.
    _power: Option<PinDriver<'static, Output>>,
    /// `LED_MODE`.
    mode: &'static str,
    /// The button's choice in NVS: lit or dark.
    choice: Option<bool>,
    button: Option<Controls>,
    /// The grade of the last click, and when it came, in ms.
    status: Option<(Grade, u64)>,
}

/// A plain LED dims with PWM.
enum Driver {
    Ws2812(TxChannelDriver<'static>),
    Gpio(LedcDriver<'static>),
}

impl Light {
    fn new(gpio: u32, mode: &'static str, choice: Option<bool>) -> Result<Self, EspError> {
        // The LED works without the button.
        let button = config::get().button_gpio.and_then(|pin| {
            Controls::new(pin)
                .inspect_err(|e| warn!("Button failed: {e}"))
                .ok()
        });
        let power = config::get()
            .led_power_gpio
            .map(|power| {
                let mut power = PinDriver::output(output_pin(power))?;
                power.set_high()?;
                Ok::<_, EspError>(power)
            })
            .transpose()?;
        let pin = output_pin(gpio);
        let driver = if config::get().led_type.as_str() == "ws2812" {
            let config = TxChannelConfig {
                resolution: 10.MHz().into(),
                ..Default::default()
            };
            Driver::Ws2812(TxChannelDriver::new(pin, &config)?)
        } else {
            let config = TimerConfig::new()
                .frequency(1.kHz().into())
                .resolution(Resolution::Bits10);
            let timer = LedcTimerDriver::new(unsafe { TIMER0::steal() }, &config)?;
            Driver::Gpio(LedcDriver::new(unsafe { CHANNEL0::steal() }, timer, pin)?)
        };
        Ok(Self {
            driver,
            _power: power,
            mode,
            choice,
            button,
            status: None,
        })
    }

    fn run(mut self) -> Result<(), EspError> {
        let start = Instant::now();
        let mut pattern = Pattern::Dark;
        let mut since = 0;
        let mut shown = None;
        loop {
            let off = OFF.load(Ordering::Relaxed);
            let now = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let gesture = self
                .button
                .as_mut()
                .and_then(|controls| controls.button.update(controls.pin.is_low(), now));
            if let Some(gesture) = gesture {
                self.act(gesture, now);
            }
            let next = if off {
                Pattern::Dark
            } else {
                self.pattern(now)
            };
            // A click restarts the blinks, also when the grade is the same.
            if next != pattern || gesture == Some(Gesture::Click) {
                pattern = next;
                since = now;
            }
            #[allow(clippy::cast_possible_truncation)] // below the period
            let ms = ((now - since) % u64::from(pattern.period_ms())) as u32;
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
            match &mut self.button {
                _ if pattern != Pattern::Dark => thread::sleep(STEP),
                Some(controls) => controls.wait(DARK_STEP),
                None => thread::sleep(DARK_STEP),
            }
        }
    }

    /// Returns the pattern at `now` ms: a press's feedback, the status for a
    /// while after a click, or the mode's pattern.
    fn pattern(&self, now: u64) -> Pattern {
        let continuous = config::get().ble_scan_continuous;
        if let Some(controls) = &self.button {
            // Without the alarm, so that the preview shows the toggle.
            let preview = Pattern::for_mode(
                led_pattern::mode(self.mode, Some(!self.lit())),
                continuous,
                false,
            );
            if let Some(pattern) = Pattern::for_press(controls.button.pending(now), preview) {
                return pattern;
            }
        }
        if let Some((grade, at)) = self.status {
            if now - at < STATUS_MS {
                return Pattern::Status(grade);
            }
        }
        let failing = WIFI_DOWN.load(Ordering::Relaxed) || FAILED.load(Ordering::Relaxed);
        Pattern::for_mode(
            led_pattern::mode(self.mode, self.choice),
            continuous,
            failing,
        )
    }

    fn lit(&self) -> bool {
        self.choice.unwrap_or(self.mode == "on")
    }

    fn act(&mut self, gesture: Gesture, now: u64) {
        // A new mode shows at once.
        self.status = None;
        match gesture {
            Gesture::Click => {
                let grade = web::link_grade();
                info!("LED status: {grade:?}");
                self.status = Some((grade, now));
            }
            Gesture::Hold => {
                let lit = !self.lit();
                self.choice = Some(lit);
                info!("LED mode: {}", led_pattern::mode(self.mode, self.choice));
                if let Err(e) = settings::set_led_lit(lit) {
                    warn!("Could not save the LED mode: {e}");
                }
            }
            Gesture::Reset => {
                self.choice = None;
                info!("LED mode: {} (LED_MODE)", self.mode);
                if let Err(e) = settings::reset() {
                    warn!("Could not reset the settings: {e}");
                }
            }
        }
    }

    fn show(&mut self, pattern: Pattern, level: u8) -> Result<(), EspError> {
        match &mut self.driver {
            Driver::Ws2812(tx) => ws2812_send(
                tx,
                ws2812_grb(pattern.ws2812_color(config::get().led_color), level),
            ),
            Driver::Gpio(channel) => {
                let max = channel.get_max_duty();
                let peak = max / pattern.gpio_divisor();
                let duty = peak * u32::from(level) / 255;
                channel.set_duty(if config::get().led_active_low {
                    max - duty
                } else {
                    duty
                })
            }
        }
    }
}

/// The button, and the interrupt that wakes a dark LED on a press.
struct Controls {
    pin: PinDriver<'static, Input>,
    button: Button,
    notification: Notification,
}

impl Controls {
    /// Takes the button's pin, from the LED thread: the interrupt notifies the
    /// thread that creates the notification.
    fn new(gpio: u32) -> Result<Self, EspError> {
        // SAFETY: nothing else uses the button's pin. Config::parse checks that
        // it exists on the chip.
        #[allow(clippy::cast_possible_truncation)] // GPIO numbers always fit in u8
        let mut pin = PinDriver::input(unsafe { AnyInputPin::steal(gpio as u8) }, Pull::Up)?;
        pin.set_interrupt_type(InterruptType::NegEdge)?;
        let notification = Notification::new();
        let notifier = notification.notifier();
        // SAFETY: the callback only notifies, which an ISR may do. The pin's
        // driver unsubscribes it when the LED thread drops it.
        unsafe {
            pin.subscribe(move || {
                notifier.notify_and_yield(NonZeroU32::MIN);
            })?;
        }
        Ok(Self {
            pin,
            button: Button::new(),
            notification,
        })
    }

    /// Waits up to `timeout` for a press. While the button is down, waits for
    /// `STEP` instead, to time the press.
    fn wait(&mut self, timeout: Duration) {
        // The driver disables the interrupt each time it fires. Without it,
        // the LED polls.
        if self.pin.is_low() || self.pin.enable_interrupt().is_err() {
            thread::sleep(STEP);
            return;
        }
        self.notification.wait(TickType::from(timeout).ticks());
    }
}

/// The LED's pin. `Config::parse` checks that it exists on the chip.
fn output_pin(gpio: u32) -> AnyOutputPin<'static> {
    // SAFETY: nothing else uses the LED's pins or the LEDC.
    #[allow(clippy::cast_possible_truncation)] // GPIO numbers always fit in u8
    unsafe {
        AnyOutputPin::steal(gpio as u8)
    }
}

/// Sends 3 bytes (GRB) to a single WS2812 LED through the RMT peripheral.
///
/// WS2812 timing at 10 MHz resolution (1 tick = 100 ns):
///   bit 0: 300 ns high (3 ticks), 900 ns low (9 ticks)
///   bit 1: 900 ns high (9 ticks), 300 ns low (3 ticks).
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
