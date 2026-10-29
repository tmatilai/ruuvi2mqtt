//! A Ruuvi Air's advertisements: data formats 6 and E1, and the advertising
//! data around them. No ESP-IDF here, so the tests run on the host.
//!
//! Layouts: <https://docs.ruuvi.com/communication/bluetooth-advertisements>.
//! The advertising data mirrors the Air firmware's `src/ble_adv.c`.

const MANUFACTURER_ID: [u8; 2] = 0x0499_u16.to_le_bytes();
const SERVICE_UUID: [u8; 2] = 0xFC98_u16.to_le_bytes();

const AD_FLAGS: u8 = 0x01;
const AD_UUID16_ALL: u8 = 0x03;
const AD_MANUFACTURER_DATA: u8 = 0xFF;
/// LE General Discoverable, BR/EDR not supported.
const FLAGS_GENERAL_NO_BREDR: u8 = 0x06;

const FLAG_CALIBRATING: u8 = 0x01;
const FLAG_VOC_BIT8: u8 = 0x40;
const FLAG_NOX_BIT8: u8 = 0x80;

/// One sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// °C
    pub temperature: f64,
    /// %
    pub humidity: f64,
    /// Pa
    pub pressure: u32,
    /// µg/m³
    pub pm1: f64,
    pub pm2_5: f64,
    pub pm4: f64,
    pub pm10: f64,
    /// ppm
    pub co2: u16,
    /// Sensirion index, 1–500
    pub voc: u16,
    pub nox: u16,
    /// lx
    pub luminosity: f64,
    /// 24 bits in E1; format 6 sends the low byte.
    pub sequence: u32,
    pub calibrating: bool,
}

impl Reading {
    /// The values of the Air firmware's mock build (`src/sensors.c`), with a
    /// temperature that climbs 0.05 °C per sample and wraps each minute, so
    /// consecutive readings differ.
    #[must_use]
    pub fn simulated(sequence: u32) -> Self {
        let sequence = sequence & 0x00FF_FFFF;
        Self {
            temperature: 21.0 + f64::from(sequence % 60) * 0.05,
            humidity: 45.0,
            pressure: 101_325,
            pm1: 11.0,
            pm2_5: 11.4,
            pm4: 11.5,
            pm10: 11.6,
            co2: 886,
            voc: 160,
            nox: 10,
            luminosity: 79.4,
            sequence,
            calibrating: false,
        }
    }

    /// Format 6, after the manufacturer ID: 20 bytes.
    #[must_use]
    pub fn format6(&self, mac: &[u8; 6]) -> [u8; 20] {
        let mut b = [0; 20];
        b[0] = 0x06;
        self.write_climate(&mut b[1..7]);
        b[7..9].copy_from_slice(&pm(self.pm2_5));
        b[9..11].copy_from_slice(&self.co2.to_be_bytes());
        b[11] = low_byte(self.voc);
        b[12] = low_byte(self.nox);
        b[13] = luminosity_code(self.luminosity);
        // b[14] is reserved.
        b[15] = low_byte(self.sequence);
        b[16] = self.flags();
        b[17..20].copy_from_slice(&mac[3..]);
        b
    }

    /// Format E1, after the manufacturer ID: 40 bytes.
    #[must_use]
    pub fn e1(&self, mac: &[u8; 6]) -> [u8; 40] {
        let mut b = [0; 40];
        b[0] = 0xE1;
        self.write_climate(&mut b[1..7]);
        b[7..9].copy_from_slice(&pm(self.pm1));
        b[9..11].copy_from_slice(&pm(self.pm2_5));
        b[11..13].copy_from_slice(&pm(self.pm4));
        b[13..15].copy_from_slice(&pm(self.pm10));
        b[15..17].copy_from_slice(&self.co2.to_be_bytes());
        b[17] = low_byte(self.voc);
        b[18] = low_byte(self.nox);
        b[19..22].copy_from_slice(&u24(scale(self.luminosity, 100.0)));
        // b[22..25] is reserved.
        b[25..28].copy_from_slice(&u24(self.sequence));
        b[28] = self.flags();
        // b[29..34] is reserved.
        b[34..40].copy_from_slice(mac);
        b
    }

    /// Temperature, humidity and pressure, the same in both formats.
    fn write_climate(&self, b: &mut [u8]) {
        #[expect(clippy::cast_possible_truncation, reason = "within ±163 °C")]
        let temperature = (self.temperature * 200.0).round() as i16;
        b[0..2].copy_from_slice(&temperature.to_be_bytes());
        b[2..4].copy_from_slice(&u16_scaled(self.humidity, 400.0));
        let pressure = u16::try_from(self.pressure.saturating_sub(50_000)).unwrap_or(u16::MAX - 1);
        b[4..6].copy_from_slice(&pressure.to_be_bytes());
    }

    fn flags(&self) -> u8 {
        let mut flags = 0;
        if self.calibrating {
            flags |= FLAG_CALIBRATING;
        }
        if self.voc & 0x100 != 0 {
            flags |= FLAG_VOC_BIT8;
        }
        if self.nox & 0x100 != 0 {
            flags |= FLAG_NOX_BIT8;
        }
        flags
    }
}

/// The legacy advertising data: flags, the Ruuvi service UUID, and format 6.
/// 31 bytes, the legacy maximum.
#[must_use]
pub fn legacy_adv_data(reading: &Reading, mac: &[u8; 6]) -> Vec<u8> {
    let mut data = vec![2, AD_FLAGS, FLAGS_GENERAL_NO_BREDR];
    push_uuid(&mut data);
    push_manufacturer_data(&mut data, &reading.format6(mac));
    data
}

/// The extended advertising data: E1, then the Ruuvi service UUID.
#[must_use]
pub fn extended_adv_data(reading: &Reading, mac: &[u8; 6]) -> Vec<u8> {
    let mut data = Vec::with_capacity(48);
    push_manufacturer_data(&mut data, &reading.e1(mac));
    push_uuid(&mut data);
    data
}

/// The name in the legacy scan response, from the last two MAC bytes.
#[must_use]
pub fn name(mac: &[u8; 6]) -> String {
    format!("Ruuvi {:02X}{:02X}", mac[4], mac[5])
}

fn push_uuid(data: &mut Vec<u8>) {
    data.extend([3, AD_UUID16_ALL]);
    data.extend(SERVICE_UUID);
}

fn push_manufacturer_data(data: &mut Vec<u8>, payload: &[u8]) {
    let len = u8::try_from(1 + MANUFACTURER_ID.len() + payload.len()).expect("fits in an AD");
    data.extend([len, AD_MANUFACTURER_DATA]);
    data.extend(MANUFACTURER_ID);
    data.extend(payload);
}

/// Format 6's luminosity: a log scale from 0 to 65535 lx in 254 steps.
fn luminosity_code(lux: f64) -> u8 {
    let code = ((lux.max(0.0) + 1.0).ln() * 254.0 / 65536_f64.ln()).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped"
    )]
    let code = code.min(254.0) as u8;
    code
}

/// PM in 0.1 µg/m³.
fn pm(value: f64) -> [u8; 2] {
    u16_scaled(value, 10.0)
}

fn u16_scaled(value: f64, factor: f64) -> [u8; 2] {
    let value = u16::try_from(scale(value, factor)).unwrap_or(u16::MAX - 1);
    value.to_be_bytes()
}

fn scale(value: f64, factor: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped"
    )]
    let value = (value * factor).round().clamp(0.0, f64::from(u32::MAX)) as u32;
    value
}

fn u24(value: u32) -> [u8; 3] {
    let [_, b @ ..] = value.min(0x00FF_FFFE).to_be_bytes();
    b
}

fn low_byte(value: impl Into<u32>) -> u8 {
    value.into().to_le_bytes()[0]
}

/// Which advertising sets to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sets {
    /// Format 6, legacy, on the 1M PHY.
    pub legacy: bool,
    /// E1, extended, 1M primary and 2M secondary PHY.
    pub extended: bool,
    /// E1, extended, on the Coded PHY.
    pub coded: bool,
}

impl Sets {
    pub const ALL: Self = Self {
        legacy: true,
        extended: true,
        coded: true,
    };
    pub const NONE: Self = Self {
        legacy: false,
        extended: false,
        coded: false,
    };

    /// Parses a comma-separated list of `legacy`, `extended` and `coded`.
    ///
    /// # Errors
    ///
    /// On an unknown name or an empty list.
    pub fn parse(list: &str) -> Result<Self, String> {
        let mut sets = Self::NONE;
        for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match name {
                "legacy" => sets.legacy = true,
                "extended" => sets.extended = true,
                "coded" => sets.coded = true,
                _ => return Err(format!("unknown advertising set {name:?}")),
            }
        }
        if sets == Self::NONE {
            return Err("no advertising sets".to_string());
        }
        Ok(sets)
    }
}

/// Parses a MAC like `C0:FF:EE:00:A1:12`. An Air's address is static
/// random, so the two top bits must be set.
///
/// # Errors
///
/// When the MAC is malformed or not a static random address.
pub fn parse_mac(text: &str) -> Result<[u8; 6], String> {
    let mut mac = [0; 6];
    let mut parts = text.split(':');
    for byte in &mut mac {
        let part = parts
            .next()
            .ok_or_else(|| format!("too short MAC {text:?}"))?;
        *byte = u8::from_str_radix(part, 16).map_err(|_| format!("bad MAC {text:?}"))?;
    }
    if parts.next().is_some() {
        return Err(format!("too long MAC {text:?}"));
    }
    if mac[0] & 0xC0 != 0xC0 {
        return Err(format!("{text} is not a static random address"));
    }
    Ok(mac)
}

/// A static random address from a chip's MAC, so each board gets its own.
#[must_use]
pub fn static_random(mut mac: [u8; 6]) -> [u8; 6] {
    mac[0] |= 0xC0;
    mac
}

#[must_use]
pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.map(|b| format!("{b:02X}")).join(":")
}

/// How often the LED breathes, in ms.
pub const PULSE_PERIOD_MS: u32 = 6000;
/// How long one breath lasts, in ms. The LED is dark for the rest.
pub const PULSE_MS: u32 = 2000;

/// The LED's brightness, 0–255, `ms` into its cycle: a slow breath that
/// rises and falls, then a dark pause.
#[must_use]
pub fn pulse_level(ms: u32) -> u8 {
    let t = ms % PULSE_PERIOD_MS;
    if t >= PULSE_MS {
        return 0;
    }
    let x = f64::from(t) / f64::from(PULSE_MS);
    // A raised cosine, squared, as the eye sees dim light as brighter
    let s = (1.0 - (std::f64::consts::TAU * x).cos()) / 2.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0–255
    let level = (s * s * 255.0).round() as u8;
    level
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

/// Parses an `RRGGBB` color.
///
/// # Errors
///
/// When it isn't six hex digits.
pub fn parse_color(text: &str) -> Result<u32, String> {
    if text.len() != 6 {
        return Err(format!("color '{text}' isn't six hex digits, RRGGBB"));
    }
    u32::from_str_radix(text, 16)
        .map_err(|_| format!("color '{text}' isn't six hex digits, RRGGBB"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruuvi_sensor_protocol::{
        CarbonDioxide, Humidity, Luminosity, MacAddress, MeasurementSequenceNumber, NitrogenOxides,
        ParticulateMatter, Pressure, SensorValues, Temperature, VolatileOrganicCompounds,
    };

    const MAC: [u8; 6] = [0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F];

    /// The values of the specs' valid test vectors.
    fn spec_reading() -> Reading {
        Reading {
            temperature: 29.5,
            humidity: 55.3,
            pressure: 101_102,
            pm1: 10.1,
            pm2_5: 11.2,
            pm4: 121.3,
            pm10: 455.4,
            co2: 201,
            voc: 10,
            nox: 2,
            luminosity: 13_027.0,
            sequence: 0x00DE_CDEE,
            calibrating: true,
        }
    }

    fn decode(adv_data: &[u8]) -> SensorValues {
        // Find the manufacturer data AD structure.
        let mut rest = adv_data;
        while let [len, ad_type, ..] = *rest {
            let (ad, tail) = rest.split_at(usize::from(len) + 1);
            if ad_type == AD_MANUFACTURER_DATA {
                let id = u16::from_le_bytes([ad[2], ad[3]]);
                return SensorValues::from_manufacturer_specific_data(id, &ad[4..]).unwrap();
            }
            rest = tail;
        }
        panic!("no manufacturer data in {adv_data:02X?}");
    }

    #[test]
    fn e1_matches_the_spec_vector() {
        let expected = [
            0xE1, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x65, 0x00, 0x70, 0x04, 0xBD, 0x11,
            0xCA, 0x00, 0xC9, 0x0A, 0x02, 0x13, 0xE0, 0xAC, 0x00, 0x00, 0x00, 0xDE, 0xCD, 0xEE,
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xCB, 0xB8, 0x33, 0x4C, 0x88, 0x4F,
        ];
        assert_eq!(spec_reading().e1(&MAC), expected);
    }

    #[test]
    fn format6_matches_the_spec_vector() {
        let reading = Reading {
            voc: 5,
            nox: 1,
            luminosity: 13_026.67,
            sequence: 0xCD,
            calibrating: false,
            ..spec_reading()
        };
        let expected = [
            0x06, 0x17, 0x0C, 0x56, 0x68, 0xC7, 0x9E, 0x00, 0x70, 0x00, 0xC9, 0x05, 0x01, 0xD9,
            0x00, 0xCD, 0x00, 0x4C, 0x88, 0x4F,
        ];
        assert_eq!(reading.format6(&MAC), expected);
    }

    #[test]
    fn high_indexes_set_the_flag_bits() {
        let reading = Reading {
            voc: 500,
            nox: 300,
            ..spec_reading()
        };
        for values in [
            decode(&legacy_adv_data(&reading, &MAC)),
            decode(&extended_adv_data(&reading, &MAC)),
        ] {
            assert_eq!(values.voc_index(), Some(500));
            assert_eq!(values.nox_index(), Some(300));
        }
    }

    #[test]
    fn simulated_readings_decode() {
        let reading = Reading::simulated(0x0123_4567);
        let legacy = legacy_adv_data(&reading, &MAC);
        let extended = extended_adv_data(&reading, &MAC);
        assert_eq!(legacy.len(), 31);
        assert_eq!(extended.len(), 48);

        let e1 = decode(&extended);
        assert_eq!(e1.temperature_as_millicelsius(), Some(22_350));
        assert_eq!(e1.humidity_as_ppm(), Some(450_000));
        assert_eq!(e1.pressure_as_pascals(), Some(101_325));
        assert_eq!(e1.pm1_0_as_nanograms_per_cubic_meter(), Some(11_000));
        assert_eq!(e1.pm2_5_as_nanograms_per_cubic_meter(), Some(11_400));
        assert_eq!(e1.pm4_0_as_nanograms_per_cubic_meter(), Some(11_500));
        assert_eq!(e1.pm10_0_as_nanograms_per_cubic_meter(), Some(11_600));
        assert_eq!(e1.carbon_dioxide_as_ppm(), Some(886));
        assert_eq!(e1.voc_index(), Some(160));
        assert_eq!(e1.nox_index(), Some(10));
        assert_eq!(e1.luminosity_as_millilux(), Some(79_400));
        assert_eq!(e1.measurement_sequence_number(), Some(0x23_4567));
        assert_eq!(e1.mac_address(), Some(MAC));

        let f6 = decode(&legacy);
        assert_eq!(f6.temperature_as_millicelsius(), Some(22_350));
        assert_eq!(f6.pm2_5_as_nanograms_per_cubic_meter(), Some(11_400));
        assert_eq!(f6.measurement_sequence_number(), Some(0x67));
        assert_eq!(f6.mac_address(), Some([0, 0, 0, 0x4C, 0x88, 0x4F]));
        // The log scale's nearest step to 79.4 lx.
        let lux = f6.luminosity_as_millilux().unwrap();
        assert!((74_000..=85_000).contains(&lux), "{lux}");
    }

    #[test]
    fn temperature_wraps_each_minute() {
        assert!((Reading::simulated(59).temperature - 23.95).abs() < 1e-9);
        assert!((Reading::simulated(60).temperature - 21.0).abs() < 1e-9);
    }

    #[test]
    fn name_uses_the_last_mac_bytes() {
        assert_eq!(name(&MAC), "Ruuvi 884F");
    }

    #[test]
    fn parses_sets() {
        assert_eq!(Sets::parse("legacy,extended,coded"), Ok(Sets::ALL));
        assert_eq!(
            Sets::parse(" extended "),
            Ok(Sets {
                legacy: false,
                extended: true,
                coded: false
            })
        );
        assert!(Sets::parse("").is_err());
        assert!(Sets::parse("legacy,2m").is_err());
    }

    #[test]
    fn parses_macs() {
        assert_eq!(parse_mac("CB:B8:33:4C:88:4F"), Ok(MAC));
        assert_eq!(format_mac(&MAC), "CB:B8:33:4C:88:4F");
        assert!(parse_mac("CB:B8:33:4C:88").is_err());
        assert!(parse_mac("CB:B8:33:4C:88:4F:00").is_err());
        assert!(parse_mac("CB:B8:33:4C:88:XX").is_err());
        assert!(parse_mac("0B:B8:33:4C:88:4F").is_err());
    }

    #[test]
    fn static_random_sets_the_top_bits() {
        assert_eq!(static_random([0x0B, 1, 2, 3, 4, 5]), [0xCB, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn pulse_breathes_then_pauses() {
        assert_eq!(pulse_level(0), 0);
        assert_eq!(pulse_level(PULSE_MS / 2), 255);
        assert_eq!(pulse_level(PULSE_MS), 0);
        assert_eq!(pulse_level(PULSE_PERIOD_MS - 1), 0);
        assert_eq!(pulse_level(PULSE_PERIOD_MS + PULSE_MS / 2), 255);
        // Rises steadily to the peak
        let rise: Vec<u8> = (0..=PULSE_MS / 2).step_by(100).map(pulse_level).collect();
        assert!(rise.windows(2).all(|w| w[0] <= w[1]), "{rise:?}");
    }

    #[test]
    fn scales_the_color() {
        assert_eq!(ws2812_grb(0x10_20_30, 255), [0x20, 0x10, 0x30]);
        assert_eq!(ws2812_grb(0x10_20_30, 0), [0, 0, 0]);
        assert_eq!(ws2812_grb(0xFF_00_80, 128), [0, 128, 64]);
    }

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("000020"), Ok(0x20));
        assert_eq!(parse_color("FF8000"), Ok(0xFF_80_00));
        assert!(parse_color("00002").is_err());
        assert!(parse_color("00002G").is_err());
    }
}
