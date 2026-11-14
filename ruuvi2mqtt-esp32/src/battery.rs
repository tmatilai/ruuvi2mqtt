//! The battery gauges that `BATTERY` names, and their readings.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gauge {
    /// The MAX17048 fuel gauge.
    Max17048,
    /// The M5PM1 power chip, which reports no level.
    M5pm1,
}

impl Gauge {
    #[must_use]
    pub const fn address(self) -> u8 {
        match self {
            Self::Max17048 => 0x36,
            Self::M5pm1 => 0x6E,
        }
    }

    #[must_use]
    pub const fn has_level(self) -> bool {
        matches!(self, Self::Max17048)
    }

    #[must_use]
    pub const fn has_usb(self) -> bool {
        matches!(self, Self::M5pm1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Battery {
    pub mv: u16,
    /// Percent. The MAX17048 may report above 100 while it learns the cell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    /// Percent per hour, positive while charging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f32>,
    /// Whether USB powers the board, which then charges the battery or keeps
    /// it full.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb: Option<bool>,
}

impl Battery {
    /// From the MAX17048's VCELL, SOC and CRATE registers, high byte first.
    #[must_use]
    pub fn max17048(vcell: [u8; 2], soc: [u8; 2], crate_: [u8; 2]) -> Self {
        // 78.125 µV per unit, 5/64 mV.
        let mv = (u32::from(u16::from_be_bytes(vcell)) * 5 + 32) / 64;
        Self {
            mv: u16::try_from(mv).unwrap_or(u16::MAX),
            level: Some(tenths(f32::from(u16::from_be_bytes(soc)) / 256.0)),
            rate: Some(tenths(f32::from(i16::from_be_bytes(crate_)) * 0.208)),
            usb: None,
        }
    }

    /// From the M5PM1's battery and USB input voltage registers, in mV, low
    /// byte first. `None` for all ones, which the chip sometimes sends while
    /// it wakes up.
    #[must_use]
    pub fn m5pm1(vbat: [u8; 2], vin: [u8; 2]) -> Option<Self> {
        let (vbat, vin) = (u16::from_le_bytes(vbat), u16::from_le_bytes(vin));
        if vbat == u16::MAX || vin == u16::MAX {
            return None;
        }
        Some(Self {
            mv: vbat,
            level: None,
            rate: None,
            // USB gives 5 V.
            usb: Some(vin > USB_MIN_MV),
        })
    }
}

/// Whether the battery is charging, full or draining.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Charging,
    Full,
    Discharging,
}

impl Battery {
    /// `None` when the reading doesn't say.
    #[must_use]
    pub fn state(&self) -> Option<State> {
        match (self.usb, self.level, self.rate) {
            (Some(true), ..) if self.mv >= FULL_MV => Some(State::Full),
            (Some(true), ..) => Some(State::Charging),
            (Some(false), ..) => Some(State::Discharging),
            (None, Some(level), Some(rate)) if level >= 99.5 && rate >= 0.0 => Some(State::Full),
            (None, _, Some(rate)) if rate > 0.0 => Some(State::Charging),
            (None, _, Some(rate)) if rate < 0.0 => Some(State::Discharging),
            _ => None,
        }
    }

    /// A rough level from the voltage, for a gauge that reports none. Only on
    /// battery, since charging raises the voltage.
    #[must_use]
    pub fn estimated_level(&self) -> Option<f32> {
        if self.level.is_some() || self.usb != Some(false) {
            return None;
        }
        let level = match CURVE.windows(2).find(|w| self.mv < w[1].0) {
            Some(&[(mv0, l0), (mv1, l1)]) if self.mv >= mv0 => {
                l0 + (l1 - l0) * f32::from(self.mv - mv0) / f32::from(mv1 - mv0)
            }
            Some(_) => 0.0,
            None => 100.0,
        };
        Some(level.round())
    }
}

/// Above any battery voltage, below USB's 5 V.
const USB_MIN_MV: u16 = 4500;

/// On USB, a cell counts as full from this voltage on. The charger holds it
/// at about 4.2 V, also while it tops the cell up, and the M5PM1 doesn't say
/// which.
const FULL_MV: u16 = 4150;

/// A Li-ion cell's voltage at rest, in mV, and its charge in percent. The
/// board's load lowers the voltage, so the estimate reads a bit low.
const CURVE: [(u16, f32); 12] = [
    (3270, 0.0),
    (3610, 5.0),
    (3690, 10.0),
    (3730, 20.0),
    (3770, 30.0),
    (3800, 40.0),
    (3840, 50.0),
    (3870, 60.0),
    (3950, 70.0),
    (4020, 80.0),
    (4110, 90.0),
    (4200, 100.0),
];

/// The gauges' resolution is finer than their accuracy.
fn tenths(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max17048() {
        // 3.9 V, 81.5 %, discharging 1.04 %/h
        let battery = Battery::max17048([0xC3, 0x00], [0x51, 0x80], (-5i16).to_be_bytes());
        assert_eq!(
            battery,
            Battery {
                mv: 3900,
                level: Some(81.5),
                rate: Some(-1.0),
                usb: None,
            }
        );
        assert_eq!(
            serde_json::to_string(&battery).unwrap(),
            r#"{"mv":3900,"level":81.5,"rate":-1.0}"#
        );
    }

    #[test]
    fn max17048_extremes() {
        let battery = Battery::max17048([0xFF, 0xFF], [0xFF, 0xFF], i16::MAX.to_be_bytes());
        assert_eq!(battery.mv, 5120);
        assert_eq!(battery.level, Some(256.0));
        assert_eq!(battery.rate, Some(6815.5));
    }

    #[test]
    fn m5pm1() {
        let battery = Battery::m5pm1(4012u16.to_le_bytes(), 0u16.to_le_bytes()).unwrap();
        assert_eq!(
            serde_json::to_string(&battery).unwrap(),
            r#"{"mv":4012,"usb":false}"#
        );
        let battery = Battery::m5pm1(4150u16.to_le_bytes(), 5020u16.to_le_bytes()).unwrap();
        assert_eq!(battery.usb, Some(true));
        assert_eq!(Battery::m5pm1([0xFF; 2], 5020u16.to_le_bytes()), None);
        assert_eq!(Battery::m5pm1(4150u16.to_le_bytes(), [0xFF; 2]), None);
    }

    fn stick(mv: u16, usb: bool) -> Battery {
        Battery::m5pm1(
            mv.to_le_bytes(),
            (if usb { 5100u16 } else { 0 }).to_le_bytes(),
        )
        .unwrap()
    }

    fn feather(level: f32, rate: f32) -> Battery {
        Battery {
            mv: 4000,
            level: Some(level),
            rate: Some(rate),
            usb: None,
        }
    }

    #[test]
    fn state() {
        assert_eq!(stick(4184, true).state(), Some(State::Full));
        assert_eq!(stick(4000, true).state(), Some(State::Charging));
        assert_eq!(stick(4184, false).state(), Some(State::Discharging));
        assert_eq!(feather(100.2, 0.0).state(), Some(State::Full));
        assert_eq!(feather(81.4, 2.9).state(), Some(State::Charging));
        assert_eq!(feather(81.4, -1.0).state(), Some(State::Discharging));
        assert_eq!(feather(81.4, 0.0).state(), None);
    }

    #[test]
    fn estimated_level() {
        assert_eq!(stick(3000, false).estimated_level(), Some(0.0));
        assert_eq!(stick(3270, false).estimated_level(), Some(0.0));
        assert_eq!(stick(3800, false).estimated_level(), Some(40.0));
        assert_eq!(stick(4012, false).estimated_level(), Some(79.0));
        assert_eq!(stick(4200, false).estimated_level(), Some(100.0));
        assert_eq!(stick(4300, false).estimated_level(), Some(100.0));
        // Charging raises the voltage, and the gauge has a level of its own.
        assert_eq!(stick(4012, true).estimated_level(), None);
        assert_eq!(feather(81.4, -1.0).estimated_level(), None);
    }
}
