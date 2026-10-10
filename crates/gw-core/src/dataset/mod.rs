//! Datasets: typed field keys and kinds. Pure, no I/O.

pub mod key;
pub mod kind;
pub mod value;

pub use key::{FieldKey, KeyError, MAX_KEY_LEN, RESERVED_KEYS};
pub use kind::FieldKind;
pub use value::{validate_row, validate_value, FieldError, FieldSpec, RowError, MAX_ROW_BYTES};
