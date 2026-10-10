//! Datasets: typed field keys and kinds. Pure, no I/O.

pub mod key;
pub mod kind;

pub use key::{FieldKey, KeyError, MAX_KEY_LEN, RESERVED_KEYS};
pub use kind::FieldKind;
