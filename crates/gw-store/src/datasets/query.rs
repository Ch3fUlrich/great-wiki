//! The dataset query builder (ADR 0029): filters, a typed sort and a keyset cursor over
//! `dataset_row`, as SQL whose every variable part is a bound parameter.
//!
//! **Field references exist only as [`FieldKey`].** There is no constructor taking a `&str`
//! or `String` name: a key is validated by `FieldKey::parse` (`^[a-z][a-z0-9_]*$`) before it
//! can be named here, and even then it travels as the bound JSON path of `json_extract`,
//! never as SQL text. Values are bound too. The only text in the statement that is not a
//! literal of this file is the placeholder count of an `IN` list, derived from a length.

use gw_core::dataset::FieldKey;
use serde::Deserialize;
use serde_json::Value;

/// Most filters one query carries.
pub const MAX_FILTERS: usize = 10;
/// Most values one `in` filter lists.
pub const MAX_IN_VALUES: usize = 100;

/// One condition on a field. Deserialized from the API, so a hostile key dies in
/// `FieldKey`'s own `Deserialize`, before any SQL exists.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Filter {
    Eq { key: FieldKey, value: Value },
    Contains { key: FieldKey, value: String },
    IsEmpty { key: FieldKey },
    In { key: FieldKey, values: Vec<Value> },
}

/// How a sort compares: by number or as text. Chosen by the caller from the field's kind,
/// never from the data, so `10` follows `9` for a number field and precedes it for text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    Number,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sort {
    pub key: FieldKey,
    pub kind: SortKind,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum CursorValue {
    Null,
    Num(f64),
    Text(String),
}

/// Where the previous page ended: the last row's sort value and id.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    value: CursorValue,
    id: String,
}

impl Cursor {
    pub(super) fn of(num: Option<f64>, text: Option<String>, id: String) -> Self {
        let value = match (num, text) {
            (Some(n), _) => CursorValue::Num(n),
            (None, Some(t)) => CursorValue::Text(t),
            (None, None) => CursorValue::Null,
        };
        Cursor { value, id }
    }

    /// Opaque token: hex of a small JSON object.
    pub fn encode(&self) -> String {
        let v = match &self.value {
            CursorValue::Null => Value::Null,
            CursorValue::Num(n) => serde_json::json!(n),
            CursorValue::Text(t) => Value::String(t.clone()),
        };
        let json = serde_json::json!({ "v": v, "id": self.id }).to_string();
        json.bytes().map(|b| format!("{b:02x}")).collect()
    }

    pub fn decode(token: &str) -> Result<Self, String> {
        let bad = || "the cursor is not valid".to_string();
        if !token.len().is_multiple_of(2) || token.len() > 4096 || !token.is_ascii() {
            return Err(bad());
        }
        let bytes: Vec<u8> = (0..token.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&token[i..i + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(|_| bad())?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| bad())?;
        let id = v.get("id").and_then(Value::as_str).ok_or_else(bad)?;
        let value = match v.get("v") {
            Some(Value::Null) | None => CursorValue::Null,
            Some(Value::Number(n)) => CursorValue::Num(n.as_f64().ok_or_else(bad)?),
            Some(Value::String(s)) => CursorValue::Text(s.clone()),
            Some(_) => return Err(bad()),
        };
        Ok(Cursor {
            value,
            id: id.to_string(),
        })
    }
}

/// A query over one dataset's rows.
#[derive(Debug, Clone, Default)]
pub struct RowQuery {
    pub filters: Vec<Filter>,
    pub sort: Option<Sort>,
    pub after: Option<Cursor>,
    pub limit: i64,
}

/// A bound parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Bind {
    Text(String),
    Real(f64),
    Int(i64),
}

/// The statements for one query. `where_sql` is shared by the count and the page.
#[derive(Debug, Clone)]
pub struct Built {
    /// Extra select columns `sn`, `st`: the row's sort value, for the next cursor.
    pub sort_cols_sql: String,
    pub sort_cols_binds: Vec<Bind>,
    pub where_sql: String,
    pub where_binds: Vec<Bind>,
    pub page_sql: String,
    pub page_binds: Vec<Bind>,
}

/// `json_extract` of one field, its path a bound parameter.
const CELL: &str = "json_extract(\"values\", ?)";

/// The bound JSON path of a field. `FieldKey` is `[a-z][a-z0-9_]*`, so it cannot break out
/// of the path either, but the point is that it never becomes SQL text at all.
fn path_bind(key: &FieldKey) -> Bind {
    Bind::Text(format!("$.{}", key.as_str()))
}

fn scalar(v: &Value) -> Result<Bind, String> {
    match v {
        Value::String(s) => Ok(Bind::Text(s.clone())),
        Value::Bool(b) => Ok(Bind::Int(i64::from(*b))),
        Value::Number(n) => Ok(n
            .as_i64()
            .map(Bind::Int)
            .or_else(|| n.as_f64().map(Bind::Real))
            .ok_or("a filter number is out of range")?),
        _ => Err("a filter value is a string, a number or a boolean".into()),
    }
}

fn filter_sql(f: &Filter, binds: &mut Vec<Bind>) -> Result<String, String> {
    Ok(match f {
        Filter::Eq { key, value } => {
            binds.push(path_bind(key));
            binds.push(scalar(value)?);
            format!("{CELL} = ?")
        }
        Filter::Contains { key, value } => {
            // `instr`, not `LIKE`: no wildcard in the needle means anything.
            binds.push(path_bind(key));
            binds.push(Bind::Text(value.clone()));
            format!("instr(lower(CAST({CELL} AS TEXT)), lower(?)) > 0")
        }
        Filter::IsEmpty { key } => {
            binds.push(path_bind(key));
            binds.push(path_bind(key));
            format!("({CELL} IS NULL OR CAST({CELL} AS TEXT) IN ('', '[]'))")
        }
        Filter::In { key, values } => {
            if values.is_empty() || values.len() > MAX_IN_VALUES {
                return Err(format!("an `in` filter lists 1 to {MAX_IN_VALUES} values"));
            }
            binds.push(path_bind(key));
            for v in values {
                binds.push(scalar(v)?);
            }
            // Only the number of placeholders is generated, from a length.
            format!("{CELL} IN ({})", vec!["?"; values.len()].join(", "))
        }
    })
}

/// The typed sort expression; NULL for a row without a value.
fn sort_expr(sort: &Sort, binds: &mut Vec<Bind>) -> String {
    binds.push(path_bind(&sort.key));
    let ty = match sort.kind {
        SortKind::Number => "REAL",
        SortKind::Text => "TEXT",
    };
    format!("CAST({CELL} AS {ty})")
}

impl RowQuery {
    /// Statements for `doc_id`'s rows. Rows without a sort value come last in either
    /// direction; ties break on `id`, ascending, so the order is total and a cursor
    /// (sort value, id) names exactly one position in it.
    pub fn build(&self, doc_id: &str) -> Result<Built, String> {
        if self.filters.len() > MAX_FILTERS {
            return Err(format!("a query has at most {MAX_FILTERS} filters"));
        }
        let mut where_binds = vec![Bind::Text(doc_id.to_string())];
        let mut where_sql = String::from("doc_id = ?");
        for f in &self.filters {
            where_sql.push_str(" AND ");
            where_sql.push_str(&filter_sql(f, &mut where_binds)?);
        }

        let mut sort_binds = Vec::new();
        let mut page_binds = Vec::new();
        let mut page_sql = String::new();
        let sort_cols_sql;
        let order;
        let mut order_binds = Vec::new();
        match &self.sort {
            None => {
                sort_cols_sql = ", NULL AS sn, created_at AS st".to_string();
                if let Some(c) = &self.after {
                    let CursorValue::Text(v) = &c.value else {
                        return Err("the cursor does not belong to this order".into());
                    };
                    page_sql.push_str(" AND (created_at > ? OR (created_at = ? AND id > ?))");
                    page_binds.push(Bind::Text(v.clone()));
                    page_binds.push(Bind::Text(v.clone()));
                    page_binds.push(Bind::Text(c.id.clone()));
                }
                order = "created_at, id".to_string();
            }
            Some(sort) => {
                let e = sort_expr(sort, &mut sort_binds);
                sort_cols_sql = match sort.kind {
                    SortKind::Number => format!(", {e} AS sn, NULL AS st"),
                    SortKind::Text => format!(", NULL AS sn, {e} AS st"),
                };
                if let Some(c) = &self.after {
                    let value = match (&c.value, sort.kind) {
                        (CursorValue::Null, _) => None,
                        (CursorValue::Num(n), SortKind::Number) => Some(Bind::Real(*n)),
                        (CursorValue::Text(t), SortKind::Text) => Some(Bind::Text(t.clone())),
                        _ => return Err("the cursor does not belong to this order".into()),
                    };
                    match value {
                        None => {
                            page_sql.push_str(&format!(" AND ({e} IS NULL AND id > ?)"));
                            page_binds.push(path_bind(&sort.key));
                            page_binds.push(Bind::Text(c.id.clone()));
                        }
                        Some(v) => {
                            let op = if sort.desc { "<" } else { ">" };
                            page_sql.push_str(&format!(
                                " AND ({e} {op} ? OR ({e} = ? AND id > ?) OR {e} IS NULL)"
                            ));
                            page_binds.push(path_bind(&sort.key));
                            page_binds.push(v.clone());
                            page_binds.push(path_bind(&sort.key));
                            page_binds.push(v);
                            page_binds.push(Bind::Text(c.id.clone()));
                            page_binds.push(path_bind(&sort.key));
                        }
                    }
                }
                let dir = if sort.desc { "DESC" } else { "ASC" };
                order = format!("{e} IS NULL, {e} {dir}, id");
                order_binds.push(path_bind(&sort.key));
                order_binds.push(path_bind(&sort.key));
            }
        }
        page_sql.push_str(&format!(" ORDER BY {order} LIMIT ?"));
        page_binds.extend(order_binds);
        page_binds.push(Bind::Int(self.limit.saturating_add(1)));
        Ok(Built {
            sort_cols_sql,
            sort_cols_binds: sort_binds,
            where_sql,
            where_binds,
            page_sql,
            page_binds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(s: &str) -> FieldKey {
        FieldKey::parse(s).unwrap()
    }

    fn built(q: &RowQuery) -> Built {
        q.build("doc-1").unwrap()
    }

    fn all_sql(b: &Built) -> String {
        format!("{} {} {}", b.sort_cols_sql, b.where_sql, b.page_sql)
    }

    #[test]
    fn every_filter_kind_builds_with_bound_values_only() {
        let q = RowQuery {
            filters: vec![
                Filter::Eq {
                    key: key("title"),
                    value: json!("MARKER_EQ"),
                },
                Filter::Contains {
                    key: key("note"),
                    value: "MARKER_CONTAINS".into(),
                },
                Filter::IsEmpty { key: key("owner") },
                Filter::In {
                    key: key("state"),
                    values: vec![json!("MARKER_A"), json!("MARKER_B"), json!(3)],
                },
            ],
            limit: 10,
            ..Default::default()
        };
        let b = built(&q);
        let sql = all_sql(&b);
        for leak in ["title", "note", "owner", "state", "MARKER", "doc-1"] {
            assert!(!sql.contains(leak), "{leak} reached the SQL text: {sql}");
        }
        let all = format!("{:?}", b.where_binds);
        for bound in [
            "$.title",
            "$.note",
            "$.owner",
            "$.state",
            "MARKER_EQ",
            "MARKER_CONTAINS",
            "MARKER_A",
            "MARKER_B",
            "doc-1",
        ] {
            assert!(all.contains(bound), "{bound} not bound: {all}");
        }
        assert_eq!(
            sql.matches('?').count(),
            b.sort_cols_binds.len() + b.where_binds.len() + b.page_binds.len(),
            "placeholders and binds must pair up"
        );
    }

    #[test]
    fn a_sort_key_and_cursor_value_are_bound_too() {
        let q = RowQuery {
            sort: Some(Sort {
                key: key("amount"),
                kind: SortKind::Number,
                desc: true,
            }),
            after: Some(Cursor::of(Some(7.5), None, "row-9".into())),
            limit: 5,
            ..Default::default()
        };
        let b = built(&q);
        let sql = all_sql(&b);
        assert!(!sql.contains("amount") && !sql.contains("row-9") && !sql.contains("7.5"));
        assert_eq!(
            sql.matches('?').count(),
            b.sort_cols_binds.len() + b.where_binds.len() + b.page_binds.len()
        );
        assert!(b.page_binds.contains(&Bind::Real(7.5)));
        assert!(b.page_binds.contains(&Bind::Text("$.amount".into())));
    }

    #[test]
    fn an_unusable_filter_value_is_refused() {
        for f in [
            Filter::Eq {
                key: key("a"),
                value: Value::Null,
            },
            Filter::Eq {
                key: key("a"),
                value: json!([1]),
            },
            Filter::In {
                key: key("a"),
                values: vec![],
            },
            Filter::In {
                key: key("a"),
                values: vec![json!({"x": 1})],
            },
            Filter::In {
                key: key("a"),
                values: vec![json!(1); MAX_IN_VALUES + 1],
            },
        ] {
            let q = RowQuery {
                filters: vec![f.clone()],
                limit: 1,
                ..Default::default()
            };
            assert!(q.build("d").is_err(), "{f:?}");
        }
        let many = RowQuery {
            filters: vec![Filter::IsEmpty { key: key("a") }; MAX_FILTERS + 1],
            limit: 1,
            ..Default::default()
        };
        assert!(many.build("d").is_err());
    }

    #[test]
    fn a_hostile_key_is_refused_before_any_sql_exists() {
        let hostile = "a') OR 1=1 --";
        assert!(FieldKey::parse(hostile).is_err());
        // The API door: a filter that names it fails to deserialize at all.
        let body = json!({ "op": "eq", "key": hostile, "value": 1 });
        assert!(serde_json::from_value::<Filter>(body).is_err());
        let ok = json!({ "op": "eq", "key": "a", "value": 1 });
        assert!(serde_json::from_value::<Filter>(ok).is_ok());
        let extra = json!({ "op": "is_empty", "key": "a", "sql": "1=1" });
        assert!(serde_json::from_value::<Filter>(extra).is_err());
    }

    /// The builder has no `&str` (or `String`) way to name a field: every key in a public
    /// signature or field is a `FieldKey`. Read from the source because "this constructor
    /// does not exist" cannot be asserted any other way; `Filter` and `Sort` are also
    /// built above from `FieldKey`s alone, which compiles only while that is true.
    #[test]
    fn the_builder_has_no_str_path_for_a_field_reference() {
        let src = include_str!("query.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let mut keyed = 0;
        for line in code.lines() {
            let l = line.trim();
            let names_key = l.contains("key:") || l.contains("key,") || l.contains("key)");
            let signature = l.starts_with("pub ")
                || l.contains("fn ")
                || l.starts_with("Eq ")
                || l.starts_with("Contains ")
                || l.starts_with("IsEmpty ")
                || l.starts_with("In ");
            if names_key && signature && !l.starts_with("//") {
                keyed += 1;
                assert!(
                    !l.contains("key: &str")
                        && !l.contains("key: String")
                        && !l.contains("key: impl ")
                        && !l.contains("&str,"),
                    "a field reference that is not a FieldKey: {l}"
                );
            }
        }
        assert!(keyed >= 5, "the scan saw only {keyed} keyed lines");
    }

    #[test]
    fn a_cursor_round_trips_and_a_forged_one_is_refused() {
        for c in [
            Cursor::of(Some(10.0), None, "a".into()),
            Cursor::of(None, Some("x y".into()), "b".into()),
            Cursor::of(None, None, "c".into()),
        ] {
            assert_eq!(Cursor::decode(&c.encode()).unwrap(), c);
        }
        for bad in ["", "zz", "abc", "7b7d", "7b226964223a317d"] {
            assert!(Cursor::decode(bad).is_err(), "{bad}");
        }
    }
}
