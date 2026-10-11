//! The formula language, exercised through its public API: precedence, the three budgets,
//! call semantics, and error cells. Hostile input is `formula::hostile_input` in the lib.

use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

use gw_core::formula::{
    eval, field_refs, parse, EvalCtx, Expr, FormulaError, MAX_DEPTH, MAX_NODES, MAX_STEPS,
};

const KNOWN: [&str; 6] = ["a", "b", "name", "due", "count", "tags"];

fn known() -> impl Fn(&str) -> bool {
    let keys: Vec<&str> = KNOWN.to_vec();
    move |k| keys.contains(&k)
}

/// `{a}` resolves to `1`, `{b}` to `2`, `{name}` to `"Ann"`, `{due}` to `"2026-01-02"`,
/// `{count}` to `0`, `{tags}` to an array; any other key is not in this row.
fn row_field(k: &str) -> Option<Value> {
    match k {
        "a" => Some(json!(1)),
        "b" => Some(json!(2)),
        "name" => Some(json!("Ann")),
        "due" => Some(json!("2026-01-02")),
        "count" => Some(json!(0)),
        "tags" => Some(json!(["x", "y", "z"])),
        _ => None,
    }
}

fn ctx() -> EvalCtx<'static> {
    EvalCtx {
        field: &row_field,
        today: "2026-10-10",
    }
}

fn p(src: &str) -> Result<Expr, FormulaError> {
    parse(src, &known())
}

fn v(src: &str) -> Value {
    let e = p(src).unwrap_or_else(|e| panic!("parse {src:?}: {e}"));
    eval(&e, &ctx()).unwrap_or_else(|e| panic!("eval {src:?}: {e}"))
}

fn f(src: &str) -> f64 {
    v(src)
        .as_f64()
        .unwrap_or_else(|| panic!("{src:?} did not give a number, got {:?}", v(src)))
}

fn err(src: &str) -> FormulaError {
    match p(src) {
        Ok(e) => eval(&e, &ctx())
            .err()
            .unwrap_or_else(|| panic!("{src:?} succeeded")),
        Err(e) => e,
    }
}

fn n(src: &str, expected: &FormulaError) {
    let got = err(src);
    assert_eq!(&got, expected, "{src:?}");
    let text = got.cell_text();
    assert!(text.starts_with("#ERR("), "{src:?} -> {text}");
    assert!(text.is_ascii(), "{src:?} -> {text}");
    assert!(text.len() < 64, "{src:?} -> {text}");
}

#[test]
fn limits_are_the_published_ones() {
    assert_eq!((MAX_NODES, MAX_STEPS, MAX_DEPTH), (256, 10_000, 32));
}

#[test]
fn precedence_binds_as_specified() {
    assert_eq!(f("1+2*3"), 7.0);
    assert_eq!(f("(1+2)*3"), 9.0);
    assert_eq!(f("7%3"), 1.0);
    assert_eq!(f("10/4"), 2.5);
    assert_eq!(v("1+2<4*2"), json!(true)); // (1+2) < (4*2)
    assert_eq!(v("not true or true"), json!(true));
    assert_eq!(v("not 1 < 2"), json!(false)); // not (1<2)
    assert_eq!(v("true and false or true"), json!(true));
    assert_eq!(f("-2+3"), 1.0);
    assert_eq!(f("2*-3"), -6.0);
    assert_eq!(f("-(1+2)*3"), -9.0);
    assert_eq!(v("1 == 1.0"), json!(true));
    assert_eq!(v("1 != 2"), json!(true));
    assert_eq!(v("\"x\" == \"x\""), json!(true));
    assert_eq!(v("\"a\" == \"b\""), json!(false));
    assert_eq!(v("{tags} == {tags}"), json!(true));
    n("{tags} == 1", &FormulaError::Type("comparable types")); // mixed types are an error
    assert_eq!(v("len(\"ab\") == 2"), json!(true));
}

#[test]
fn literals_and_escapes() {
    assert_eq!(f("3.5"), 3.5);
    assert_eq!(v("true"), json!(true));
    assert_eq!(v("null"), Value::Null);
    assert_eq!(v("\"a\\\"b\\\\c\""), json!("a\"b\\c"));
    assert_eq!(f("{a}+{b}"), 3.0);
    assert_eq!(v("{name}"), json!("Ann"));
}

#[test]
fn calls_behave_as_specified() {
    assert_eq!(f("if(1<2,10,20)"), 10.0);
    assert_eq!(f("if(1>2,10,20)"), 20.0);
    // Only the taken branch is evaluated: the other one divides by zero.
    assert_eq!(f("if(1<2,10,1/0)"), 10.0);
    assert_eq!(f("if(1>2,1/0,{a}+1)"), 2.0);
    n("if(1,2,3)", &FormulaError::Type("boolean expected"));
    // serde_json prints a computed number as a decimal, so `1` written as a literal is
    // "1.0" while a stored integer field keeps its own look. Neither is special-cased here.
    assert_eq!(v("concat(\"a\",1,true,null,{name})"), json!("a1.0trueAnn"));
    assert_eq!(v("concat(1)"), json!("1.0"));
    assert_eq!(v("concat({a})"), json!("1"));
    assert_eq!(f("round(2.5)"), 3.0);
    assert_eq!(f("round(-2.5)"), -3.0);
    assert_eq!(f("round(1.23456,2)"), 1.23);
    assert_eq!(f("round(1.23456,4)"), 1.2346);
    assert_eq!(f("round(1.23456,0)"), 1.0);
    assert_eq!(f("min(3,1,2)"), 1.0);
    assert_eq!(f("max(3,1,2)"), 3.0);
    assert_eq!(f("abs(-4)"), 4.0);
    assert_eq!(f("len(\"héllo\")"), 5.0);
    assert_eq!(f("len({tags})"), 3.0);
    assert_eq!(v("today()"), json!("2026-10-10"));
    n("round(1,10)", &FormulaError::Type("digits 0..9"));
    n("round(1,{name})", &FormulaError::Type("number expected"));
    n("round(1,10)", &FormulaError::Type("digits 0..9"));
    n("round(1,1.5)", &FormulaError::Type("digits 0..9"));
    n("concat({tags})", &FormulaError::Type("concatenable value"));
    n("min(1,{tags})", &FormulaError::Type("number expected"));
    n(
        "coalesce()",
        &FormulaError::Parse("coalesce takes at least 1 argument", 0),
    );
}

#[test]
fn coalesce_skips_nulls_and_a_missing_field_reads_null() {
    let held = json!(null);
    let row = EvalCtx {
        field: &|k| match k {
            "a" => Some(held.clone()),
            "b" => Some(json!("back")),
            _ => None,
        },
        today: "2026-10-10",
    };
    let e = p("coalesce({a},{b},\"last\")").unwrap();
    assert_eq!(eval(&e, &row).unwrap(), json!("back"));
    let e = p("coalesce({a},null)").unwrap();
    assert_eq!(eval(&e, &row).unwrap(), Value::Null);
    let e = p("{name}").unwrap();
    assert_eq!(eval(&e, &row).unwrap(), Value::Null);
    let e = p("coalesce({name},\"d\")").unwrap();
    assert_eq!(eval(&e, &row).unwrap(), json!("d"));
    let e = p("{a}+1").unwrap();
    assert!(matches!(eval(&e, &row), Err(FormulaError::Type(_))));
    let e = p("if({a},1,2)").unwrap();
    assert!(matches!(eval(&e, &row), Err(FormulaError::Type(_))));
    assert_eq!(v("{count}"), json!(0));
}

#[test]
fn and_or_need_bools_and_short_circuit() {
    assert_eq!(v("true and true"), json!(true));
    assert_eq!(v("true and false"), json!(false));
    assert_eq!(v("false or true"), json!(true));
    assert_eq!(v("true or 1/0 == 0"), json!(true)); // rhs never evaluated
    assert_eq!(v("false and 1/0 == 0"), json!(false));
    n("\"x\" and true", &FormulaError::Type("boolean expected"));
    n("1 or true", &FormulaError::Type("boolean expected"));
    n("false or 1", &FormulaError::Type("boolean expected"));
    n("true and 1", &FormulaError::Type("boolean expected"));
    n("not 1", &FormulaError::Type("boolean expected"));
}

#[test]
fn arithmetic_refuses_null_and_wrong_types() {
    n("1+\"x\"", &FormulaError::Type("number expected"));
    n("\"x\"*2", &FormulaError::Type("number expected"));
    n("\"x\"-1", &FormulaError::Type("number expected"));
    n("\"x\"/1", &FormulaError::Type("number expected"));
    n("\"x\"%1", &FormulaError::Type("number expected"));
    n("-\"x\"", &FormulaError::Type("number expected"));
    n("abs(\"x\")", &FormulaError::Type("number expected"));
    n("1<\"x\"", &FormulaError::Type("comparable types"));
    n("true<false", &FormulaError::Type("comparable types"));
    n("len(1)", &FormulaError::Type("string or array"));
    n("min(\"x\",1)", &FormulaError::Type("number expected"));
    assert_eq!(v("null == null"), json!(true));
    assert_eq!(v("null == 1"), json!(false));
    assert_eq!(v("null != 1"), json!(true));
    assert_eq!(v("true == true"), json!(true));
    assert_eq!(v("{name} == \"Ann\""), json!(true));
    assert_eq!(v("\"a\" < \"b\""), json!(true));
    assert_eq!(v("1 <= 1"), json!(true));
    assert_eq!(v("2 >= 3"), json!(false));
}

#[test]
fn divide_and_modulo_by_zero() {
    n("1/0", &FormulaError::DivZero);
    n("1%0", &FormulaError::DivZero);
    n("1/{count}", &FormulaError::DivZero);
    n("1/(2-2)", &FormulaError::DivZero);
    let text = err("1/0").cell_text();
    assert!(text.starts_with("#ERR("), "{text}");
    assert!(text.is_ascii(), "{text}");
}

#[test]
fn node_budget_refuses_a_two_hundred_seventh_node() {
    // n numbers summed left-associatively are n atoms + n-1 operators = 2n-1 nodes.
    let at_limit = vec!["1"; 128].join("+"); // 255 nodes
    p(&at_limit).unwrap();
    let over = vec!["1"; 129].join("+"); // 257 nodes
    n(&over, &FormulaError::TooManyNodes);
    // A call's argument nodes count too.
    let deep = format!(
        "abs({}+{})",
        vec!["1"; 120].join("+"),
        vec!["1"; 120].join("+")
    );
    n(&deep, &FormulaError::TooManyNodes);
}

#[test]
fn depth_budget_refuses_nesting_beyond_thirty_two() {
    let at_limit = "(".repeat(MAX_DEPTH) + "1" + &")".repeat(MAX_DEPTH);
    p(&at_limit).unwrap();
    let over = "(".repeat(MAX_DEPTH + 1) + "1" + &")".repeat(MAX_DEPTH + 1);
    n(&over, &FormulaError::TooDeep);
    // 100 000 open parens must be refused, not blow the stack.
    let absurd = "(".repeat(100_000);
    let got = catch_unwind(AssertUnwindSafe(|| p(&absurd)));
    match got {
        Ok(Err(FormulaError::TooDeep)) => {}
        Ok(other) => panic!("100 000 parens gave {other:?}"),
        Err(_) => panic!("100 000 parens panicked"),
    }
    // A long flat chain is refused by the node budget, also without recursion.
    let long = vec!["1"; 100_000].join("+");
    assert!(matches!(p(&long), Err(FormulaError::TooManyNodes)));
    let long_parens = vec!["(1)"; 200].join("+");
    assert!(matches!(p(&long_parens), Err(FormulaError::TooManyNodes)));
    let deep_not = "not ".repeat(40) + "true";
    assert!(matches!(p(&deep_not), Err(FormulaError::TooDeep)));
    let deep_neg = "-".repeat(40) + "1";
    assert!(matches!(p(&deep_neg), Err(FormulaError::TooDeep)));
}

#[test]
fn unknown_field_refs_are_refused_at_parse() {
    assert_eq!(
        p("{nope}").unwrap_err(),
        FormulaError::UnknownField("nope".into())
    );
    assert_eq!(
        p("1+{count2}").unwrap_err(),
        FormulaError::UnknownField("count2".into())
    );
    assert_eq!(
        p("{Bad Key}").unwrap_err(),
        FormulaError::UnknownField("Bad Key".into())
    );
    assert_eq!(p("{}").unwrap_err(), FormulaError::UnknownField("".into()));
    assert_eq!(
        p("{id}").unwrap_err(),
        FormulaError::UnknownField("id".into())
    );
    let text = p("{nope}").unwrap_err().cell_text();
    assert_eq!(text, "#ERR(unknown field nope)");
    assert!(p("{a}+{b}").is_ok());
}

#[test]
fn field_refs_are_distinct_and_ordered() {
    let e = p("{b}+{a}+{b}+{count}*0").unwrap();
    assert_eq!(field_refs(&e), vec!["b", "a", "count"]);
    assert_eq!(field_refs(&p("1+1").unwrap()), Vec::<String>::new());
    assert_eq!(
        field_refs(&p("if({due},{name},{due})").unwrap()),
        vec!["due", "name"]
    );
}

#[test]
fn syntax_and_lex_failures() {
    n("1+", &FormulaError::Parse("expected a value", 2));
    n("+", &FormulaError::Parse("expected a value", 0));
    n("1 2", &FormulaError::Parse("expected end of input", 2));
    n("(1", &FormulaError::Parse("expected )", 2));
    n("1)", &FormulaError::Parse("expected end of input", 1));
    n("if(1)", &FormulaError::Parse("if takes 3 arguments", 0));
    n(
        "today(1)",
        &FormulaError::Parse("today takes no arguments", 0),
    );
    n("abs()", &FormulaError::Parse("abs takes 1 argument", 0));
    n(
        "round(1,2,3)",
        &FormulaError::Parse("round takes 1 or 2 arguments", 0),
    );
    n("nope(1)", &FormulaError::Parse("unknown function", 0));
    n("if(1<2,1)", &FormulaError::Parse("if takes 3 arguments", 0));
    n("\"abc", &FormulaError::Lex("unterminated string", 0));
    n("\"a\\qb\"", &FormulaError::Lex("bad escape", 2));
    n(
        "{abc",
        &FormulaError::Lex("unterminated field reference", 0),
    );
    n(
        "{a{b}",
        &FormulaError::Lex("unterminated field reference", 0),
    );
    n("1$", &FormulaError::Lex("unexpected character", 1));
    n("1=1", &FormulaError::Lex("expected ==", 1));
    n("1!", &FormulaError::Lex("expected !=", 1));
    n(".5", &FormulaError::Lex("unexpected character", 0));
    n("1.", &FormulaError::Lex("number ends with a dot", 0));
    n("", &FormulaError::Parse("expected a value", 0));
    n("TRUE", &FormulaError::Parse("unknown identifier", 0));
    n("if 1", &FormulaError::Parse("unknown identifier", 0));
    n("1 and true", &FormulaError::Type("boolean expected"));
    // A position is a char index, so a multi-byte character ahead of the error still counts
    // as one, and `1+😀` is refused instead of indexing into the middle of a code point.
    n("\"é\"+", &FormulaError::Parse("expected a value", 4));
    n("1+😀", &FormulaError::Lex("unexpected character", 2));
}

#[test]
fn non_finite_results_are_errors() {
    let overflow = format!("1{}", "0".repeat(400));
    n(&overflow, &FormulaError::Lex("number out of range", 0));
    // 1e160 is a fine literal; their product is not a number at all.
    let big = format!("1{}", "0".repeat(160));
    n(&format!("{big}*{big}"), &FormulaError::NotFinite);
    n("0/0", &FormulaError::DivZero);
    assert_eq!(f(&format!("{big}/{big}")), 1.0);
}

#[test]
fn every_error_cell_is_a_bounded_ascii_cell() {
    let cases = [
        "1+",
        "\"abc",
        "{a}+{nope}",
        "1/0",
        "1e400*1e400",
        "(((((",
        "()",
        "if(1)",
        "{id}",
        "1+2*3-(",
        "not",
        "coalesce()",
        "(((1",
    ];
    for src in cases {
        let e = err(src);
        let text = e.cell_text();
        assert!(
            text.starts_with("#ERR(") && text.ends_with(')'),
            "{src:?} -> {text}"
        );
        assert!(text.is_ascii(), "{src:?} -> {text}");
        assert!(text.len() <= 64, "{src:?} -> {text}");
    }
}
