//! The dataset `formula` language: lexer → parser → evaluator over `serde_json::Value`.
//!
//! Pure — no I/O and no clock of its own. `today()` answers with the date the caller injects
//! through [`EvalCtx`], which is what keeps a formula's result a function of the row alone.
//! The three budgets (nodes, steps, depth) are what make an untrusted expression cheap to
//! refuse: this module must never be the reason a query stalls or a stack overflows.
//! An error is a value the caller renders as a `#ERR(reason)` cell, never a panic and never
//! markup.

pub mod deps;
mod eval;
#[cfg(test)]
mod hostile_input;
mod lex;
mod parse;

pub use eval::eval;
pub use parse::{field_refs, parse};

use serde_json::Value;
use thiserror::Error;

use crate::dataset::FieldKey;

/// Largest accepted syntax tree, counted in AST nodes.
pub const MAX_NODES: usize = 256;
/// Largest number of node visits a single evaluation may cost.
pub const MAX_STEPS: usize = 10_000;
/// Deepest nesting — parentheses, call arguments, `not` and unary minus all count.
pub const MAX_DEPTH: usize = 32;
/// Longest rendered error cell, brackets included. A field key is at most 48 characters, so a
/// real key always fits; anything longer is cut.
const MAX_CELL: usize = 64;

/// A parsed formula. Opaque: build it with [`parse`], read its dependencies with
/// [`field_refs`], run it with [`eval`].
#[derive(Debug, Clone)]
pub struct Expr(pub(crate) parse::Node);

/// What a formula may look at while it runs: one row's values, and the clock the caller
/// injects so that `today()` is a fact of the evaluation rather than of the machine.
pub struct EvalCtx<'a> {
    /// The row's value for a field key, or `None` when the row carries no such value.
    /// A missing value reads as `null`; arithmetic on it is an error, `coalesce` skips it.
    pub field: &'a dyn Fn(&str) -> Option<Value>,
    /// Injected clock, `YYYY-MM-DD`. `today()` returns exactly this.
    pub today: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormulaError {
    #[error("lex error at character {1}: {0}")]
    Lex(&'static str, usize),
    #[error("parse error at character {1}: {0}")]
    Parse(&'static str, usize),
    #[error("unknown field `{0}`")]
    UnknownField(String),
    #[error("formula uses more than {MAX_NODES} nodes")]
    TooManyNodes,
    #[error("formula nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("formula needs more than {MAX_STEPS} steps")]
    Budget,
    #[error("division by zero")]
    DivZero,
    #[error("{0}")]
    Type(&'static str),
    #[error("result is not a finite number")]
    NotFinite,
}

impl FormulaError {
    /// The cell a reader sees: `#ERR(reason)`, ASCII, bounded, and rendered as plain text.
    /// Nothing from the formula is echoed back except a field key that is itself validated.
    pub fn cell_text(&self) -> String {
        let reason = match self {
            FormulaError::Lex(msg, pos) => format!("lex at {pos}: {msg}"),
            FormulaError::Parse(msg, pos) => format!("parse at {pos}: {msg}"),
            FormulaError::UnknownField(key) => match FieldKey::parse(key) {
                Ok(_) => format!("unknown field {key}"),
                // The key failed the field-key rules, so it is not echoed — it is not a key.
                Err(_) => "unknown field".to_string(),
            },
            FormulaError::TooManyNodes => "too many nodes".to_string(),
            FormulaError::TooDeep => "too deeply nested".to_string(),
            FormulaError::Budget => "too many steps".to_string(),
            FormulaError::DivZero => "divide by zero".to_string(),
            FormulaError::Type(msg) => (*msg).to_string(),
            FormulaError::NotFinite => "not a finite number".to_string(),
        };
        let mut cell = String::from("#ERR(");
        for c in reason.chars() {
            if cell.chars().count() >= MAX_CELL - 1 {
                break;
            }
            if c.is_ascii_graphic() || c == ' ' {
                cell.push(c);
            }
        }
        cell.push(')');
        cell
    }
}
