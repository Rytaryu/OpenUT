use std::fmt;

use serde::{Deserialize, Deserializer, Serializer, de};

pub(super) mod word {
    use super::*;

    pub fn serialize<S: Serializer>(value: &u16, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{value:04x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() != 4 || !text.bytes().all(|character| character.is_ascii_hexdigit()) {
            return Err(de::Error::custom("expected four hexadecimal digits"));
        }
        u16::from_str_radix(&text, 16).map_err(|_| de::Error::custom("invalid hexadecimal word"))
    }
}

pub(super) mod bytes {
    use super::*;

    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&HexBytes(value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let (pairs, rest) = text.as_bytes().as_chunks::<2>();
        if !rest.is_empty() {
            return Err(de::Error::custom(
                "hexadecimal reports require complete bytes",
            ));
        }
        pairs
            .iter()
            .map(|&[high, low]| {
                let high = char::from(high).to_digit(16)?;
                let low = char::from(low).to_digit(16)?;
                Some((high as u8) << 4 | low as u8)
            })
            .collect::<Option<_>>()
            .ok_or_else(|| de::Error::custom("invalid hexadecimal report"))
    }

    struct HexBytes<'a>(&'a [u8]);

    impl fmt::Display for HexBytes<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            for byte in self.0 {
                write!(f, "{byte:02x}")?;
            }
            Ok(())
        }
    }
}
