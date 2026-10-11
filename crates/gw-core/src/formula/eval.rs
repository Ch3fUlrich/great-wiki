//! Evaluator: walks a parsed expression over one row's values. A missing field reads as
//! `null`; arithmetic on it is a type error, `coalesce` is what skips it. Every node visit
//! costs one step and the budget is spent per row, so a formula cannot stall a query.

use serde_json::{Number, Value};
use std::cmp::Ordering;

use super::parse::{Func, Node, Op};
use super::{EvalCtx, Expr, FormulaError, MAX_STEPS};

pub(crate) const T_NUM: &str = "number expected";
pub(crate) const T_BOOL: &str = "boolean expected";
pub(crate) const T_ORDER: &str = "comparable types";
pub(crate) const T_LEN: &str = "string or array";
pub(crate) const T_DIGITS: &str = "digits 0..9";
pub(crate) const T_JOIN: &str = "concatenable value";
/// Parse settles arity, so these shapes are certain for a parsed expression. The fallbacks
/// exist so a hand-built node — an `Expr` from a future refactor, not from `parse` — can never
/// panic the server.
const T_ARITY: &str = "call arity";

pub fn eval(e: &Expr, ctx: &EvalCtx) -> Result<Value, FormulaError> {
    eval_with_budget(e, ctx, MAX_STEPS)
}

/// The step budget as an argument, so a test can prove the counter works without constructing
/// an expression that costs ten thousand visits. `eval` is the production entry point.
pub(crate) fn eval_with_budget(
    e: &Expr,
    ctx: &EvalCtx,
    steps: usize,
) -> Result<Value, FormulaError> {
    let mut w = Walk {
        field: ctx.field,
        today: ctx.today,
        steps: 0,
        budget: steps,
    };
    w.node(&e.0)
}

struct Walk<'a> {
    field: &'a dyn Fn(&str) -> Option<Value>,
    today: &'a str,
    steps: usize,
    budget: usize,
}

impl<'a> Walk<'a> {
    fn node(&mut self, n: &Node) -> Result<Value, FormulaError> {
        self.steps += 1;
        if self.steps > self.budget {
            return Err(FormulaError::Budget);
        }
        match n {
            Node::Num(v) => number(*v),
            Node::Str(s) => Ok(Value::String(s.clone())),
            Node::Bool(b) => Ok(Value::Bool(*b)),
            Node::Null => Ok(Value::Null),
            Node::Field(key) => Ok((self.field)(key).unwrap_or(Value::Null)),
            Node::Neg(inner) => number(-self.number(inner)?),
            Node::Not(inner) => Ok(Value::Bool(!self.boolean(inner)?)),
            Node::Bin(op, lhs, rhs) => self.binary(*op, lhs, rhs),
            Node::Call(func, args) => self.call(*func, args),
        }
    }

    fn number(&mut self, n: &Node) -> Result<f64, FormulaError> {
        as_number(&self.node(n)?)
    }

    fn boolean(&mut self, n: &Node) -> Result<bool, FormulaError> {
        match self.node(n)? {
            Value::Bool(b) => Ok(b),
            _ => Err(FormulaError::Type(T_BOOL)),
        }
    }

    fn binary(&mut self, op: Op, lhs: &Node, rhs: &Node) -> Result<Value, FormulaError> {
        use Op::*;
        match op {
            // Short-circuit: the right operand is looked at only when it can still change the
            // answer, which is what makes `false and 1/0` and `true or 1/0` safe.
            And => Ok(Value::Bool(self.boolean(lhs)? && self.boolean(rhs)?)),
            Or => Ok(Value::Bool(self.boolean(lhs)? || self.boolean(rhs)?)),
            Eq | Ne => {
                let (l, r) = (self.node(lhs)?, self.node(rhs)?);
                let same = equal(&l, &r)?;
                Ok(Value::Bool(if matches!(op, Ne) { !same } else { same }))
            }
            Lt | Le | Gt | Ge => {
                let (l, r) = (self.node(lhs)?, self.node(rhs)?);
                let ord = order(&l, &r)?;
                Ok(Value::Bool(match op {
                    Lt => ord == Ordering::Less,
                    Le => ord != Ordering::Greater,
                    Gt => ord == Ordering::Greater,
                    _ => ord != Ordering::Less,
                }))
            }
            Add | Sub | Mul | Div | Mod => {
                let (a, b) = (self.number(lhs)?, self.number(rhs)?);
                let v = match op {
                    Add => a + b,
                    Sub => a - b,
                    Mul => a * b,
                    Div => {
                        if b == 0.0 {
                            return Err(FormulaError::DivZero);
                        }
                        a / b
                    }
                    _ => {
                        if b == 0.0 {
                            return Err(FormulaError::DivZero);
                        }
                        a % b
                    }
                };
                number(v)
            }
        }
    }

    fn call(&mut self, func: Func, args: &[Node]) -> Result<Value, FormulaError> {
        match func {
            Func::If => {
                let [c, a, b] = args else {
                    return Err(FormulaError::Type(T_ARITY));
                };
                if self.boolean(c)? {
                    self.node(a)
                } else {
                    self.node(b)
                }
            }
            Func::Concat => {
                let mut s = String::new();
                for arg in args {
                    join(&mut s, &self.node(arg)?)?;
                }
                Ok(Value::String(s))
            }
            Func::Round => {
                let (x, d) = match args {
                    [x] => (x, None),
                    [x, d] => (x, Some(d)),
                    _ => return Err(FormulaError::Type(T_ARITY)),
                };
                let digits = match d {
                    Some(n) => self.digits(n)?,
                    None => 0,
                };
                let scale = 10f64.powi(digits);
                number((self.number(x)? * scale).round() / scale)
            }
            Func::Min | Func::Max => {
                let (first, rest) = match args.split_first() {
                    Some(split) => split,
                    None => return Err(FormulaError::Type(T_ARITY)),
                };
                let pick = matches!(func, Func::Max);
                let mut best = self.number(first)?;
                for n in rest {
                    let v = self.number(n)?;
                    best = if pick { best.max(v) } else { best.min(v) };
                }
                number(best)
            }
            Func::Abs => {
                let [a] = args else {
                    return Err(FormulaError::Type(T_ARITY));
                };
                number(self.number(a)?.abs())
            }
            Func::Len => {
                let [a] = args else {
                    return Err(FormulaError::Type(T_ARITY));
                };
                match self.node(a)? {
                    Value::String(s) => number(s.chars().count() as f64),
                    Value::Array(items) => number(items.len() as f64),
                    _ => Err(FormulaError::Type(T_LEN)),
                }
            }
            Func::Today => Ok(Value::String(self.today.to_string())),
            Func::Coalesce => {
                for n in args {
                    let v = self.node(n)?;
                    if !v.is_null() {
                        return Ok(v);
                    }
                }
                Ok(Value::Null)
            }
        }
    }

    /// A digit count for `round`'s second argument: a whole number in `0..=9`.
    fn digits(&mut self, n: &Node) -> Result<i32, FormulaError> {
        let v = self.number(n)?;
        if v.fract() != 0.0 || !(0.0..=9.0).contains(&v) {
            return Err(FormulaError::Type(T_DIGITS));
        }
        Ok(v as i32)
    }
}

/// Every number result goes through here: `serde_json` has no non-finite number, so NaN and
/// an overflow become `#ERR(not finite)` instead of a value that cannot be serialised.
fn number(v: f64) -> Result<Value, FormulaError> {
    if !v.is_finite() {
        return Err(FormulaError::NotFinite);
    }
    Number::from_f64(v)
        .map(Value::Number)
        .ok_or(FormulaError::NotFinite)
}

fn as_number(v: &Value) -> Result<f64, FormulaError> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or(FormulaError::Type(T_NUM)),
        _ => Err(FormulaError::Type(T_NUM)),
    }
}

fn equal(l: &Value, r: &Value) -> Result<bool, FormulaError> {
    if l.is_null() || r.is_null() {
        return Ok(l.is_null() && r.is_null());
    }
    match (l, r) {
        // 1 and 1.0 are the same number however serde_json stored each of them.
        (Value::Number(_), Value::Number(_)) => Ok(as_number(l)? == as_number(r)?),
        (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Array(_), Value::Array(_))
        | (Value::Object(_), Value::Object(_)) => Ok(l == r),
        _ => Err(FormulaError::Type(T_ORDER)),
    }
}

fn order(l: &Value, r: &Value) -> Result<Ordering, FormulaError> {
    match (l, r) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.partial_cmp(&y).ok_or(FormulaError::Type(T_ORDER)),
            _ => Err(FormulaError::Type(T_ORDER)),
        },
        (Value::String(a), Value::String(b)) => Ok(a.cmp(b)),
        _ => Err(FormulaError::Type(T_ORDER)),
    }
}

fn join(out: &mut String, v: &Value) -> Result<(), FormulaError> {
    match v {
        Value::Null => {}
        Value::String(s) => out.push_str(s),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&n.to_string()),
        _ => return Err(FormulaError::Type(T_JOIN)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::parse;

    /// `a` carries a number, `gone` is a field the row does not carry at all.
    fn a_row(k: &str) -> Option<Value> {
        match k {
            "a" => Some(Value::from(7)),
            "gone" => None,
            _ => Some(Value::Null),
        }
    }

    fn row<'a>() -> EvalCtx<'a> {
        EvalCtx {
            field: &a_row,
            today: "1999-01-01",
        }
    }

    fn expr(src: &str) -> Expr {
        parse(src, &|k| k == "a" || k == "gone").unwrap()
    }

    #[test]
    fn step_budget_counts_every_node_visit() {
        // `1+1` is three nodes: two literals and the addition.
        let e = expr("1+1");
        assert!(eval_with_budget(&e, &row(), 3).is_ok());
        assert_eq!(eval_with_budget(&e, &row(), 2), Err(FormulaError::Budget));
        assert!(eval_with_budget(&e, &row(), MAX_STEPS).is_ok());
    }

    #[test]
    fn short_circuiting_spends_no_steps_on_the_untaken_side() {
        let e = expr("true or 1/0 == 0");
        assert_eq!(eval_with_budget(&e, &row(), 2), Ok(Value::Bool(true)));
        assert_eq!(eval_with_budget(&e, &row(), 1), Err(FormulaError::Budget));
        // The same expression with the disjunction forced costs the whole tree.
        let e = expr("false or 1/0 == 0");
        assert_eq!(
            eval_with_budget(&e, &row(), MAX_STEPS),
            Err(FormulaError::DivZero)
        );
    }

    #[test]
    fn a_field_the_row_does_not_carry_reads_as_null() {
        assert_eq!(eval(&expr("{gone}"), &row()), Ok(Value::Null));
        assert_eq!(
            eval(&expr("coalesce({gone},\"d\")"), &row()),
            Ok(Value::String("d".into()))
        );
        assert_eq!(
            eval(&expr("{gone}+1"), &row()),
            Err(FormulaError::Type(T_NUM))
        );
        assert_eq!(eval(&expr("{a}*2"), &row()).unwrap().as_f64(), Some(14.0));
    }

    #[test]
    fn today_is_the_injected_clock() {
        assert_eq!(
            eval(&expr("today()"), &row()),
            Ok(Value::String("1999-01-01".into()))
        );
    }

    #[test]
    fn integral_results_stay_as_serde_json_prints_them() {
        // Not special-cased: however a number arrived — literal or division — the cell shows
        // the same text, because both go through the same `Number`.
        assert_eq!(
            eval(&expr("concat(4/2)"), &row()),
            eval(&expr("concat(2)"), &row()),
        );
    }
}
