//! Malformed formula input: nothing here is a valid formula, and the point is that the
//! language never panics on it and never answers with anything but a cell.
//!
//! The generator is a seeded xorshift64 written inline rather than a `rand` dependency: a
//! formula module must not be the reason the crate grows a dependency, and a fixed seed makes
//! the thousand strings the same thousand strings on every machine.

use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::formula::{eval, parse, EvalCtx, Expr, FormulaError};

/// The dataset this pretend formula language is asked about.
fn p(src: &str) -> Result<Expr, FormulaError> {
    parse(src, &|k| {
        matches!(k, "a" | "b" | "name" | "due" | "count" | "tags")
    })
}

fn row_field(k: &str) -> Option<Value> {
    match k {
        "a" => Some(Value::from(1)),
        "b" => Some(Value::from(2)),
        "name" => Some(Value::String("Ann".into())),
        _ => None,
    }
}

fn ctx() -> EvalCtx<'static> {
    EvalCtx {
        field: &row_field,
        today: "2026-10-10",
    }
}

struct Xor64(u64);

impl Xor64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const ALPHABET: &str = "0123456789.+-*/%()<>=!{}_ abcdefghijklmnopqrstuvwxyz\",\\";
const FRAGMENTS: [&str; 23] = [
    "1.5",
    "\"a\\\"b\"",
    "{a}",
    "{name",
    "()(",
    ")",
    "+",
    "-",
    "*",
    "/",
    "%",
    "==",
    "!=",
    "<=",
    ">=",
    "<",
    ">",
    " and ",
    " or ",
    "not ",
    "if(",
    "today()",
    "coalesce(",
];

fn garbage(rng: &mut Xor64, alphabet: &str, max_len: usize) -> String {
    let chars: Vec<char> = alphabet.chars().collect();
    let len = rng.below(max_len as u64 + 1) as usize;
    let mut s = String::new();
    for _ in 0..len {
        s.push(chars[rng.below(chars.len() as u64) as usize]);
    }
    s
}

/// One round of hostile input: parse it, and if the parser took it, evaluate it. Any panic is
/// a failure, and any error must be a cell.
fn feed(src: &str, accepted: &mut usize, values: &mut usize) {
    let run = || -> Option<Result<Value, FormulaError>> {
        let e = p(src).ok()?;
        *accepted += 1;
        Some(eval(&e, &ctx()))
    };
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(Some(Ok(v))) => {
            *values += 1;
            // A result that cannot be serialised cannot be stored, so it must never come back.
            serde_json::to_string(&v)
                .unwrap_or_else(|e| panic!("{src:?} gave an unstorable {v}: {e}"));
        }
        Ok(Some(Err(err))) => assert_cell(src, &err.cell_text()),
        Ok(None) => {}
        Err(_) => panic!("panicked on {src:?}"),
    }
}

#[test]
fn malformed_input_never_panics() {
    let mut rng = Xor64(0x9E37_79B9_7F4A_7C15);
    let (mut accepted, mut values) = (0usize, 0usize);
    for _ in 0..1000 {
        let src = garbage(&mut rng, ALPHABET, 60);
        feed(&src, &mut accepted, &mut values);
    }
    for _ in 0..200 {
        let mut src = String::new();
        for _ in 0..rng.below(12) + 1 {
            let i = rng.below(FRAGMENTS.len() as u64) as usize;
            src.push_str(FRAGMENTS[i]);
        }
        feed(&src, &mut accepted, &mut values);
    }
    assert!(
        accepted >= 5,
        "barely any of 1200 was a formula — the generator is dead"
    );
    assert!(values > 0, "nothing evaluated — the generator is broken");
}

fn assert_cell(src: &str, text: &str) {
    assert!(text.starts_with("#ERR("), "{src:?} -> {text}");
    assert!(text.is_ascii(), "{src:?} -> {text}");
}
