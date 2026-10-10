//! Typed value validation. Pure; the store calls it on every write.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::{FieldKey, FieldKind};

/// Largest accepted row, as serialized JSON bytes.
pub const MAX_ROW_BYTES: usize = 64 * 1024;
/// Longest tag id accepted.
const MAX_ID_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FieldError {
    #[error("expected {0}")]
    WrongType(&'static str),
    #[error("number must be finite")]
    NotFinite,
    #[error("date must be YYYY-MM-DD{}", if *.0 { " or RFC 3339" } else { "" })]
    BadDate(bool),
    #[error("url must be http, https or mailto")]
    BadUrl,
    #[error("`{0}` is not an option of this field")]
    UnknownOption(String),
    #[error("id is empty or too long")]
    BadId,
    /// person, file and relation need the store to check what they point at.
    #[error("{0} values need the store to validate")]
    NeedsStore(&'static str),
    #[error("{0} fields are computed and never stored")]
    Computed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RowError {
    #[error("unknown field `{0}`")]
    UnknownKey(String),
    #[error("field `{key}`: {error}")]
    Field { key: String, error: FieldError },
    #[error("row is larger than {MAX_ROW_BYTES} bytes")]
    TooLarge,
}

/// One field's schema, as `validate_row` needs it.
#[derive(Debug, Clone)]
pub struct FieldSpec<'a> {
    pub key: &'a FieldKey,
    pub kind: FieldKind,
    pub config: &'a Value,
}

/// Validate one value; returns the normalised form to store. `null` clears a cell.
pub fn validate_value(kind: FieldKind, config: &Value, json: &Value) -> Result<Value, FieldError> {
    if kind.is_computed() {
        return Err(FieldError::Computed(kind.as_str()));
    }
    if json.is_null() {
        return Ok(Value::Null);
    }
    match kind {
        FieldKind::Text => Ok(Value::String(string(json)?.to_string())),
        FieldKind::Number => match json {
            Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => Ok(json.clone()),
            Value::Number(_) => Err(FieldError::NotFinite),
            _ => Err(FieldError::WrongType("a number")),
        },
        FieldKind::Bool => match json {
            Value::Bool(_) => Ok(json.clone()),
            _ => Err(FieldError::WrongType("true or false")),
        },
        FieldKind::Date => {
            let with_time = config
                .get("with_time")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let s = string(json)?;
            if valid_date(s) || (with_time && valid_rfc3339(s)) {
                Ok(json.clone())
            } else {
                Err(FieldError::BadDate(with_time))
            }
        }
        FieldKind::Select => {
            let id = string(json)?;
            check_option(config, id)?;
            Ok(json.clone())
        }
        FieldKind::MultiSelect => {
            let ids = dedupe(strings(json)?);
            for id in &ids {
                check_option(config, id)?;
            }
            Ok(ids.into())
        }
        FieldKind::Tags => {
            let ids = dedupe(strings(json)?);
            if ids.iter().any(|t| t.is_empty() || t.len() > MAX_ID_LEN) {
                return Err(FieldError::BadId);
            }
            Ok(ids.into())
        }
        FieldKind::Url => {
            let s = string(json)?;
            let lower = s.to_ascii_lowercase();
            let ok = ["http://", "https://", "mailto:"]
                .iter()
                .any(|p| lower.starts_with(p) && s.len() > p.len());
            if ok && !s.chars().any(|c| c.is_control() || c.is_whitespace()) {
                Ok(json.clone())
            } else {
                Err(FieldError::BadUrl)
            }
        }
        FieldKind::Person | FieldKind::File | FieldKind::Relation => {
            Err(FieldError::NeedsStore(kind.as_str()))
        }
        FieldKind::Rollup | FieldKind::Formula => unreachable!("computed kinds returned above"),
    }
}

/// Validate a whole row against its schema: unknown keys refused, size capped.
pub fn validate_row(
    fields: &[FieldSpec<'_>],
    row: &Map<String, Value>,
) -> Result<Map<String, Value>, RowError> {
    let mut out = Map::new();
    for (key, v) in row {
        let spec = fields
            .iter()
            .find(|f| f.key.as_str() == key)
            .ok_or_else(|| RowError::UnknownKey(key.clone()))?;
        let v = validate_value(spec.kind, spec.config, v).map_err(|error| RowError::Field {
            key: key.clone(),
            error,
        })?;
        out.insert(key.clone(), v);
    }
    let size = serde_json::to_vec(&out).map_or(usize::MAX, |b| b.len());
    if size > MAX_ROW_BYTES {
        return Err(RowError::TooLarge);
    }
    Ok(out)
}

fn string(v: &Value) -> Result<&str, FieldError> {
    v.as_str().ok_or(FieldError::WrongType("a string"))
}

fn strings(v: &Value) -> Result<Vec<String>, FieldError> {
    v.as_array()
        .ok_or(FieldError::WrongType("a list of strings"))?
        .iter()
        .map(|x| string(x).map(str::to_string))
        .collect()
}

fn dedupe(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|i| seen.insert(i.clone())).collect()
}

fn check_option(config: &Value, id: &str) -> Result<(), FieldError> {
    let known = config
        .get("options")
        .and_then(Value::as_array)
        .is_some_and(|o| {
            o.iter()
                .any(|x| x.get("id").and_then(Value::as_str) == Some(id))
        });
    if known {
        Ok(())
    } else {
        Err(FieldError::UnknownOption(id.to_string()))
    }
}

/// Exactly `n` ASCII digits, parsed.
fn digits(s: &str, n: usize) -> Option<u32> {
    if s.len() == n && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

fn valid_date(s: &str) -> bool {
    if s.len() != 10 || !s.is_ascii() || &s[4..5] != "-" || &s[7..8] != "-" {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (
        digits(&s[0..4], 4),
        digits(&s[5..7], 2),
        digits(&s[8..10], 2),
    ) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max).contains(&d)
}

fn valid_rfc3339(s: &str) -> bool {
    if !s.is_ascii() {
        return false;
    }
    let Some((date, rest)) = s.split_once(['T', 't']) else {
        return false;
    };
    if !valid_date(date) {
        return false;
    }
    let Some(i) = rest.find(['Z', 'z', '+', '-']) else {
        return false;
    };
    let (time, off) = rest.split_at(i);
    let (hms, frac) = match time.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (time, None),
    };
    if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let p: Vec<&str> = hms.split(':').collect();
    let ok_time = p.len() == 3
        && digits(p[0], 2).is_some_and(|h| h < 24)
        && digits(p[1], 2).is_some_and(|m| m < 60)
        && digits(p[2], 2).is_some_and(|s| s < 61);
    let ok_off = matches!(off, "Z" | "z")
        || (off.len() == 6
            && matches!(&off[..1], "+" | "-")
            && &off[3..4] == ":"
            && digits(&off[1..3], 2).is_some_and(|h| h < 24)
            && digits(&off[4..6], 2).is_some_and(|m| m < 60));
    ok_time && ok_off
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(kind: FieldKind, config: Value, j: Value) -> Result<Value, FieldError> {
        validate_value(kind, &config, &j)
    }

    #[test]
    fn text_and_bool() {
        assert_eq!(v(FieldKind::Text, json!({}), json!("hi")), Ok(json!("hi")));
        assert!(v(FieldKind::Text, json!({}), json!(1)).is_err());
        assert!(v(FieldKind::Bool, json!({}), json!(true)).is_ok());
        assert!(v(FieldKind::Bool, json!({}), json!("true")).is_err());
        assert_eq!(v(FieldKind::Text, json!({}), Value::Null), Ok(Value::Null));
    }

    #[test]
    fn number_rejects_nan_inf_and_strings() {
        assert!(v(FieldKind::Number, json!({}), json!(1.5)).is_ok());
        assert!(v(FieldKind::Number, json!({}), json!("1")).is_err());
        for s in ["NaN", "Infinity", "-inf"] {
            assert!(v(FieldKind::Number, json!({}), json!(s)).is_err());
        }
        // serde_json cannot hold NaN/Inf as a Number: it becomes null.
        assert_eq!(Value::from(f64::NAN), Value::Null);
    }

    #[test]
    fn url_scheme_allow_list() {
        for ok in [
            "http://a.b",
            "https://a.b/x?y=1",
            "mailto:a@b.c",
            "HTTPS://A.B",
        ] {
            assert!(v(FieldKind::Url, json!({}), json!(ok)).is_ok(), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "data:text/html,x",
            "JaVaScRiPt:x",
            " http://a.b",
            "http://",
            "ftp://a",
            "http://a b",
            "",
            "//a.b",
        ] {
            assert_eq!(
                v(FieldKind::Url, json!({}), json!(bad)),
                Err(FieldError::BadUrl),
                "{bad}"
            );
        }
    }

    #[test]
    fn date_formats() {
        let d = json!({});
        let t = json!({"with_time": true});
        assert!(v(FieldKind::Date, d.clone(), json!("2024-02-29")).is_ok());
        for bad in [
            "2023-02-29",
            "2024-13-01",
            "2024-1-01",
            "2024-01-32",
            "x",
            "20240101",
        ] {
            assert!(v(FieldKind::Date, d.clone(), json!(bad)).is_err(), "{bad}");
        }
        assert!(v(FieldKind::Date, d, json!("2024-01-01T10:00:00Z")).is_err());
        for ok in [
            "2024-01-01T10:00:00Z",
            "2024-01-01T10:00:00.123+02:00",
            "2024-01-01",
        ] {
            assert!(v(FieldKind::Date, t.clone(), json!(ok)).is_ok(), "{ok}");
        }
        for bad in [
            "2024-01-01T25:00:00Z",
            "2024-01-01T10:00:00",
            "2024-01-01T10:00Z",
            "2024-01-01Té:00:00Z",
        ] {
            assert!(v(FieldKind::Date, t.clone(), json!(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn select_and_multi_select() {
        let cfg = json!({"options": [{"id": "a"}, {"id": "b"}]});
        assert!(v(FieldKind::Select, cfg.clone(), json!("a")).is_ok());
        assert_eq!(
            v(FieldKind::Select, cfg.clone(), json!("z")),
            Err(FieldError::UnknownOption("z".into()))
        );
        assert!(v(FieldKind::Select, json!({}), json!("a")).is_err());
        assert_eq!(
            v(FieldKind::MultiSelect, cfg.clone(), json!(["b", "a", "b"])),
            Ok(json!(["b", "a"]))
        );
        assert!(v(FieldKind::MultiSelect, cfg.clone(), json!(["a", "z"])).is_err());
        assert!(v(FieldKind::MultiSelect, cfg, json!("a")).is_err());
    }

    #[test]
    fn tags_dedupe_and_bound() {
        assert_eq!(
            v(FieldKind::Tags, json!({}), json!(["x", "x", "y"])),
            Ok(json!(["x", "y"]))
        );
        assert_eq!(
            v(FieldKind::Tags, json!({}), json!([""])),
            Err(FieldError::BadId)
        );
        assert!(v(FieldKind::Tags, json!({}), json!(["a".repeat(129)])).is_err());
    }

    #[test]
    fn store_kinds_are_stubs_and_computed_refused() {
        for k in [FieldKind::Person, FieldKind::File, FieldKind::Relation] {
            assert_eq!(
                v(k, json!({}), json!("x")),
                Err(FieldError::NeedsStore(k.as_str()))
            );
        }
        for k in [FieldKind::Rollup, FieldKind::Formula] {
            assert_eq!(
                v(k, json!({}), json!(1)),
                Err(FieldError::Computed(k.as_str()))
            );
        }
    }

    #[test]
    fn row_validation() {
        let (a, b) = (FieldKey::parse("a").unwrap(), FieldKey::parse("b").unwrap());
        let cfg = json!({});
        let fields = [
            FieldSpec {
                key: &a,
                kind: FieldKind::Number,
                config: &cfg,
            },
            FieldSpec {
                key: &b,
                kind: FieldKind::Text,
                config: &cfg,
            },
        ];
        let row = |j: Value| j.as_object().unwrap().clone();
        assert!(validate_row(&fields, &row(json!({"a": 1, "b": "x"}))).is_ok());
        assert_eq!(
            validate_row(&fields, &row(json!({"zz": 1}))),
            Err(RowError::UnknownKey("zz".into()))
        );
        assert!(matches!(
            validate_row(&fields, &row(json!({"a": "no"}))),
            Err(RowError::Field { .. })
        ));
        let big = "x".repeat(MAX_ROW_BYTES);
        assert_eq!(
            validate_row(&fields, &row(json!({"b": big}))),
            Err(RowError::TooLarge)
        );
        let just = "x".repeat(MAX_ROW_BYTES - 8); // {"b":"…"} adds 8 bytes
        assert!(validate_row(&fields, &row(json!({"b": just}))).is_ok());
    }
}
