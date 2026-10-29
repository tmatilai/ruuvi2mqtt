//! How long to hold a reading before the main loop checks the throttle.
//!
//! Instances that hear the same tag share its throttle timer over MQTT, so
//! their timers expire together. The first to publish after that wins, and
//! the others see its message before their own delay ends. A stronger signal
//! gets a shorter delay, so the closest instance usually publishes.

use std::time::Duration;

use rand::RngExt;

/// Signals at or above this all get no RSSI delay. A tag this strong is
/// within about a meter.
const RSSI_STRONG: i16 = -40;
/// Signals at or below this, and readings without RSSI, get the full RSSI
/// delay. BLE receivers lose packets around this level.
const RSSI_WEAK: i16 = -100;
/// So -40..-100 dBm maps to 0..600 ms.
const MS_PER_DB: u64 = 10;
/// Breaks ties between instances with a similar signal. 100 ms equals 10 dB,
/// about the per-packet fading spread, so instances more than ~10 dB apart
/// keep a stable order and closer ones alternate.
const MAX_JITTER: Duration = Duration::from_millis(100);

/// Maps the RSSI to a delay and adds `jitter`.
pub fn publish_delay(rssi: Option<i16>, jitter: Duration) -> Duration {
    let rssi = rssi.unwrap_or(RSSI_WEAK).clamp(RSSI_WEAK, RSSI_STRONG);
    let db_below_strong = u64::from(RSSI_STRONG.abs_diff(rssi));
    Duration::from_millis(db_below_strong * MS_PER_DB) + jitter
}

/// Drawn fresh for each reading, so a tie never repeats.
pub fn random_jitter() -> Duration {
    rand::rng().random_range(Duration::ZERO..=MAX_JITTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn strong_signal_has_no_rssi_delay() {
        assert_eq!(publish_delay(Some(-40), Duration::ZERO), ms(0));
        assert_eq!(publish_delay(Some(-20), Duration::ZERO), ms(0));
        assert_eq!(publish_delay(Some(10), Duration::ZERO), ms(0));
    }

    #[test]
    fn weak_signal_has_full_rssi_delay() {
        assert_eq!(publish_delay(Some(-100), Duration::ZERO), ms(600));
        assert_eq!(publish_delay(Some(-127), Duration::ZERO), ms(600));
        assert_eq!(publish_delay(Some(i16::MIN), Duration::ZERO), ms(600));
    }

    #[test]
    fn missing_rssi_has_full_rssi_delay() {
        assert_eq!(publish_delay(None, Duration::ZERO), ms(600));
    }

    #[test]
    fn delay_is_linear_in_rssi() {
        assert_eq!(publish_delay(Some(-41), Duration::ZERO), ms(10));
        assert_eq!(publish_delay(Some(-70), Duration::ZERO), ms(300));
        assert_eq!(publish_delay(Some(-99), Duration::ZERO), ms(590));
    }

    #[test]
    fn stronger_signal_never_waits_longer() {
        for rssi in -130..=0 {
            assert!(
                publish_delay(Some(rssi + 1), Duration::ZERO)
                    <= publish_delay(Some(rssi), Duration::ZERO)
            );
        }
    }

    #[test]
    fn jitter_is_added() {
        assert_eq!(publish_delay(Some(-70), ms(42)), ms(342));
        assert_eq!(publish_delay(None, ms(42)), ms(642));
    }

    #[test]
    fn jitter_stays_in_range() {
        for _ in 0..1000 {
            assert!(random_jitter() <= MAX_JITTER);
        }
    }

    #[test]
    fn max_delay_is_well_below_advertising_interval() {
        // RuuviTags advertise about once a second. A shorter delay keeps
        // readings from overtaking each other.
        assert_eq!(publish_delay(None, MAX_JITTER), ms(700));
    }
}
