//! A slow breath on the board's LED, to show that the simulator runs
//! without drawing the eye. The `LED_*` variables work as in the firmware,
//! except that `LED_MODE=on` breathes.

use std::{
    thread,
    time::{Duration, Instant},
};

use air_simulator::{PULSE_MS, PULSE_PERIOD_MS, parse_color, pulse_level, ws2812_grb};
use anyhow::{Context, Result, anyhow, bail};
use esp_idf_svc::hal::{
    gpio::AnyOutputPin,
    ledc::{
        LedcDriver, LedcTimerDriver,
        config::{Resolution, TimerConfig},
    },
    peripherals::Peripherals,
    rmt::{
        PinState, Pulse, PulseTicks, Symbol, TxChannelDriver,
        config::{TransmitConfig, TxChannelConfig},
        encoder::{BytesEncoder, BytesEncoderConfig},
    },
    units::FromValueType,
};
use log::{info, warn};

/// How often the brightness changes during a breath.
const STEP: Duration = Duration::from_millis(20);

/// A plain LED's peak, as a fraction of full duty: full duty is bright.
const GPIO_PEAK_DIVISOR: u32 = 8;

enum Kind {
    Ws2812 { color: u32 },
    Gpio { active_low: bool },
}

/// Starts the LED in a thread of its own: `LED_MODE=on` breathes, `off`
/// keeps it dark, and so does `errors`, as nothing fails here. Unset, it
/// breathes when `LED_GPIO` is set, and leaves the pin alone otherwise.
pub fn start() -> Result<()> {
    let gpio = non_empty(option_env!("LED_GPIO"));
    let breath = match non_empty(option_env!("LED_MODE")) {
        None if gpio.is_none() => return Ok(()),
        None | Some("on") => true,
        Some("off" | "errors") => false,
        Some(other) => bail!("bad LED_MODE '{other}'"),
    };
    let gpio = gpio.context("LED_MODE needs LED_GPIO")?;
    let gpio: u8 = gpio.parse().map_err(|_| anyhow!("bad LED_GPIO '{gpio}'"))?;
    let active_low = match non_empty(option_env!("LED_ACTIVE_LOW")).unwrap_or("false") {
        "true" => true,
        "false" => false,
        other => bail!("bad LED_ACTIVE_LOW '{other}'"),
    };
    let kind = match non_empty(option_env!("LED_TYPE")).unwrap_or("ws2812") {
        "ws2812" if active_low => bail!("LED_ACTIVE_LOW is only for LED_TYPE=gpio"),
        "ws2812" => Kind::Ws2812 {
            color: parse_color(non_empty(option_env!("LED_COLOR")).unwrap_or("000020"))
                .map_err(anyhow::Error::msg)?,
        },
        "gpio" => Kind::Gpio { active_low },
        other => bail!("bad LED_TYPE '{other}'"),
    };
    thread::Builder::new()
        .name("led".into())
        .stack_size(4096)
        .spawn(move || {
            if let Err(e) = run(&kind, gpio, breath) {
                warn!("LED stopped: {e:#}");
            }
        })?;
    if breath {
        info!("LED breathing on GPIO {gpio}");
    }
    Ok(())
}

fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|v| !v.is_empty())
}

fn run(kind: &Kind, gpio: u8, breath: bool) -> Result<()> {
    // SAFETY: nothing else in the simulator uses this pin.
    let pin = unsafe { AnyOutputPin::steal(gpio) };
    let mut set: Box<dyn FnMut(u8) -> Result<()>> = match *kind {
        Kind::Ws2812 { color } => {
            let config = TxChannelConfig {
                resolution: 10.MHz().into(),
                ..Default::default()
            };
            let mut tx = TxChannelDriver::new(pin, &config)?;
            Box::new(move |level| ws2812_send(&mut tx, ws2812_grb(color, level)))
        }
        Kind::Gpio { active_low } => {
            let peripherals = Peripherals::take().context("taking the LEDC")?;
            let config = TimerConfig::new()
                .frequency(1.kHz().into())
                .resolution(Resolution::Bits10);
            let timer = LedcTimerDriver::new(peripherals.ledc.timer0, &config)?;
            let mut channel = LedcDriver::new(peripherals.ledc.channel0, timer, pin)?;
            let max = channel.get_max_duty();
            let peak = max / GPIO_PEAK_DIVISOR;
            Box::new(move |level| {
                let duty = peak * u32::from(level) / 255;
                Ok(channel.set_duty(if active_low { max - duty } else { duty })?)
            })
        }
    };

    if !breath {
        set(0)?;
        // The driver keeps the pin at its level while it lives.
        loop {
            thread::park();
        }
    }

    let start = Instant::now();
    loop {
        #[allow(clippy::cast_possible_truncation)] // below PULSE_PERIOD_MS
        let ms = (start.elapsed().as_millis() % u128::from(PULSE_PERIOD_MS)) as u32;
        set(pulse_level(ms))?;
        // Dark until the next breath
        let pause = if ms < PULSE_MS {
            STEP
        } else {
            Duration::from_millis(u64::from(PULSE_PERIOD_MS - ms))
        };
        thread::sleep(pause);
    }
}

/// Sends GRB bytes to one WS2812 LED, as the firmware's `led.rs` does.
///
/// At 10 MHz (1 tick = 100 ns), a 0 bit is 300 ns high and 900 ns low, a 1
/// bit 900 ns high and 300 ns low.
fn ws2812_send(tx: &mut TxChannelDriver<'static>, grb: [u8; 3]) -> Result<()> {
    let short = PulseTicks::new(3)?;
    let long = PulseTicks::new(9)?;
    let config = BytesEncoderConfig {
        bit0: Symbol::new(
            Pulse::new(PinState::High, short),
            Pulse::new(PinState::Low, long),
        ),
        bit1: Symbol::new(
            Pulse::new(PinState::High, long),
            Pulse::new(PinState::Low, short),
        ),
        msb_first: true,
        ..Default::default()
    };
    let encoder = BytesEncoder::with_config(&config)?;
    tx.send_and_wait(encoder, &grb, &TransmitConfig::default())?;
    Ok(())
}
