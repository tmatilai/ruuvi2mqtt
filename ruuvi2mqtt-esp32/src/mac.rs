use std::fmt;

use serde::{Serialize, Serializer};

/// A 6-byte MAC address with colon-separated `Display` and bare-hex topic format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mac(pub [u8; 6]);

impl Mac {
    /// Bare lowercase hex string for use in MQTT topics (e.g. `"aabbccddeeff"`).
    #[must_use]
    pub fn to_topic_string(self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5],
        )
    }

    /// Parses the topic format, in either case.
    #[must_use]
    pub fn from_topic_str(s: &str) -> Option<Self> {
        if s.len() != 12 {
            return None;
        }
        let mut bytes = [0; 6];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

/// Colon-separated lowercase hex (e.g. `"aa:bb:cc:dd:ee:ff"`).
impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5],
        )
    }
}

/// The topic format, so that a reading and its statistics share the ID.
impl Serialize for Mac {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_topic_string())
    }
}

impl From<[u8; 6]> for Mac {
    fn from(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::Mac;

    const EXAMPLE: Mac = Mac([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

    #[test]
    fn display_is_colon_separated_lowercase() {
        assert_eq!(EXAMPLE.to_string(), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn topic_string_is_bare_lowercase_hex() {
        assert_eq!(EXAMPLE.to_topic_string(), "aabbccddeeff");
    }

    #[test]
    fn parses_the_topic_format() {
        assert_eq!(Mac::from_topic_str("aabbccddeeff"), Some(EXAMPLE));
        assert_eq!(Mac::from_topic_str("AABBCCDDEEFF"), Some(EXAMPLE));
        assert_eq!(Mac::from_topic_str("aabbccddee"), None);
        assert_eq!(Mac::from_topic_str("aabbccddeefg"), None);
        assert_eq!(Mac::from_topic_str("aa:bb:cc:dd:ee:ff"), None);
        assert_eq!(Mac::from_topic_str("aabbccddeeé"), None);
    }

    #[test]
    fn serializes_in_topic_format() {
        assert_eq!(
            serde_json::to_string(&EXAMPLE).unwrap(),
            r#""aabbccddeeff""#
        );
    }

    #[test]
    fn from_byte_array() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let mac = Mac::from(bytes);
        assert_eq!(mac.0, bytes);
    }

    #[test]
    fn equality() {
        let a = Mac([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let b = Mac([0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        let c = Mac([0xFF, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn all_zeros() {
        let mac = Mac([0x00; 6]);
        assert_eq!(mac.to_string(), "00:00:00:00:00:00");
        assert_eq!(mac.to_topic_string(), "000000000000");
    }
}
