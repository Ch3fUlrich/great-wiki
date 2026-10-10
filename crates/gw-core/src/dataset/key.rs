//! Field keys: the only form in which a field name reaches SQL paths or JSON row keys.

use serde::{Deserialize, Serialize};

/// Longest accepted key, in bytes (keys are ASCII).
pub const MAX_KEY_LEN: usize = 48;
/// Names a row already carries itself.
pub const RESERVED_KEYS: [&str; 4] = ["id", "created_at", "updated_at", "version"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("field key is empty")]
    Empty,
    #[error("field key is longer than {MAX_KEY_LEN} characters")]
    TooLong,
    #[error("field key must start with a lowercase letter")]
    BadStart,
    #[error("field key may contain only a-z, 0-9 and _")]
    BadChar,
    #[error("field key `{0}` is reserved")]
    Reserved(String),
}

/// A validated field key: `^[a-z][a-z0-9_]*$`, at most 48 chars, not reserved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct FieldKey(String);

impl FieldKey {
    pub fn parse(s: &str) -> Result<Self, KeyError> {
        if s.is_empty() {
            return Err(KeyError::Empty);
        }
        if s.len() > MAX_KEY_LEN {
            return Err(KeyError::TooLong);
        }
        if !s.as_bytes()[0].is_ascii_lowercase() {
            return Err(KeyError::BadStart);
        }
        if !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(KeyError::BadChar);
        }
        if RESERVED_KEYS.contains(&s) {
            return Err(KeyError::Reserved(s.to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for FieldKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_keys() {
        for k in ["a", "a_1", "due_date", &"a".repeat(48)] {
            assert_eq!(FieldKey::parse(k).unwrap().as_str(), k);
        }
    }

    #[test]
    fn refuses_bad_keys() {
        for k in [
            "", "1a", "A", "a-b", "a b", "a\"", "a'; --", "_a", "é", "aé",
        ] {
            assert!(FieldKey::parse(k).is_err(), "{k:?}");
        }
        assert_eq!(FieldKey::parse(&"a".repeat(49)), Err(KeyError::TooLong));
    }

    #[test]
    fn refuses_reserved() {
        for k in RESERVED_KEYS {
            assert_eq!(FieldKey::parse(k), Err(KeyError::Reserved(k.into())));
        }
    }

    #[test]
    fn any_char_outside_set_is_refused_at_every_position() {
        for c in (0u8..=127).map(char::from) {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                continue;
            }
            for key in [
                format!("{c}"),
                format!("{c}ab"),
                format!("a{c}b"),
                format!("ab{c}"),
            ] {
                assert!(FieldKey::parse(&key).is_err(), "{key:?}");
            }
        }
    }

    #[test]
    fn deserialize_validates() {
        assert!(serde_json::from_str::<FieldKey>("\"ok_1\"").is_ok());
        assert!(serde_json::from_str::<FieldKey>("\"A\"").is_err());
        assert_eq!(
            serde_json::to_string(&FieldKey::parse("x").unwrap()).unwrap(),
            "\"x\""
        );
    }
}
