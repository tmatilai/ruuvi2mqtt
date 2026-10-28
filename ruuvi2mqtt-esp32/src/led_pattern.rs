//! The LED's patterns, as brightness over time.

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
}

/// How long a breath lasts, in ms. The LED is dark for the rest of its
/// period.
const BREATH_MS: u32 = 2000;

/// How long an alarm blink lasts, and when the second one starts, in ms.
const BLINK_MS: u32 = 120;
const SECOND_BLINK_MS: u32 = 360;

impl Pattern {
    /// The LED's pattern for `LED_MODE`.
    #[must_use]
    pub fn for_mode(mode: Option<&str>, continuous: bool, failing: bool) -> Self {
        match mode {
            Some("on" | "errors") if failing => Self::Alarm,
            Some("on") if continuous => Self::Breath,
            Some("on") => Self::Steady,
            _ => Self::Dark,
        }
    }

    /// How often the pattern repeats, in ms.
    #[must_use]
    pub const fn period_ms(self) -> u32 {
        match self {
            Self::Dark | Self::Steady => 1000,
            Self::Breath => 6000,
            Self::Alarm => 3000,
        }
    }

    /// The brightness, 0–255, `ms` into the pattern.
    #[must_use]
    pub fn level(self, ms: u32) -> u8 {
        let t = ms % self.period_ms();
        match self {
            Self::Steady => 255,
            Self::Breath if t < BREATH_MS => {
                let x = f64::from(t) / f64::from(BREATH_MS);
                // A raised cosine, squared, as the eye sees dim light as
                // brighter
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
            Self::Dark | Self::Breath | Self::Alarm => 0,
        }
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
            assert_eq!(pattern(None, true), Pattern::Dark);
            assert_eq!(pattern(Some("off"), true), Pattern::Dark);
            assert_eq!(pattern(Some("errors"), false), Pattern::Dark);
            assert_eq!(pattern(Some("errors"), true), Pattern::Alarm);
            assert_eq!(pattern(Some("on"), true), Pattern::Alarm);
        }
        assert_eq!(Pattern::for_mode(Some("on"), true, false), Pattern::Breath);
        assert_eq!(Pattern::for_mode(Some("on"), false, false), Pattern::Steady);
    }

    #[test]
    fn breathes_then_pauses() {
        let breath = Pattern::Breath;
        assert_eq!(breath.level(0), 0);
        assert_eq!(breath.level(BREATH_MS / 2), 255);
        assert_eq!(breath.level(BREATH_MS), 0);
        assert_eq!(breath.level(breath.period_ms() - 1), 0);
        assert_eq!(breath.level(breath.period_ms() + BREATH_MS / 2), 255);
        // Rises steadily to the peak
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
    fn scales_the_color() {
        assert_eq!(ws2812_grb(0x10_20_30, 255), [0x20, 0x10, 0x30]);
        assert_eq!(ws2812_grb(0x10_20_30, 0), [0, 0, 0]);
        assert_eq!(ws2812_grb(0xFF_00_80, 128), [0, 128, 64]);
    }
}
