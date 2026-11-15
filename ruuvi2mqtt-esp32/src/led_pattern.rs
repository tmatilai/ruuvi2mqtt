//! The LED's patterns, as brightness over time, and their colors.

use crate::button::Gesture;
use crate::status::Grade;

/// What the LED shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Dark,
    /// Lit, while a board that deep sleeps is awake.
    Steady,
    /// A slow, dim breath, to show that the board runs without drawing the
    /// eye.
    Breath,
    /// Two quick blinks, while Wi-Fi is down or a cycle failed.
    Alarm,
    /// Full brightness, for the moment a hold reaches the toggle.
    Flash,
    /// Fast blinks, while a press has reached the reset.
    Rapid,
    /// One blink for good, two for fair and three for poor, then a pause.
    Status(Grade),
}

/// How long a breath lasts, in ms. The LED is dark for the rest of its
/// period.
const BREATH_MS: u32 = 2000;

/// How long an alarm blink lasts, and when the second one starts, in ms.
const BLINK_MS: u32 = 120;
const SECOND_BLINK_MS: u32 = 360;

/// How long a rapid blink lasts, in ms.
const RAPID_BLINK_MS: u32 = 100;

/// How long a status blink lasts, and how far apart the blinks start, in ms.
const STATUS_BLINK_MS: u32 = 150;
const STATUS_BLINK_EVERY_MS: u32 = 400;

/// How long a hold flashes before the preview, in ms.
const FLASH_MS: u64 = 150;

/// Returns `LED_MODE` with the button's choice: lit is `on`, dark is the
/// compiled mode, or `errors` when that is `on`.
#[must_use]
pub fn mode(compiled: &str, lit: Option<bool>) -> &str {
    match lit {
        Some(true) => "on",
        Some(false) if compiled == "on" => "errors",
        Some(false) | None => compiled,
    }
}

impl Pattern {
    /// Returns the LED's pattern for `mode`, an `LED_MODE` value.
    #[must_use]
    pub fn for_mode(mode: &str, continuous: bool, failing: bool) -> Self {
        match mode {
            "on" | "errors" if failing => Self::Alarm,
            "on" if continuous => Self::Breath,
            "on" => Self::Steady,
            _ => Self::Dark,
        }
    }

    /// Returns the LED's feedback for a press, or `None` when the press shows
    /// nothing yet. A hold flashes, then shows `preview`, the pattern after the
    /// toggle.
    #[must_use]
    pub fn for_press(pending: Option<(Gesture, u64)>, preview: Self) -> Option<Self> {
        match pending? {
            (Gesture::Click, _) => None,
            (Gesture::Hold, ms) if ms < FLASH_MS => Some(Self::Flash),
            (Gesture::Hold, _) => Some(preview),
            (Gesture::Reset, _) => Some(Self::Rapid),
        }
    }

    /// How often the pattern repeats, in ms.
    #[must_use]
    pub const fn period_ms(self) -> u32 {
        match self {
            Self::Dark | Self::Steady | Self::Flash => 1000,
            Self::Breath => 6000,
            Self::Alarm => 3000,
            Self::Rapid => 200,
            Self::Status(_) => 2000,
        }
    }

    /// The brightness, 0–255, `ms` into the pattern.
    #[must_use]
    pub fn level(self, ms: u32) -> u8 {
        let t = ms % self.period_ms();
        match self {
            Self::Steady | Self::Flash => 255,
            Self::Breath if t < BREATH_MS => {
                let x = f64::from(t) / f64::from(BREATH_MS);
                // A raised cosine, squared, as the eye sees dim light as
                // brighter.
                let s = (1.0 - (std::f64::consts::TAU * x).cos()) / 2.0;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0–255
                let level = (s * s * 255.0).round() as u8;
                level
            }
            Self::Alarm
                if t < BLINK_MS || (SECOND_BLINK_MS..SECOND_BLINK_MS + BLINK_MS).contains(&t) =>
            {
                255
            }
            Self::Rapid if t < RAPID_BLINK_MS => 255,
            Self::Status(grade)
                if t % STATUS_BLINK_EVERY_MS < STATUS_BLINK_MS
                    && t / STATUS_BLINK_EVERY_MS < blinks(grade) =>
            {
                255
            }
            Self::Dark | Self::Breath | Self::Alarm | Self::Rapid | Self::Status(_) => 0,
        }
    }

    /// Returns the WS2812's color, as `RRGGBB`. `led_color` is `LED_COLOR`.
    #[must_use]
    pub const fn ws2812_color(self, led_color: u32) -> u32 {
        match self {
            // Red, at half brightness.
            Self::Alarm => 0x80_00_00,
            // White.
            Self::Flash | Self::Rapid => 0x40_40_40,
            Self::Status(Grade::Good) => 0x00_40_00,
            // Yellow. The eye sees green brighter than red, so it gets less.
            Self::Status(Grade::Fair) => 0x40_18_00,
            Self::Status(Grade::Poor) => 0x40_00_00,
            Self::Dark | Self::Steady | Self::Breath => led_color,
        }
    }

    /// Returns what a plain LED's full duty is divided by at the pattern's
    /// peak. The full duty is bright, so only a press's feedback uses it.
    #[must_use]
    pub const fn gpio_divisor(self) -> u32 {
        match self {
            Self::Flash | Self::Rapid => 1,
            Self::Alarm | Self::Status(_) => 2,
            Self::Dark | Self::Steady | Self::Breath => 8,
        }
    }
}

const fn blinks(grade: Grade) -> u32 {
    match grade {
        Grade::Good => 1,
        Grade::Fair => 2,
        Grade::Poor => 3,
    }
}

/// An `RRGGBB` color scaled to a brightness level, as WS2812 GRB bytes.
#[must_use]
pub fn ws2812_grb(color: u32, level: u8) -> [u8; 3] {
    let [_, r, g, b] = color.to_be_bytes();
    let scale = |c: u8| {
        #[allow(clippy::cast_possible_truncation)] // at most 255
        let v = (u16::from(c) * u16::from(level) / 255) as u8;
        v
    };
    [scale(g), scale(r), scale(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_mode() {
        for continuous in [true, false] {
            let pattern = |mode, failing| Pattern::for_mode(mode, continuous, failing);
            assert_eq!(pattern("off", true), Pattern::Dark);
            assert_eq!(pattern("errors", false), Pattern::Dark);
            assert_eq!(pattern("errors", true), Pattern::Alarm);
            assert_eq!(pattern("on", true), Pattern::Alarm);
        }
        assert_eq!(Pattern::for_mode("on", true, false), Pattern::Breath);
        assert_eq!(Pattern::for_mode("on", false, false), Pattern::Steady);
    }

    #[test]
    fn breathes_then_pauses() {
        let breath = Pattern::Breath;
        assert_eq!(breath.level(0), 0);
        assert_eq!(breath.level(BREATH_MS / 2), 255);
        assert_eq!(breath.level(BREATH_MS), 0);
        assert_eq!(breath.level(breath.period_ms() - 1), 0);
        assert_eq!(breath.level(breath.period_ms() + BREATH_MS / 2), 255);
        // Rises steadily to the peak.
        let rise: Vec<u8> = (0..=BREATH_MS / 2)
            .step_by(100)
            .map(|ms| breath.level(ms))
            .collect();
        assert!(rise.windows(2).all(|w| w[0] <= w[1]), "{rise:?}");
    }

    #[test]
    fn blinks_twice() {
        let alarm = Pattern::Alarm;
        let lit: Vec<u32> = (0..alarm.period_ms())
            .step_by(20)
            .filter(|&ms| alarm.level(ms) > 0)
            .collect();
        assert_eq!(lit, [0, 20, 40, 60, 80, 100, 360, 380, 400, 420, 440, 460]);
        assert_eq!(alarm.level(alarm.period_ms()), 255);
    }

    #[test]
    fn applies_the_choice() {
        for compiled in ["on", "errors", "off"] {
            assert_eq!(mode(compiled, None), compiled);
            assert_eq!(mode(compiled, Some(true)), "on");
        }
        assert_eq!(mode("on", Some(false)), "errors");
        assert_eq!(mode("errors", Some(false)), "errors");
        assert_eq!(mode("off", Some(false)), "off");
    }

    #[test]
    fn shows_the_press() {
        for preview in [Pattern::Breath, Pattern::Dark] {
            let pattern = |pending| Pattern::for_press(pending, preview);
            assert_eq!(pattern(None), None);
            assert_eq!(pattern(Some((Gesture::Click, 1000))), None);
            assert_eq!(pattern(Some((Gesture::Hold, 0))), Some(Pattern::Flash));
            assert_eq!(pattern(Some((Gesture::Hold, 149))), Some(Pattern::Flash));
            assert_eq!(pattern(Some((Gesture::Hold, 150))), Some(preview));
            assert_eq!(pattern(Some((Gesture::Reset, 0))), Some(Pattern::Rapid));
        }
    }

    #[test]
    fn flashes() {
        let flash = Pattern::Flash;
        assert!((0..2 * flash.period_ms())
            .step_by(20)
            .all(|ms| flash.level(ms) == 255));
    }

    #[test]
    fn blinks_rapidly() {
        let rapid = Pattern::Rapid;
        let lit: Vec<u32> = (0..2 * rapid.period_ms())
            .step_by(20)
            .filter(|&ms| rapid.level(ms) > 0)
            .collect();
        assert_eq!(lit, [0, 20, 40, 60, 80, 200, 220, 240, 260, 280]);
    }

    #[test]
    fn blinks_the_grade() {
        // When each blink starts.
        let blinks = |grade| {
            let status = Pattern::Status(grade);
            (0..=status.period_ms())
                .step_by(10)
                .filter(|&ms| status.level(ms) > 0 && (ms == 0 || status.level(ms - 10) == 0))
                .collect::<Vec<u32>>()
        };
        assert_eq!(blinks(Grade::Good), [0, 2000]);
        assert_eq!(blinks(Grade::Fair), [0, 400, 2000]);
        assert_eq!(blinks(Grade::Poor), [0, 400, 800, 2000]);
        let poor = Pattern::Status(Grade::Poor);
        assert_eq!(poor.level(949), 255);
        assert_eq!(poor.level(950), 0);
    }

    #[test]
    fn colors_the_ws2812() {
        let color = |pattern: Pattern| pattern.ws2812_color(0x00_00_20);
        for pattern in [Pattern::Dark, Pattern::Steady, Pattern::Breath] {
            assert_eq!(color(pattern), 0x00_00_20);
        }
        assert_eq!(color(Pattern::Alarm), 0x80_00_00);
        assert_eq!(color(Pattern::Flash), 0x40_40_40);
        assert_eq!(color(Pattern::Rapid), 0x40_40_40);
        assert_eq!(color(Pattern::Status(Grade::Good)), 0x00_40_00);
        assert_eq!(color(Pattern::Status(Grade::Fair)), 0x40_18_00);
        assert_eq!(color(Pattern::Status(Grade::Poor)), 0x40_00_00);
    }

    #[test]
    fn dims_a_plain_led() {
        for pattern in [Pattern::Dark, Pattern::Steady, Pattern::Breath] {
            assert_eq!(pattern.gpio_divisor(), 8);
        }
        assert_eq!(Pattern::Alarm.gpio_divisor(), 2);
        assert_eq!(Pattern::Status(Grade::Fair).gpio_divisor(), 2);
        assert_eq!(Pattern::Flash.gpio_divisor(), 1);
        assert_eq!(Pattern::Rapid.gpio_divisor(), 1);
    }

    #[test]
    fn scales_the_color() {
        assert_eq!(ws2812_grb(0x10_20_30, 255), [0x20, 0x10, 0x30]);
        assert_eq!(ws2812_grb(0x10_20_30, 0), [0, 0, 0]);
        assert_eq!(ws2812_grb(0xFF_00_80, 128), [0, 128, 64]);
    }
}
