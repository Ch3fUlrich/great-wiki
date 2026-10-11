//! Computed fields are evaluated when a row is read and are never stored: a formula's value is
//! a function of the row's stored cells, so listing a row a hundred times answers the same way
//! every time, and the stored JSON keeps only what somebody typed.
//!
//! A formula may read another formula, so the fields are evaluated in dependency order and each
//! result is fed to those that read it. The row arrives here already holding its rollups — those
//! are answered by `rollup.rs`, before formulas run, because a formula may read a rollup while a
//! rollup never reads a formula. Anything that goes wrong becomes one error cell, never a panic
//! and never a lost row.

use super::{DatasetField, DatasetRow};
use gw_core::dataset::FieldKind;
use gw_core::formula::{self, EvalCtx, Expr};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};

/// The cell an error leaves behind: `{"error": "#ERR(reason)"}`, rendered as plain text. The
/// `rollup` kind answers with the same shape when it has no number to give.
pub(super) fn error_cell(error: &formula::FormulaError) -> Value {
    json!({ "error": error.cell_text() })
}

/// Evaluate one expression against the row's values as they stand: every field the formula
/// reads is looked up in `values`, which already holds the stored cells and the formulas that
/// have been computed so far. A key that is not there reads as `null`.
fn compute(expr: &Expr, values: &Map<String, Value>, today: &str) -> Value {
    let field = |key: &str| values.get(key).cloned();
    let ctx = EvalCtx {
        field: &field,
        today,
    };
    match formula::eval(expr, &ctx) {
        Ok(value) => value,
        Err(error) => error_cell(&error),
    }
}

/// The row as a reader sees it: its stored values plus one entry per formula field, evaluated in
/// the order the dependencies ask for. `today` is the caller's clock, fetched once per call.
/// `hidden` is the caller's hidden-field set: a formula in it is a column this caller never sees,
/// so it is not evaluated and leaves no value behind — a visible formula is never computed from a
/// hidden field, because reaching one would have made it hidden too.
pub(super) fn with_computed(
    fields: &[DatasetField],
    mut row: DatasetRow,
    today: &str,
    hidden: &HashSet<String>,
) -> DatasetRow {
    let formulas: Vec<&DatasetField> = fields
        .iter()
        .filter(|f| f.kind == FieldKind::Formula.as_str() && !hidden.contains(f.key.as_str()))
        .collect();
    if formulas.is_empty() {
        return row;
    }
    let keys: HashSet<&str> = formulas.iter().map(|f| f.key.as_str()).collect();

    let stored: Map<String, Value> = row.values.as_object().cloned().unwrap_or_default();
    let mut values = stored;

    // Parse each expression once. One that no longer parses is an error cell right away and is
    // left out of the ordering; a cycle would be caught at schema write, so none is expected.
    let mut nodes: Vec<(String, Expr, Vec<String>)> = Vec::new();
    for f in &formulas {
        match f.config.get("expr").and_then(Value::as_str) {
            Some(src) => match formula::parse(src, &|_| true) {
                Ok(expr) => {
                    let deps = formula::field_refs(&expr)
                        .into_iter()
                        .filter(|d| keys.contains(d.as_str()) && d.as_str() != f.key.as_str())
                        .collect();
                    nodes.push((f.key.clone(), expr, deps));
                }
                Err(error) => {
                    values.insert(f.key.clone(), error_cell(&error));
                }
            },
            None => {
                values.insert(f.key.clone(), json!({ "error": "#ERR(no expression)" }));
            }
        }
    }

    // Kahn's algorithm over the formula subgraph: a field is evaluated only once the formulas it
    // reads have been.
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (key, _, _))| (key.as_str(), i))
        .collect();
    let mut indegree = vec![0usize; nodes.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, (_, _, deps)) in nodes.iter().enumerate() {
        for dep in deps {
            if let Some(&j) = index.get(dep.as_str()) {
                indegree[i] += 1;
                dependents[j].push(i);
            }
        }
    }

    let mut ready: VecDeque<usize> = (0..nodes.len()).filter(|&i| indegree[i] == 0).collect();
    let mut done = vec![false; nodes.len()];
    while let Some(i) = ready.pop_front() {
        done[i] = true;
        let (key, expr, _) = &nodes[i];
        let value = compute(expr, &values, today);
        values.insert(key.clone(), value);
        for &j in &dependents[i] {
            if indegree[j] > 0 {
                indegree[j] -= 1;
                if indegree[j] == 0 {
                    ready.push_back(j);
                }
            }
        }
    }
    // Leftover nodes are a cycle seeded straight into the table; read what is available rather
    // than never finishing.
    for i in 0..nodes.len() {
        if !done[i] {
            let (key, expr, _) = &nodes[i];
            let value = compute(expr, &values, today);
            values.insert(key.clone(), value);
        }
    }

    row.values = Value::Object(values);
    row
}
