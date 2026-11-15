//! The button's gestures, from its level over time.

/// What a press does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Shows the status on the LED.
    Click,
    /// Toggles the LED between `on` and a dark mode.
    Hold,
    /// Resets the LED to the compiled `LED_MODE`.
    Reset,
}

/// How long a press takes to become each gesture, in ms. A shorter press is
/// noise.
const CLICK_MS: u64 = 40;
const HOLD_MS: u64 = 2000;
const RESET_MS: u64 = 10_000;

#[derive(Debug, Default)]
enum State {
    /// Not released yet: a button that is held since boot, or a shorted pin,
    /// is no press.
    #[default]
    Blocked,
    Released,
    /// Since this time, in ms.
    Pressed(u64),
}

/// Turns the button's levels into gestures. A gesture runs on release.
#[derive(Debug, Default)]
pub struct Button {
    state: State,
}

impl Button {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Blocked,
        }
    }

    /// Takes the button's level at `now_ms`. Returns the gesture when the
    /// button is released.
    pub fn update(&mut self, pressed: bool, now_ms: u64) -> Option<Gesture> {
        if pressed {
            if matches!(self.state, State::Released) {
                self.state = State::Pressed(now_ms);
            }
            return None;
        }
        let gesture = self.pending(now_ms).map(|(gesture, _)| gesture);
        self.state = State::Released;
        gesture
    }

    /// Returns the gesture that a release now gives, and for how many ms the
    /// press has been that gesture.
    #[must_use]
    pub fn pending(&self, now_ms: u64) -> Option<(Gesture, u64)> {
        let State::Pressed(since) = self.state else {
            return None;
        };
        let pressed_ms = now_ms.saturating_sub(since);
        [
            (Gesture::Reset, RESET_MS),
            (Gesture::Hold, HOLD_MS),
            (Gesture::Click, CLICK_MS),
        ]
        .into_iter()
        .find_map(|(gesture, from)| Some((gesture, pressed_ms.checked_sub(from)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn released() -> Button {
        let mut button = Button::new();
        assert_eq!(button.update(false, 0), None);
        button
    }

    /// Presses from `from` until the release at `to`, sampled every 20 ms.
    fn press(button: &mut Button, from: u64, to: u64) -> Option<Gesture> {
        for now in (from..to).step_by(20) {
            assert_eq!(button.update(true, now), None);
        }
        button.update(false, to)
    }

    #[test]
    fn clicks() {
        let mut button = released();
        assert_eq!(press(&mut button, 100, 140), Some(Gesture::Click));
        assert_eq!(press(&mut button, 1000, 2980), Some(Gesture::Click));
        // Only the release gives the gesture.
        assert_eq!(button.update(false, 3000), None);
    }

    #[test]
    fn holds() {
        let mut button = released();
        assert_eq!(press(&mut button, 100, 2100), Some(Gesture::Hold));
        assert_eq!(press(&mut button, 20_000, 29_980), Some(Gesture::Hold));
    }

    #[test]
    fn resets() {
        let mut button = released();
        assert_eq!(press(&mut button, 100, 10_100), Some(Gesture::Reset));
        assert_eq!(press(&mut button, 20_000, 80_000), Some(Gesture::Reset));
    }

    #[test]
    fn ignores_noise() {
        let mut button = released();
        assert_eq!(press(&mut button, 100, 120), None);
        assert_eq!(press(&mut button, 200, 239), None);
    }

    #[test]
    fn a_bounce_restarts_the_press() {
        let mut button = released();
        assert_eq!(button.update(true, 100), None);
        assert_eq!(button.update(false, 120), None);
        // A hold when counted from the first press.
        assert_eq!(press(&mut button, 140, 2120), Some(Gesture::Click));
    }

    #[test]
    fn held_from_boot_is_no_press() {
        let mut button = Button::new();
        assert_eq!(button.update(true, 0), None);
        assert_eq!(button.pending(12_000), None);
        assert_eq!(button.update(false, 12_000), None);
        assert_eq!(press(&mut button, 13_000, 13_100), Some(Gesture::Click));
    }

    #[test]
    fn pending_counts_from_each_threshold() {
        let mut button = released();
        assert_eq!(button.pending(1000), None);
        button.update(true, 1000);
        assert_eq!(button.pending(1039), None);
        assert_eq!(button.pending(1040), Some((Gesture::Click, 0)));
        assert_eq!(button.pending(2999), Some((Gesture::Click, 1959)));
        assert_eq!(button.pending(3000), Some((Gesture::Hold, 0)));
        assert_eq!(button.pending(10_999), Some((Gesture::Hold, 7999)));
        assert_eq!(button.pending(11_000), Some((Gesture::Reset, 0)));
        assert_eq!(button.pending(11_500), Some((Gesture::Reset, 500)));
        button.update(false, 11_500);
        assert_eq!(button.pending(11_500), None);
    }
}
