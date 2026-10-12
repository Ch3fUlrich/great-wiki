//! Saved dataset views (ADR 0029 cl. 6, spec section 5): a name, a kind and a JSON config
//! naming field keys. Config, never data - a view cannot show a row its viewer could not read
//! through the row list, because it holds none.
//!
//! Reading goes through `readable_dataset`, every change through `writable_dataset`; an
//! unreadable dataset is `NoDataset`, the same as an absent one. Keys in a config are parsed
//! by `FieldKey` and must be fields of this dataset; they are only ever JSON, never SQL.

use super::query::Filter;
use super::{clean_label, is_computed_kind, Gate, SchemaOutcome, MAX_LABEL_CHARS};
use crate::Store;
use anyhow::Result;
use gw_auth::Principal;
use gw_core::dataset::{FieldKey, FieldKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Most views one dataset may carry.
pub const MAX_VIEWS: usize = 50;
/// Largest view config, in bytes.
const MAX_VIEW_CONFIG_BYTES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: Value,
    pub position: i64,
}

/// Config of a `table` view. Unknown members are refused, so a typo is an error, not a
/// silently ignored setting.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableConfig {
    #[serde(default)]
    fields: Vec<FieldKey>,
    sort: Option<ViewSort>,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    widths: BTreeMap<String, u32>,
    /// The column a board-style grouping runs over: any view kind may carry it, and it must
    /// name a visible, stored field — see [`validate`].
    #[serde(default)]
    group_by: Option<FieldKey>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewSort {
    key: FieldKey,
    #[serde(default)]
    #[allow(dead_code)]
    desc: bool,
}

/// Config of a `board` view (ADR 0029): the select column its lanes group by, which fields
/// the cards show, and which rows reach the board. Unknown members are refused, like tables.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardConfig {
    group_by: FieldKey,
    #[serde(default)]
    card_fields: Vec<FieldKey>,
    #[serde(default)]
    filters: Vec<Filter>,
    sort: Option<ViewSort>,
}

impl BoardConfig {
    /// Every field key the config names. Keys arrive through `FieldKey`, so a hostile string
    /// never reaches this list unparsed.
    fn keys(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![self.group_by.as_str().to_string()];
        out.extend(self.card_fields.iter().map(|k| k.as_str().to_string()));
        out.extend(self.sort.iter().map(|s| s.key.as_str().to_string()));
        out.extend(self.filters.iter().map(|f| f.key().as_str().to_string()));
        out
    }
}

/// Config of a `calendar` view (ADR 0029, M8d D3a): the date column rows are placed by, an
/// optional end column for spans, which column titles a row, and which rows reach the calendar.
/// `date` is required and only ever a date field — a datetime is the same kind with `with_time`,
/// so it passes where a plain date does. Unknown members are refused, like the other kinds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CalendarConfig {
    date: FieldKey,
    #[serde(default)]
    end: Option<FieldKey>,
    /// Left out means "the dataset's first text column", which the web resolves. The store only
    /// checks the key when the config names one.
    #[serde(default)]
    title: Option<FieldKey>,
    #[serde(default)]
    filters: Vec<Filter>,
    sort: Option<ViewSort>,
}

impl CalendarConfig {
    /// Every field key the config names, as [`BoardConfig::keys`] does.
    fn keys(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![self.date.as_str().to_string()];
        out.extend(self.end.iter().map(|k| k.as_str().to_string()));
        out.extend(self.title.iter().map(|k| k.as_str().to_string()));
        out.extend(self.sort.iter().map(|s| s.key.as_str().to_string()));
        out.extend(self.filters.iter().map(|f| f.key().as_str().to_string()));
        out
    }
}

/// Config of a `gallery` view (ADR 0029, M8d D4a): the file column its cards are covered by,
/// which column titles a card, which fields show under it, and which rows reach the gallery.
/// `cover` is required and only ever a file field; whether that attachment is an image is decided
/// when the row is read, by its media type, so the config cannot say more than the kind. Unknown
/// members are refused, like the other kinds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GalleryConfig {
    cover: FieldKey,
    #[serde(default)]
    title: Option<FieldKey>,
    #[serde(default)]
    card_fields: Vec<FieldKey>,
    #[serde(default)]
    filters: Vec<Filter>,
    sort: Option<ViewSort>,
}

impl GalleryConfig {
    /// Every field key the config names, as [`BoardConfig::keys`] does.
    fn keys(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![self.cover.as_str().to_string()];
        out.extend(self.title.iter().map(|k| k.as_str().to_string()));
        out.extend(self.card_fields.iter().map(|k| k.as_str().to_string()));
        out.extend(self.sort.iter().map(|s| s.key.as_str().to_string()));
        out.extend(self.filters.iter().map(|f| f.key().as_str().to_string()));
        out
    }
}

/// Most fields one form may ask for.
const MAX_FORM_FIELDS: usize = 100;

/// Config of a `form` view (M8c C5): the fields it asks for, in the order it asks, and an
/// optional heading. Unknown members are refused, like the other kinds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FormConfig {
    pub(super) fields: Vec<FormField>,
    #[allow(dead_code)] // the form's own heading; the row does not carry it
    title: Option<String>,
}

/// One question of a form. `label` overrides the column's label for the form only; `required`
/// is a promise the submit path enforces, not a hint.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FormField {
    pub(super) key: FieldKey,
    label: Option<String>,
    #[serde(default)]
    pub(super) required: bool,
}

impl FormConfig {
    /// A saved config, parsed the way [`validate`] parsed it when it was saved.
    pub(super) fn parse(config: &Value) -> Result<Self, String> {
        serde_json::from_value(config.clone())
            .map_err(|e| format!("not a valid form view config: {e}"))
    }

    /// The subset a submit may name: the keys this form holds.
    pub(super) fn holds(&self, key: &str) -> bool {
        self.fields.iter().any(|f| f.key.as_str() == key)
    }
}

/// A form asks for stored, visible columns and nothing else: a computed one is answered by the
/// dataset, not by whoever submits, and a field the caller may not see is not a field they may
/// be asked about.
fn validate_form(config: &Value, known: &HashMap<String, String>) -> Result<(), String> {
    let parsed = FormConfig::parse(config)?;
    if parsed.fields.is_empty() {
        return Err("a form has at least one field".into());
    }
    if parsed.fields.len() > MAX_FORM_FIELDS {
        return Err(format!("a form has at most {MAX_FORM_FIELDS} fields"));
    }
    let mut seen = HashSet::new();
    for f in &parsed.fields {
        let key = f.key.as_str();
        if !seen.insert(key.to_string()) {
            return Err(format!("the form field {key:?} is asked for twice"));
        }
        if !known.contains_key(key) {
            return Err(format!("the dataset has no field {key:?}"));
        }
        if is_computed_kind(&known[key]) {
            return Err("computed fields cannot be on a form".into());
        }
        if let Some(label) = &f.label {
            let trimmed = label.trim();
            if trimmed.is_empty() {
                return Err("a form field label is not blank".into());
            }
            if trimmed.chars().count() > MAX_LABEL_CHARS {
                return Err(format!(
                    "a form field label is at most {MAX_LABEL_CHARS} characters"
                ));
            }
        }
    }
    Ok(())
}

impl TableConfig {
    /// Every field key the config names, validated: widths are keyed by plain strings (a
    /// JSON object), so they pass through `FieldKey::parse` here.
    fn keys(&self) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = self.fields.iter().map(|k| k.as_str().to_string()).collect();
        out.extend(self.sort.iter().map(|s| s.key.as_str().to_string()));
        out.extend(self.group_by.iter().map(|g| g.as_str().to_string()));
        for f in &self.filters {
            out.push(f.key().as_str().to_string());
        }
        for k in self.widths.keys() {
            let parsed = FieldKey::parse(k).map_err(|e| e.to_string())?;
            out.push(parsed.as_str().to_string());
        }
        Ok(out)
    }
}

/// Check `config` for `kind` against the fields `known` — the dataset's keys this caller may
/// see, by kind. Table, board, form, calendar and gallery are available (ADR 0029, M8c C5,
/// M8d D3a/D4a); the other kinds are in the schema's CHECK for later milestones and refused until
/// then. `group_by` has a rule of its own: in a table it groups rows under a head, and only a
/// stored column can head a group — a formula or rollup is evaluated per row, not enumerable, and
/// a relation is not groupable yet; in a board it names the lanes and must be a select field. A
/// calendar's `date`/`end` and a gallery's `cover` are refused a computed column by the same check
/// that requires them to be a date, respectively a file, field.
fn validate(kind: &str, config: &Value, known: &HashMap<String, String>) -> Result<(), String> {
    if kind != "table"
        && kind != "board"
        && kind != "form"
        && kind != "calendar"
        && kind != "gallery"
    {
        return Err(format!("the view kind {kind:?} is not available"));
    }
    if config.to_string().len() > MAX_VIEW_CONFIG_BYTES {
        return Err(format!(
            "a view config is at most {MAX_VIEW_CONFIG_BYTES} bytes"
        ));
    }
    if kind == "form" {
        return validate_form(config, known);
    }
    if kind == "gallery" {
        let parsed: GalleryConfig = serde_json::from_value(config.clone())
            .map_err(|e| format!("not a valid gallery view config: {e}"))?;
        for key in parsed.keys() {
            if !known.contains_key(&key) {
                return Err(format!("the dataset has no field {key:?}"));
            }
        }
        if known[parsed.cover.as_str()] != FieldKind::File.as_str() {
            return Err("cover must be a file field of the dataset".into());
        }
        if let Some(title) = &parsed.title {
            if known[title.as_str()] != FieldKind::Text.as_str() {
                return Err("title must be a text field of the dataset".into());
            }
        }
        for card in &parsed.card_fields {
            if is_computed_kind(&known[card.as_str()]) {
                return Err("computed fields cannot be on a gallery".into());
            }
        }
        return Ok(());
    }
    if kind == "calendar" {
        let parsed: CalendarConfig = serde_json::from_value(config.clone())
            .map_err(|e| format!("not a valid calendar view config: {e}"))?;
        for key in parsed.keys() {
            if !known.contains_key(&key) {
                return Err(format!("the dataset has no field {key:?}"));
            }
        }
        let is_date = |key: &FieldKey| known[key.as_str()] == FieldKind::Date.as_str();
        if !is_date(&parsed.date) {
            return Err("date must be a date field of the dataset".into());
        }
        if let Some(end) = &parsed.end {
            if !is_date(end) {
                return Err("end must be a date field of the dataset".into());
            }
            if end == &parsed.date {
                return Err("end must name another field than date".into());
            }
        }
        if let Some(title) = &parsed.title {
            if known[title.as_str()] != FieldKind::Text.as_str() {
                return Err("title must be a text field of the dataset".into());
            }
        }
        return Ok(());
    }
    if kind == "board" {
        let parsed: BoardConfig = serde_json::from_value(config.clone())
            .map_err(|e| format!("not a valid board view config: {e}"))?;
        for key in parsed.keys() {
            if !known.contains_key(&key) {
                return Err(format!("the dataset has no field {key:?}"));
            }
        }
        if known[parsed.group_by.as_str()] != FieldKind::Select.as_str() {
            return Err("group_by must be a select field of the dataset".into());
        }
        return Ok(());
    }
    let parsed: TableConfig = serde_json::from_value(config.clone())
        .map_err(|e| format!("not a valid table view config: {e}"))?;
    for key in parsed.keys()? {
        if !known.contains_key(&key) {
            return Err(format!("the dataset has no field {key:?}"));
        }
    }
    if let Some(group) = &parsed.group_by {
        let field_kind = &known[group.as_str()];
        if is_computed_kind(field_kind) {
            return Err("computed fields cannot be grouped".into());
        }
        if field_kind == FieldKind::Relation.as_str() {
            return Err("relation fields cannot be grouped yet".into());
        }
    }
    Ok(())
}

/// Drop everything in a saved config that names a field outside `visible`: the columns (a
/// board's or gallery's `card_fields` and a form's field entries included), a width, and a sort,
/// filter or grouping on it (filtering a hidden field is refused, not ignored, so a saved one
/// cannot survive for a caller who may not see the field). A board whose `group_by` is stripped is
/// left without it; that is fine, because a select field cannot be hidden — only relation and
/// computed ones can. A calendar's `date`, `end` and `title` and a gallery's `cover` go the same
/// way.
///
/// `kind` is an argument, not something inferred from the shape of `config`: the two kinds that
/// carry a `title` as a field key are named by the row the config came from. Deciding it by shape
/// was a guess that a config could lie to.
pub(crate) fn strip_hidden(kind: &str, config: &mut Value, visible: &HashSet<String>) {
    // A column entry is a key (`table`, `board`) or an object naming one (`form`); both shapes
    // are decided by the same visibility.
    let shown = |v: &Value| match v.get("key").and_then(Value::as_str) {
        Some(k) => visible.contains(k),
        None => v.as_str().is_some_and(|k| visible.contains(k)),
    };
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(fields)) = obj.get_mut("fields") {
        fields.retain(shown);
    }
    if let Some(Value::Array(fields)) = obj.get_mut("card_fields") {
        fields.retain(shown);
    }
    if let Some(Value::Object(widths)) = obj.get_mut("widths") {
        widths.retain(|k, _| visible.contains(k));
    }
    if obj
        .get("sort")
        .is_some_and(|s| !s.get("key").is_some_and(shown))
    {
        obj.remove("sort");
    }
    if obj.get("group_by").is_some_and(|g| !shown(g)) {
        obj.remove("group_by");
    }
    for member in ["date", "end", "cover"] {
        if obj.get(member).is_some_and(|d| !shown(d)) {
            obj.remove(member);
        }
    }
    // A calendar's and a gallery's `title` names a text column and goes with the hidden set; a
    // form's `title` is the heading the form prints, a string nobody may be denied, and no other
    // kind has the member at all.
    let title_is_a_key = matches!(kind, "calendar" | "gallery");
    if title_is_a_key && obj.get("title").is_some_and(|t| !shown(t)) {
        obj.remove("title");
    }
    if let Some(Value::Array(filters)) = obj.get_mut("filters") {
        filters.retain(|f| f.get("key").is_some_and(shown));
    }
}

type ViewRow = (String, String, String, String, i64);

fn row_to_view(r: ViewRow) -> DatasetView {
    DatasetView {
        id: r.0,
        name: r.1,
        kind: r.2,
        config: serde_json::from_str(&r.3).unwrap_or(Value::Null),
        position: r.4,
    }
}

fn view_name(name: &str) -> Result<String, String> {
    clean_label(name).map_err(|m| m.replace("field label", "view name"))
}

impl Store {
    /// The field keys `principal` may see in the dataset: every stored key minus the ones hidden
    /// from this caller — a relation they may not resolve, a rollup over it, a formula reaching
    /// either. This is the seam the schema and the query use, so a view cannot name a field the
    /// caller could not otherwise see or filter.
    pub(super) async fn visible_field_keys(
        &self,
        principal: &Principal,
        doc_id: &str,
    ) -> Result<HashSet<String>> {
        Ok(self
            .visible_field_kinds(principal, doc_id)
            .await?
            .into_keys()
            .collect())
    }

    /// The same seam as [`Store::visible_field_keys`], keeping each key's kind: `group_by` needs
    /// the kind to refuse a computed or relation head while still answering an unknown key as
    /// "no field".
    async fn visible_field_kinds(
        &self,
        principal: &Principal,
        doc_id: &str,
    ) -> Result<HashMap<String, String>> {
        let hidden = self.hidden_field_keys(principal, doc_id).await?;
        Ok(self
            .fields_of(doc_id)
            .await?
            .into_iter()
            .filter(|f| !hidden.contains(f.key.as_str()))
            .map(|f| (f.key, f.kind))
            .collect())
    }

    pub(super) async fn views_of(&self, doc_id: &str) -> Result<Vec<DatasetView>> {
        let rows: Vec<ViewRow> = sqlx::query_as(
            "SELECT id, name, kind, config, position FROM dataset_view \
             WHERE doc_id = ? ORDER BY position, created_at, id",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_view).collect())
    }

    /// The saved views of the dataset at `path`, for anybody who may read it, each with
    /// the fields hidden from this caller removed.
    pub async fn dataset_views(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<SchemaOutcome<Vec<DatasetView>>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(SchemaOutcome::NoDataset);
        };
        let visible = self.visible_field_keys(principal, &doc.id).await?;
        let mut views = self.views_of(&doc.id).await?;
        for v in &mut views {
            strip_hidden(&v.kind, &mut v.config, &visible);
        }
        Ok(SchemaOutcome::Done(views))
    }

    /// Save a view. Needs write.
    pub async fn create_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        name: &str,
        kind: &str,
        config: &Value,
    ) -> Result<SchemaOutcome<DatasetView>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let name = match view_name(name) {
            Ok(n) => n,
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let known = self.visible_field_kinds(principal, &doc_id).await?;
        if let Err(m) = validate(kind, config, &known) {
            return Ok(SchemaOutcome::Invalid(m));
        }
        let id = uuid::Uuid::now_v7().to_string();
        // Count and insert in one statement so two racing writers cannot pass the cap.
        let done = sqlx::query(
            "INSERT INTO dataset_view (id, doc_id, kind, name, config, position) \
             SELECT ?1, ?2, ?3, ?4, ?5, COALESCE(MAX(position) + 1, 0) \
             FROM dataset_view WHERE doc_id = ?2 HAVING COUNT(*) < ?6",
        )
        .bind(&id)
        .bind(&doc_id)
        .bind(kind)
        .bind(&name)
        .bind(config.to_string())
        .bind(MAX_VIEWS as i64)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(SchemaOutcome::Invalid(format!(
                "a dataset has at most {MAX_VIEWS} views"
            )));
        }
        let view = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id);
        Ok(view.map_or(SchemaOutcome::NoField, SchemaOutcome::Done))
    }

    /// Rename a view and/or replace its config. Needs write; the kind never changes.
    pub async fn update_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        id: &str,
        name: Option<&str>,
        config: Option<&Value>,
    ) -> Result<SchemaOutcome<DatasetView>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let Some(current) = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id)
        else {
            return Ok(SchemaOutcome::NoField);
        };
        let name = match name.map(view_name).transpose() {
            Ok(n) => n.unwrap_or(current.name),
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let config = config.cloned().unwrap_or(current.config);
        let known = self.visible_field_kinds(principal, &doc_id).await?;
        if let Err(m) = validate(&current.kind, &config, &known) {
            return Ok(SchemaOutcome::Invalid(m));
        }
        sqlx::query("UPDATE dataset_view SET name = ?, config = ? WHERE id = ? AND doc_id = ?")
            .bind(&name)
            .bind(config.to_string())
            .bind(id)
            .bind(&doc_id)
            .execute(&self.pool)
            .await?;
        let view = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id);
        Ok(view.map_or(SchemaOutcome::NoField, SchemaOutcome::Done))
    }

    /// Delete a view. Needs write.
    pub async fn delete_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        id: &str,
    ) -> Result<SchemaOutcome<()>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let gone = sqlx::query("DELETE FROM dataset_view WHERE id = ? AND doc_id = ?")
            .bind(id)
            .bind(&doc_id)
            .execute(&self.pool)
            .await?;
        Ok(if gone.rows_affected() == 0 {
            SchemaOutcome::NoField
        } else {
            SchemaOutcome::Done(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::{NewField, SchemaOutcome};
    use crate::documents::NewDocument;
    use crate::revisions::Author;
    use crate::{CreateOutcome, CreateRequest, Store};
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, DocumentType, FieldKind, Visibility};
    use serde_json::json;

    async fn fixture() -> (Store, Principal, Principal, Principal, String) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "raum".into(),
                    slug: Some("raum".into()),
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        let cleo = Principal::test("cleo", &[], &[]);
        for (who, perm) in [(&anna, Permission::Write), (&bea, Permission::Read)] {
            store
                .add_grant("/raum", Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let req = CreateRequest {
            parent: Some("/raum".into()),
            title: "Tabelle".into(),
            slug: None,
            template: None,
        };
        let CreateOutcome::Created { path, .. } = store
            .create_dataset_for(&anna, &req, false, "d")
            .await
            .unwrap()
        else {
            panic!("no dataset")
        };
        for (k, kind) in [("name", FieldKind::Text), ("alter", FieldKind::Number)] {
            let f = NewField {
                key: k.into(),
                label: k.into(),
                kind,
                config: json!({}),
            };
            store.add_dataset_field(&anna, &path, &f).await.unwrap();
        }
        store
            .add_dataset_field(
                &anna,
                &path,
                &NewField {
                    key: "status".into(),
                    label: "Status".into(),
                    kind: FieldKind::Select,
                    config: json!({"options": [
                        {"id": "offen", "label": "Offen"},
                        {"id": "fertig", "label": "Fertig"}
                    ]}),
                },
            )
            .await
            .unwrap();
        (store, anna, bea, cleo, path)
    }

    fn table() -> serde_json::Value {
        json!({"fields": ["name", "alter"], "sort": {"key": "alter", "desc": true},
               "filters": [{"op": "contains", "key": "name", "value": "a"}],
               "widths": {"name": 200}})
    }

    async fn made(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Alle", "table", &table())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_view() {
        let (store, anna, bea, _, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Alle", "table"));
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["widths"]["name"], 200);
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"fields": ["name"]})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config["fields"], json!(["name"]));
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
    }

    #[tokio::test]
    async fn view_writes_need_write_and_leave_the_views_unchanged() {
        let (store, anna, bea, cleo, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        let cfg = table();
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "table", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .update_dataset_view(&bea, &path, &v.id, Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store.delete_dataset_view(&bea, &path, &v.id).await.unwrap(),
            SchemaOutcome::ReadOnly
        );
        // A stranger learns nothing: the same verdict as an absent dataset.
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "table", &cfg)
                .await
                .unwrap(),
            store
                .create_dataset_view(&cleo, "/raum/nirgends", "X", "table", &cfg)
                .await
                .unwrap()
        );
        assert_eq!(
            store.dataset_views(&cleo, &path).await.unwrap(),
            SchemaOutcome::NoDataset
        );
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Alle");
    }

    #[tokio::test]
    async fn a_config_naming_a_missing_field_or_a_hostile_key_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for cfg in [
            json!({"fields": ["gibtsnicht"]}),
            json!({"sort": {"key": "gibtsnicht"}}),
            json!({"filters": [{"op": "is_empty", "key": "gibtsnicht"}]}),
            json!({"widths": {"gibtsnicht": 10}}),
            json!({"fields": ["a'; DROP TABLE dataset_row; --"]}),
            json!({"widths": {"name\") OR 1=1": 10}}),
            json!({"unknown": 1}),
            json!([1]),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "table", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        for (name, kind) in [
            ("", "table"),
            ("  ", "table"),
            ("x", "nonsense"),
            ("x", "board"),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, name, kind, &table())
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Invalid(_)),
                "{name}/{kind}: {out:?}"
            );
        }
        // An update is validated the same way, and a refused update changes nothing.
        let v = made(&store, &anna, &path).await;
        let out = store
            .update_dataset_view(&anna, &path, &v.id, None, Some(&json!({"fields": ["weg"]})))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["widths"]["name"], 200);
    }

    #[tokio::test]
    async fn the_hidden_field_filter_drops_hidden_keys_from_a_listed_view() {
        let (_, _, _, _, _) = fixture().await;
        let mut cfg = table();
        cfg["group_by"] = json!("alter");
        let visible: std::collections::HashSet<String> = ["name".to_string()].into();
        strip_hidden("table", &mut cfg, &visible);
        assert_eq!(cfg["fields"], json!(["name"]));
        assert!(cfg.get("sort").is_none());
        assert!(
            cfg.get("group_by").is_none(),
            "a grouping on a hidden field goes too: {cfg}"
        );
        assert_eq!(cfg["filters"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["widths"], json!({"name": 200}));
    }

    #[tokio::test]
    async fn views_go_with_their_dataset_and_are_bounded() {
        let (store, anna, _, _, path) = fixture().await;
        made(&store, &anna, &path).await;
        let n = || async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM dataset_view")
                .fetch_one(&store.pool)
                .await
                .unwrap()
        };
        assert_eq!(n().await, 1);
        sqlx::query("DELETE FROM documents")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(n().await, 0);
    }

    #[tokio::test]
    async fn the_fifty_first_view_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for _ in 0..MAX_VIEWS {
            made(&store, &anna, &path).await;
        }
        let out = store
            .create_dataset_view(&anna, &path, "x", "table", &table())
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
    }

    #[tokio::test]
    async fn a_view_id_from_another_dataset_is_not_reachable() {
        let (store, anna, _, _, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        let req = CreateRequest {
            parent: Some("/raum".into()),
            title: "Zweite".into(),
            slug: None,
            template: None,
        };
        let CreateOutcome::Created { path: other, .. } = store
            .create_dataset_for(&anna, &req, false, "d")
            .await
            .unwrap()
        else {
            panic!("no second dataset")
        };
        assert_eq!(
            store
                .delete_dataset_view(&anna, &other, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        assert_eq!(
            store
                .update_dataset_view(&anna, &other, &v.id, Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Alle");
    }

    /// A source under `/raum` both may read, a target under `/geheim` only `anna` may. `bea` holds
    /// write on the source (so she may create and list views) but not read on the target, so the
    /// relation, the rollup over it and the formula over that rollup are all hidden for her.
    async fn hidden_view_fixture() -> (Store, Principal, Principal, String) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        for slug in ["raum", "geheim"] {
            let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
            store
                .create_document(
                    Author::Import,
                    &NewDocument {
                        parent_path: None,
                        doc_type: DocumentType::Page,
                        title: slug.into(),
                        slug: Some(slug.into()),
                        language: "de".into(),
                        visibility: Visibility::Restricted,
                        body,
                        sort_key: 0,
                        topics: Vec::new(),
                    },
                    None,
                )
                .await
                .unwrap();
        }
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        for (slug, who, perm) in [
            ("/raum", &anna, Permission::Write),
            ("/geheim", &anna, Permission::Write),
            ("/raum", &bea, Permission::Read),
            ("/raum", &bea, Permission::Write),
        ] {
            store
                .add_grant(slug, Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let req = |parent: &str| CreateRequest {
            parent: Some(parent.into()),
            title: "d".into(),
            slug: None,
            template: None,
        };
        let CreateOutcome::Created { path: source, .. } = store
            .create_dataset_for(&anna, &req("/raum"), false, "d")
            .await
            .unwrap()
        else {
            panic!("no source dataset")
        };
        let CreateOutcome::Created { path: target, .. } = store
            .create_dataset_for(&anna, &req("/geheim"), false, "z")
            .await
            .unwrap()
        else {
            panic!("no target dataset")
        };
        for (key, kind) in [("name", FieldKind::Text), ("alter", FieldKind::Number)] {
            store
                .add_dataset_field(
                    &anna,
                    &source,
                    &NewField {
                        key: key.into(),
                        label: key.into(),
                        kind,
                        config: json!({}),
                    },
                )
                .await
                .unwrap();
        }
        store
            .add_dataset_field(
                &anna,
                &source,
                &NewField {
                    key: "mit".into(),
                    label: "Mit".into(),
                    kind: FieldKind::Relation,
                    config: json!({ "target": target, "multiple": true }),
                },
            )
            .await
            .unwrap();
        store
            .add_dataset_field(
                &anna,
                &source,
                &NewField {
                    key: "cnt".into(),
                    label: "Cnt".into(),
                    kind: FieldKind::Rollup,
                    config: json!({ "relation": "mit", "target_field": "x", "fn": "count" }),
                },
            )
            .await
            .unwrap();
        store
            .add_dataset_field(
                &anna,
                &source,
                &NewField {
                    key: "fr".into(),
                    label: "Fr".into(),
                    kind: FieldKind::Formula,
                    config: json!({ "expr": "{cnt} + 1" }),
                },
            )
            .await
            .unwrap();
        (store, anna, bea, source)
    }

    #[tokio::test]
    async fn a_view_config_naming_a_hidden_field_is_refused_as_an_unknown_one() {
        let (store, _, bea, source) = hidden_view_fixture().await;
        // A key bea may not see is refused in the exact words of a key that does not exist.
        let shape = |key: &str| format!("the dataset has no field {key:?}");
        for key in ["mit", "cnt", "fr", "gibtsnicht"] {
            let want = shape(key);
            for cfg in [
                json!({"fields": [key]}),
                json!({"sort": {"key": key}}),
                json!({"filters": [{"op": "is_empty", "key": key}]}),
                json!({"widths": {key: 10}}),
            ] {
                let out = store
                    .create_dataset_view(&bea, &source, "X", "table", &cfg)
                    .await
                    .unwrap();
                assert_eq!(out, SchemaOutcome::Invalid(want.clone()), "{key} in {cfg}");
            }
        }
    }

    /// `group_by` may ride on any view kind, but it names the column rows are grouped under, and
    /// only a stored column can head a group: a formula or a rollup is answered per row, a relation
    /// is not groupable in this milestone. A column hidden from the caller is refused as an unknown
    /// one, in the same words as a key that does not exist.
    #[tokio::test]
    async fn a_group_by_names_a_visible_stored_field_and_nothing_else() {
        let (store, anna, bea, source) = hidden_view_fixture().await;
        store
            .add_dataset_field(
                &anna,
                &source,
                &NewField {
                    key: "state".into(),
                    label: "State".into(),
                    kind: FieldKind::Select,
                    config: json!({"options": [{"id": "off"}, {"id": "on"}]}),
                },
            )
            .await
            .unwrap();
        let out = store
            .create_dataset_view(&anna, &source, "G", "table", &json!({"group_by": "state"}))
            .await
            .unwrap();
        let SchemaOutcome::Done(v) = out else {
            panic!("{out:?}")
        };
        assert_eq!(v.config["group_by"], json!("state"));
        for (who, cfg, want) in [
            (
                &anna,
                json!({"group_by": "fr"}),
                "computed fields cannot be grouped",
            ),
            (
                &anna,
                json!({"group_by": "cnt"}),
                "computed fields cannot be grouped",
            ),
            (
                &anna,
                json!({"group_by": "mit"}),
                "relation fields cannot be grouped yet",
            ),
            (
                &bea,
                json!({"group_by": "mit"}),
                "the dataset has no field \"mit\"",
            ),
            (
                &bea,
                json!({"group_by": "cnt"}),
                "the dataset has no field \"cnt\"",
            ),
            (
                &anna,
                json!({"group_by": "gibtsnicht"}),
                "the dataset has no field \"gibtsnicht\"",
            ),
        ] {
            assert_eq!(
                store
                    .create_dataset_view(who, &source, "X", "table", &cfg)
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(want.into()),
                "{cfg}"
            );
        }
    }

    #[tokio::test]
    async fn listing_views_strips_fields_hidden_from_the_reader() {
        let (store, anna, bea, source) = hidden_view_fixture().await;
        // anna, who sees every column, saves a view that names them all.
        let cfg = json!({
            "fields": ["name", "mit", "cnt", "fr", "alter"],
            "sort": {"key": "fr"},
            "filters": [{"op": "is_empty", "key": "cnt"}, {"op": "contains", "key": "name", "value": "a"}],
            "widths": {"mit": 100, "name": 200}
        });
        let SchemaOutcome::Done(v) = store
            .create_dataset_view(&anna, &source, "Alle", "table", &cfg)
            .await
            .unwrap()
        else {
            panic!("anna names the fields she may read")
        };
        // Her own listing is untouched.
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &source).await.unwrap() else {
            panic!()
        };
        assert_eq!(list[0].config, cfg);
        assert_eq!(list[0].id, v.id);
        // bea's listing has the hidden columns stripped from the config, not the whole view failed.
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &source).await.unwrap() else {
            panic!()
        };
        assert_eq!(
            list.len(),
            1,
            "the view still lists; only its hidden keys go"
        );
        let c = &list[0].config;
        assert_eq!(c["fields"], json!(["name", "alter"]));
        assert!(
            c.get("sort").is_none(),
            "a sort on a hidden field is dropped: {c}"
        );
        assert_eq!(
            c["filters"].as_array().unwrap().len(),
            1,
            "only the filter on a visible field survives: {c}"
        );
        assert_eq!(c["filters"][0]["key"], json!("name"));
        assert_eq!(c["widths"], json!({"name": 200}));
    }

    fn board() -> serde_json::Value {
        json!({"group_by": "status", "card_fields": ["name", "alter"],
               "filters": [{"op": "is_empty", "key": "name"}],
               "sort": {"key": "alter", "desc": true}})
    }

    async fn made_board(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Brett", "board", &board())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_board_view() {
        let (store, anna, bea, _, path) = fixture().await;
        let v = made_board(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Brett", "board"));
        assert_eq!(v.config["group_by"], json!("status"));
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["card_fields"], json!(["name", "alter"]));
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"group_by": "status"})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config, json!({"group_by": "status"}));
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
    }

    #[tokio::test]
    async fn a_board_groups_only_under_a_select_field() {
        let (store, anna, _, _, path) = fixture().await;
        for key in ["name", "alter"] {
            assert_eq!(
                store
                    .create_dataset_view(&anna, &path, "X", "board", &json!({"group_by": key}))
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid("group_by must be a select field of the dataset".into()),
                "{key}"
            );
        }
        // group_by is required: a board without one is not a board config.
        let out = store
            .create_dataset_view(
                &anna,
                &path,
                "X",
                "board",
                &json!({"card_fields": ["name"]}),
            )
            .await
            .unwrap();
        assert!(
            matches!(&out, SchemaOutcome::Invalid(m) if m.contains("not a valid board view config")),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn updating_a_board_re_validates_it_as_a_board() {
        let (store, anna, _, _, path) = fixture().await;
        let v = made_board(&store, &anna, &path).await;
        // The update path validates against the STORED kind: a text column cannot become
        // the lanes of an existing board, and a table-shaped config is not a board config.
        for cfg in [json!({"group_by": "name"}), json!({"fields": ["name"]})] {
            let out = store
                .update_dataset_view(&anna, &path, &v.id, None, Some(&cfg))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        let list = match store.dataset_views(&anna, &path).await.unwrap() {
            SchemaOutcome::Done(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(list[0].config["group_by"], "status");
        assert_eq!(list[0].kind, "board");
    }

    #[tokio::test]
    async fn a_board_naming_a_missing_field_a_hostile_key_or_an_unknown_member_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for key in ["gibtsnicht", "a') OR 1=1 --"] {
            let want = if key == "gibtsnicht" {
                format!("the dataset has no field {key:?}")
            } else {
                String::new()
            };
            for cfg in [
                json!({"group_by": key}),
                json!({"group_by": "status", "card_fields": [key]}),
                json!({"group_by": "status", "filters": [{"op": "is_empty", "key": key}]}),
                json!({"group_by": "status", "sort": {"key": key}}),
            ] {
                let out = store
                    .create_dataset_view(&anna, &path, "X", "board", &cfg)
                    .await
                    .unwrap();
                if want.is_empty() {
                    assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
                } else {
                    assert_eq!(out, SchemaOutcome::Invalid(want.clone()), "{cfg}");
                }
            }
        }
        for cfg in [
            json!({"group_by": "status", "bogerig": true}),
            json!({"fields": ["name"]}),
            json!([1]),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "board", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
    }

    #[tokio::test]
    async fn board_writes_need_write_and_a_stranger_gets_nodataset() {
        let (store, anna, bea, cleo, path) = fixture().await;
        made_board(&store, &anna, &path).await;
        let cfg = board();
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "board", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "board", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::NoDataset
        );
        assert_eq!(
            store.dataset_views(&cleo, &path).await.unwrap(),
            SchemaOutcome::NoDataset
        );
    }

    #[tokio::test]
    async fn kinds_outside_table_and_board_are_still_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for kind in ["gantt", "nonsense"] {
            assert_eq!(
                store
                    .create_dataset_view(&anna, &path, "X", kind, &json!({"group_by": "status"}))
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(format!("the view kind {kind:?} is not available")),
                "{kind}"
            );
        }
    }

    #[tokio::test]
    async fn listing_strips_hidden_card_fields_from_a_board_config() {
        let (_, _, _, _, _) = fixture().await;
        let mut cfg = board();
        let visible: HashSet<String> = ["name".to_string(), "status".to_string()].into();
        strip_hidden("board", &mut cfg, &visible);
        assert_eq!(cfg["card_fields"], json!(["name"]));
        assert!(
            cfg.get("sort").is_none(),
            "a sort on a hidden field is dropped: {cfg}"
        );
        assert_eq!(cfg["filters"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["group_by"], json!("status"));
    }

    // --- form views (M8c C5) ----------------------------------------------------------

    fn form() -> serde_json::Value {
        json!({"fields": [{"key": "name", "label": "Name", "required": true},
                          {"key": "alter", "required": false}],
               "title": "Anmelden"})
    }

    async fn made_form(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Formular", "form", &form())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_form_view() {
        let (store, anna, bea, cleo, path) = fixture().await;
        let v = made_form(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Formular", "form"));
        assert_eq!(v.config["fields"].as_array().unwrap().len(), 2);
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list[0].config["fields"][0]["label"], "Name");
        // The update path re-validates as a form: a board-shaped config is refused and
        // changes nothing.
        let bad = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                None,
                Some(&json!({"group_by": "status"})),
            )
            .await
            .unwrap();
        assert!(matches!(bad, SchemaOutcome::Invalid(_)), "{bad:?}");
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"fields": [{"key": "name"}]})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config, json!({"fields": [{"key": "name"}]}));
        // A reader may not save, a stranger learns nothing.
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "form", &form())
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "form", &form())
                .await
                .unwrap(),
            SchemaOutcome::NoDataset
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
    }

    #[tokio::test]
    async fn a_form_config_needs_the_right_fields_and_nothing_else() {
        let (store, anna, _, _, path) = fixture().await;
        for cfg in [
            // A form without a field is not a form.
            json!({"fields": []}),
            // The same key twice: one field cannot be asked for twice.
            json!({"fields": [{"key": "name"}, {"key": "name"}]}),
            // A field the dataset does not have.
            json!({"fields": [{"key": "gibtsnicht"}]}),
            // A hostile key never reaches the database: FieldKey refuses it.
            json!({"fields": [{"key": "a'; DROP TABLE dataset_row; --"}]}),
            // A label that is blank or long is a bad label, not a silent default.
            json!({"fields": [{"key": "name", "label": "  "}]}),
            json!({"fields": [{"key": "name", "label": "l".repeat(121)}]}),
            // An unknown member anywhere: in the config, and in a field entry.
            json!({"fields": [{"key": "name"}], "bogerig": true}),
            json!({"fields": [{"key": "name", "bogerig": true}]}),
            // A field entry whose key is missing.
            json!({"fields": [{"label": "Name"}]}),
            // Not an object per field at all.
            json!({"fields": ["name"]}),
            json!([1]),
            json!({"unknown": 1}),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "form", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        // An unknown field is named in the same words as everywhere else.
        assert_eq!(
            store
                .create_dataset_view(
                    &anna,
                    &path,
                    "X",
                    "form",
                    &json!({"fields": [{"key": "weg"}]})
                )
                .await
                .unwrap(),
            SchemaOutcome::Invalid("the dataset has no field \"weg\"".into())
        );
    }

    #[tokio::test]
    async fn a_form_holds_no_computed_field() {
        let (store, anna, bea, source) = hidden_view_fixture().await;
        for key in ["fr", "cnt"] {
            assert_eq!(
                store
                    .create_dataset_view(
                        &anna,
                        &source,
                        "X",
                        "form",
                        &json!({"fields": [{"key": key}]})
                    )
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid("computed fields cannot be on a form".into()),
                "{key}"
            );
        }
        // A relation is not computed and may be asked for; hidden to the caller it is absent.
        assert!(matches!(
            store
                .create_dataset_view(
                    &anna,
                    &source,
                    "X",
                    "form",
                    &json!({"fields": [{"key": "mit"}]})
                )
                .await
                .unwrap(),
            SchemaOutcome::Done(_)
        ));
        assert_eq!(
            store
                .create_dataset_view(
                    &bea,
                    &source,
                    "X",
                    "form",
                    &json!({"fields": [{"key": "mit"}]})
                )
                .await
                .unwrap(),
            SchemaOutcome::Invalid("the dataset has no field \"mit\"".into())
        );
    }

    #[tokio::test]
    async fn listing_strips_hidden_form_fields_from_the_listed_config() {
        let (store, anna, bea, source) = hidden_view_fixture().await;
        let cfg = json!({"fields": [{"key": "name"}, {"key": "mit"}, {"key": "alter"}]});
        let SchemaOutcome::Done(_) = store
            .create_dataset_view(&anna, &source, "Alle", "form", &cfg)
            .await
            .unwrap()
        else {
            panic!("anna names the fields she may read")
        };
        // The same hidden key in a table config, so both shapes of `fields` are checked here.
        let SchemaOutcome::Done(_) = store
            .create_dataset_view(
                &anna,
                &source,
                "T",
                "table",
                &json!({"fields": ["name", "mit"]}),
            )
            .await
            .unwrap()
        else {
            panic!("anna names the fields she may read")
        };
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &source).await.unwrap() else {
            panic!()
        };
        let of = |kind: &str| -> Vec<serde_json::Value> {
            let v = list.iter().find(|v| v.kind == kind).unwrap();
            v.config["fields"].as_array().unwrap().clone()
        };
        assert_eq!(
            of("form"),
            vec![json!({"key": "name"}), json!({"key": "alter"})],
            "the form entry whose key is hidden goes: {:?}",
            of("form")
        );
        assert_eq!(of("table"), vec![json!("name")]);
    }

    #[tokio::test]
    async fn strip_hidden_drops_form_entries_whose_key_is_hidden() {
        let mut cfg = json!({"fields": [{"key": "name"}, {"key": "mit"}], "title": "T"});
        let visible: HashSet<String> = ["name".to_string()].into();
        strip_hidden("form", &mut cfg, &visible);
        assert_eq!(cfg["fields"], json!([{"key": "name"}]));
        assert_eq!(cfg["title"], json!("T"));
    }

    // --- calendar views (M8d D3a) ----------------------------------------------------

    /// The date columns a calendar places rows by. `zeit` is a datetime: the same kind with
    /// `with_time`, and a calendar must accept it as readily as a plain date.
    async fn dated(store: &Store, who: &Principal, path: &str) {
        for (key, config) in [
            ("start", json!({})),
            ("ende", json!({})),
            ("zeit", json!({"with_time": true})),
        ] {
            store
                .add_dataset_field(
                    who,
                    path,
                    &NewField {
                        key: key.into(),
                        label: key.into(),
                        kind: FieldKind::Date,
                        config,
                    },
                )
                .await
                .unwrap();
        }
    }

    fn calendar() -> serde_json::Value {
        json!({"date": "start", "end": "ende", "title": "name",
               "filters": [{"op": "is_empty", "key": "start"}],
               "sort": {"key": "start", "desc": true}})
    }

    async fn made_calendar(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Kalender", "calendar", &calendar())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_calendar_view() {
        let (store, anna, bea, _, path) = fixture().await;
        dated(&store, &anna, &path).await;
        let v = made_calendar(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Kalender", "calendar"));
        assert_eq!(v.config["date"], json!("start"));
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["end"], json!("ende"));
        assert_eq!(list[0].config["title"], json!("name"));
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"date": "start"})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config, json!({"date": "start"}));
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
    }

    /// `date` is required and must name a date column; `end` must be one too, and a different
    /// field; `title` must be text. A datetime is the same kind, so `zeit` passes where `start`
    /// does.
    #[tokio::test]
    async fn a_calendar_places_only_date_fields() {
        let (store, anna, _, _, path) = fixture().await;
        dated(&store, &anna, &path).await;
        for (cfg, want) in [
            (
                json!({"date": "name"}),
                "date must be a date field of the dataset",
            ),
            (
                json!({"date": "status"}),
                "date must be a date field of the dataset",
            ),
            (
                json!({"date": "start", "end": "name"}),
                "end must be a date field of the dataset",
            ),
            (
                json!({"date": "start", "end": "start"}),
                "end must name another field than date",
            ),
            (
                json!({"date": "start", "title": "alter"}),
                "title must be a text field of the dataset",
            ),
        ] {
            assert_eq!(
                store
                    .create_dataset_view(&anna, &path, "X", "calendar", &cfg)
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(want.into()),
                "{cfg}"
            );
        }
        // date is required: a calendar without one is not a calendar config.
        let out = store
            .create_dataset_view(&anna, &path, "X", "calendar", &json!({"title": "name"}))
            .await
            .unwrap();
        assert!(
            matches!(&out, SchemaOutcome::Invalid(m) if m.contains("not a valid calendar view config")),
            "{out:?}"
        );
        // A datetime is a date field with `with_time`, and so places rows like any other.
        for key in ["start", "zeit"] {
            assert!(
                matches!(
                    store
                        .create_dataset_view(&anna, &path, "D", "calendar", &json!({"date": key}))
                        .await
                        .unwrap(),
                    SchemaOutcome::Done(_)
                ),
                "{key}"
            );
        }
    }

    #[tokio::test]
    async fn updating_a_calendar_re_validates_it_as_a_calendar() {
        let (store, anna, _, _, path) = fixture().await;
        dated(&store, &anna, &path).await;
        let v = made_calendar(&store, &anna, &path).await;
        // The update path validates against the STORED kind: a text column cannot become the
        // dates of an existing calendar, and a table- or board-shaped config is not a calendar.
        for cfg in [
            json!({"date": "name"}),
            json!({"date": "start", "end": "name"}),
            json!({"group_by": "status"}),
            json!({"fields": ["name"]}),
        ] {
            let out = store
                .update_dataset_view(&anna, &path, &v.id, None, Some(&cfg))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        let list = match store.dataset_views(&anna, &path).await.unwrap() {
            SchemaOutcome::Done(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(list[0].config["date"], "start");
        assert_eq!(list[0].kind, "calendar");
    }

    #[tokio::test]
    async fn a_calendar_naming_a_missing_field_a_hostile_key_or_an_unknown_member_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        dated(&store, &anna, &path).await;
        for key in ["gibtsnicht", "a') OR 1=1 --"] {
            let want = if key == "gibtsnicht" {
                format!("the dataset has no field {key:?}")
            } else {
                String::new()
            };
            for cfg in [
                json!({"date": key}),
                json!({"date": "start", "end": key}),
                json!({"date": "start", "title": key}),
                json!({"date": "start", "filters": [{"op": "is_empty", "key": key}]}),
                json!({"date": "start", "sort": {"key": key}}),
            ] {
                let out = store
                    .create_dataset_view(&anna, &path, "X", "calendar", &cfg)
                    .await
                    .unwrap();
                if want.is_empty() {
                    assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
                } else {
                    assert_eq!(out, SchemaOutcome::Invalid(want.clone()), "{cfg}");
                }
            }
        }
        for cfg in [
            json!({"date": "start", "bogerig": true}),
            json!({"fields": ["name"]}),
            json!([1]),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "calendar", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
    }

    #[tokio::test]
    async fn calendar_writes_need_write_and_a_stranger_gets_nodataset() {
        let (store, anna, bea, cleo, path) = fixture().await;
        dated(&store, &anna, &path).await;
        made_calendar(&store, &anna, &path).await;
        let cfg = calendar();
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "calendar", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .update_dataset_view(&bea, &path, "x", Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store.delete_dataset_view(&bea, &path, "x").await.unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "calendar", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::NoDataset
        );
        assert_eq!(
            store.dataset_views(&cleo, &path).await.unwrap(),
            SchemaOutcome::NoDataset
        );
    }

    /// A column the caller may not see is not a column their calendar may name, in the exact
    /// words of a key that does not exist.
    #[tokio::test]
    async fn a_calendar_naming_a_hidden_field_is_refused_as_an_unknown_one() {
        let (store, _, bea, source) = hidden_view_fixture().await;
        for key in ["mit", "cnt", "fr"] {
            assert_eq!(
                store
                    .create_dataset_view(&bea, &source, "X", "calendar", &json!({"date": key}))
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(format!("the dataset has no field {key:?}")),
                "{key}"
            );
        }
    }

    /// A date, an end, a title or a sort over a field the reader may not see is stripped from the
    /// listed config, and the calendar itself still lists — as a board's `group_by` is.
    #[tokio::test]
    async fn listing_strips_hidden_date_fields_from_a_calendar_config() {
        let (_, _, _, _, _) = fixture().await;
        let mut cfg = calendar();
        let visible: HashSet<String> = ["name".to_string(), "ende".to_string()].into();
        strip_hidden("calendar", &mut cfg, &visible);
        assert!(
            cfg.get("date").is_none(),
            "a calendar whose date is hidden loses it: {cfg}"
        );
        assert_eq!(cfg["end"], json!("ende"));
        assert_eq!(cfg["title"], json!("name"));
        assert!(
            cfg.get("sort").is_none(),
            "a sort on a hidden field is dropped: {cfg}"
        );
        assert!(
            cfg["filters"].as_array().unwrap().is_empty(),
            "the filter on the hidden date goes: {cfg}"
        );
    }

    /// `title` is a field key only in a calendar's config: a form's `title` is its heading, and
    /// stripping by that member must not reach into it.
    #[tokio::test]
    async fn strip_hidden_keeps_a_form_title_while_dropping_a_calendar_one() {
        let visible: HashSet<String> = ["name".to_string()].into();
        let mut form_cfg = json!({"fields": [{"key": "name"}], "title": "anmelden"});
        strip_hidden("form", &mut form_cfg, &visible);
        assert_eq!(form_cfg["title"], json!("anmelden"));
        let mut cal_cfg = json!({"date": "start", "title": "name"});
        strip_hidden("calendar", &mut cal_cfg, &visible);
        assert!(cal_cfg.get("date").is_none());
        assert_eq!(cal_cfg["title"], json!("name"));
    }

    /// Whether `title` names a field is a fact about the view's KIND, not about the shape of the
    /// JSON in front of the stripper: a heading that happens to read like a hidden field key
    /// stays, and a calendar or gallery loses its title with nothing else in the config to
    /// give it away — including a calendar whose own `date` has just been stripped.
    #[tokio::test]
    async fn strip_hidden_decides_a_title_by_the_kind_not_by_the_shape() {
        let visible: HashSet<String> = ["name".to_string()].into();

        // A form heading that is the name of a hidden field is still a heading.
        let mut form_cfg = json!({"fields": [{"key": "name"}], "title": "mit"});
        strip_hidden("form", &mut form_cfg, &visible);
        assert_eq!(
            form_cfg["title"],
            json!("mit"),
            "a form's title is its own string, never a field key: {form_cfg}"
        );

        // The shape that used to fool the heuristic: a form's config carrying a `date` member.
        // Nothing about that JSON says "form" any more than a calendar's does — the kind does.
        let mut sneaky = json!({"title": "mit", "date": "start"});
        strip_hidden("form", &mut sneaky, &visible);
        assert_eq!(sneaky["title"], json!("mit"), "{sneaky}");

        // A calendar and a gallery with no `date`/`cover` member left are still that kind.
        for kind in ["calendar", "gallery"] {
            let mut cfg = json!({"title": "mit"});
            strip_hidden(kind, &mut cfg, &visible);
            assert!(
                cfg.get("title").is_none(),
                "a {kind} whose title names a hidden field loses it: {cfg}"
            );
        }
    }

    /// A computed column is evaluated per row and is no date and no file: a rollup or a formula
    /// is refused as a calendar's `date`/`end` and as a gallery's `cover`, by the kind rule
    /// (D3a/D4a seat, LOW: the refusal had no test of its own).
    #[tokio::test]
    async fn a_computed_field_is_refused_as_a_calendar_date_or_a_gallery_cover() {
        let (store, anna, _, source) = hidden_view_fixture().await;
        for key in ["cnt", "fr"] {
            for (kind, cfg, want) in [
                (
                    "calendar",
                    json!({"date": key}),
                    "date must be a date field of the dataset",
                ),
                (
                    "gallery",
                    json!({"cover": key}),
                    "cover must be a file field of the dataset",
                ),
            ] {
                assert_eq!(
                    store
                        .create_dataset_view(&anna, &source, "X", kind, &cfg)
                        .await
                        .unwrap(),
                    SchemaOutcome::Invalid(want.into()),
                    "{kind} {cfg}"
                );
            }
        }
    }

    /// A calendar whose `date` is hidden loses both: the stripped date must not leave the
    /// hidden title behind as the config's only surviving key.
    #[tokio::test]
    async fn a_calendar_whose_date_is_hidden_still_loses_a_hidden_title() {
        let visible: HashSet<String> = ["ende".to_string()].into();
        let mut cfg = calendar();
        strip_hidden("calendar", &mut cfg, &visible);
        assert!(cfg.get("date").is_none(), "{cfg}");
        assert_eq!(cfg["end"], json!("ende"));
        assert!(
            cfg.get("title").is_none(),
            "the date going does not spare the title: {cfg}"
        );
    }

    // --- gallery views (M8d D4a) -----------------------------------------------------

    /// The file columns a gallery covers itself with. Which of them holds an image is decided at
    /// read time by the attachment's media type, not here.
    async fn filed(store: &Store, who: &Principal, path: &str) {
        for key in ["bild", "beleg"] {
            store
                .add_dataset_field(
                    who,
                    path,
                    &NewField {
                        key: key.into(),
                        label: key.into(),
                        kind: FieldKind::File,
                        config: json!({}),
                    },
                )
                .await
                .unwrap();
        }
    }

    fn gallery() -> serde_json::Value {
        json!({"cover": "bild", "title": "name", "card_fields": ["name", "alter"],
               "filters": [{"op": "is_empty", "key": "bild"}],
               "sort": {"key": "alter", "desc": true}})
    }

    async fn made_gallery(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Galerie", "gallery", &gallery())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_gallery_view() {
        let (store, anna, bea, _, path) = fixture().await;
        filed(&store, &anna, &path).await;
        let v = made_gallery(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Galerie", "gallery"));
        assert_eq!(v.config["cover"], json!("bild"));
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["card_fields"], json!(["name", "alter"]));
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"cover": "beleg"})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config, json!({"cover": "beleg"}));
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
    }

    /// `cover` is required and only ever a file column; `title` only ever a text one;
    /// `card_fields` may show any stored column, but a computed one is not a column a card holds.
    #[tokio::test]
    async fn a_gallery_covers_only_file_fields() {
        let (store, anna, _, _, path) = fixture().await;
        filed(&store, &anna, &path).await;
        for (cfg, want) in [
            (
                json!({"cover": "name"}),
                "cover must be a file field of the dataset",
            ),
            (
                json!({"cover": "status"}),
                "cover must be a file field of the dataset",
            ),
            (
                json!({"cover": "bild", "title": "alter"}),
                "title must be a text field of the dataset",
            ),
        ] {
            assert_eq!(
                store
                    .create_dataset_view(&anna, &path, "X", "gallery", &cfg)
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(want.into()),
                "{cfg}"
            );
        }
        // cover is required: a gallery without one is not a gallery config.
        let out = store
            .create_dataset_view(&anna, &path, "X", "gallery", &json!({"title": "name"}))
            .await
            .unwrap();
        assert!(
            matches!(&out, SchemaOutcome::Invalid(m) if m.contains("not a valid gallery view config")),
            "{out:?}"
        );
        // A computed column answers per row, so no card of it.
        let (hidden_store, anna2, _, source) = hidden_view_fixture().await;
        hidden_store
            .add_dataset_field(
                &anna2,
                &source,
                &NewField {
                    key: "bild".into(),
                    label: "Bild".into(),
                    kind: FieldKind::File,
                    config: json!({}),
                },
            )
            .await
            .unwrap();
        for key in ["fr", "cnt"] {
            assert_eq!(
                hidden_store
                    .create_dataset_view(
                        &anna2,
                        &source,
                        "X",
                        "gallery",
                        &json!({"cover": "bild", "card_fields": [key]})
                    )
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid("computed fields cannot be on a gallery".into()),
                "{key}"
            );
        }
        // A file column may be a card field too; the config does not forbid it.
        assert!(matches!(
            store
                .create_dataset_view(
                    &anna,
                    &path,
                    "X",
                    "gallery",
                    &json!({"cover": "bild", "card_fields": ["beleg"]})
                )
                .await
                .unwrap(),
            SchemaOutcome::Done(_)
        ));
    }

    #[tokio::test]
    async fn updating_a_gallery_re_validates_it_as_a_gallery() {
        let (store, anna, _, _, path) = fixture().await;
        filed(&store, &anna, &path).await;
        let v = made_gallery(&store, &anna, &path).await;
        // The update path validates against the STORED kind: a text column cannot become the
        // cover of an existing gallery, and a calendar- or table-shaped config is not a gallery.
        for cfg in [
            json!({"cover": "name"}),
            json!({"date": "name"}),
            json!({"group_by": "status"}),
            json!({"fields": ["name"]}),
        ] {
            let out = store
                .update_dataset_view(&anna, &path, &v.id, None, Some(&cfg))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        let list = match store.dataset_views(&anna, &path).await.unwrap() {
            SchemaOutcome::Done(l) => l,
            other => panic!("{other:?}"),
        };
        assert_eq!(list[0].config["cover"], "bild");
        assert_eq!(list[0].kind, "gallery");
    }

    #[tokio::test]
    async fn a_gallery_naming_a_missing_field_a_hostile_key_or_an_unknown_member_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        filed(&store, &anna, &path).await;
        for key in ["gibtsnicht", "a') OR 1=1 --"] {
            let want = if key == "gibtsnicht" {
                format!("the dataset has no field {key:?}")
            } else {
                String::new()
            };
            for cfg in [
                json!({"cover": key}),
                json!({"cover": "bild", "title": key}),
                json!({"cover": "bild", "card_fields": [key]}),
                json!({"cover": "bild", "filters": [{"op": "is_empty", "key": key}]}),
                json!({"cover": "bild", "sort": {"key": key}}),
            ] {
                let out = store
                    .create_dataset_view(&anna, &path, "X", "gallery", &cfg)
                    .await
                    .unwrap();
                if want.is_empty() {
                    assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
                } else {
                    assert_eq!(out, SchemaOutcome::Invalid(want.clone()), "{cfg}");
                }
            }
        }
        for cfg in [
            json!({"cover": "bild", "bogerig": true}),
            json!({"fields": ["name"]}),
            json!([1]),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "gallery", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
    }

    #[tokio::test]
    async fn gallery_writes_need_write_and_a_stranger_gets_nodataset() {
        let (store, anna, bea, cleo, path) = fixture().await;
        filed(&store, &anna, &path).await;
        made_gallery(&store, &anna, &path).await;
        let cfg = gallery();
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "gallery", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .update_dataset_view(&bea, &path, "x", Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store.delete_dataset_view(&bea, &path, "x").await.unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "gallery", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::NoDataset
        );
        assert_eq!(
            store.dataset_views(&cleo, &path).await.unwrap(),
            SchemaOutcome::NoDataset
        );
    }

    #[tokio::test]
    async fn a_gallery_naming_a_hidden_field_is_refused_as_an_unknown_one() {
        let (store, anna, bea, source) = hidden_view_fixture().await;
        store
            .add_dataset_field(
                &anna,
                &source,
                &NewField {
                    key: "bild".into(),
                    label: "Bild".into(),
                    kind: FieldKind::File,
                    config: json!({}),
                },
            )
            .await
            .unwrap();
        for key in ["mit", "cnt", "fr"] {
            assert_eq!(
                store
                    .create_dataset_view(&bea, &source, "X", "gallery", &json!({"cover": key}))
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(format!("the dataset has no field {key:?}")),
                "{key}"
            );
            assert_eq!(
                store
                    .create_dataset_view(
                        &bea,
                        &source,
                        "X",
                        "gallery",
                        &json!({"cover": "bild", "card_fields": [key]})
                    )
                    .await
                    .unwrap(),
                SchemaOutcome::Invalid(format!("the dataset has no field {key:?}")),
                "{key} as a card field"
            );
        }
    }

    /// A cover, a title, a card field or a sort over a column the reader may not see is stripped
    /// from the listed config, and the gallery itself still lists — as a board's `group_by` is.
    #[tokio::test]
    async fn listing_strips_hidden_cover_and_card_fields_from_a_gallery_config() {
        let (_, _, _, _, _) = fixture().await;
        let mut cfg = gallery();
        let visible: HashSet<String> = ["name".to_string(), "alter".to_string()].into();
        strip_hidden("gallery", &mut cfg, &visible);
        assert!(
            cfg.get("cover").is_none(),
            "a gallery whose cover is hidden loses it: {cfg}"
        );
        assert_eq!(cfg["title"], json!("name"));
        assert_eq!(cfg["card_fields"], json!(["name", "alter"]));
        assert_eq!(
            cfg["sort"]["key"],
            json!("alter"),
            "a sort on a field the reader may see stays: {cfg}"
        );
        assert!(
            cfg["filters"].as_array().unwrap().is_empty(),
            "the filter on the hidden cover goes: {cfg}"
        );
        // The same config for a reader who may not see `alter`: its card field and its sort go.
        let mut strict = gallery();
        let visible: HashSet<String> = ["name".to_string(), "bild".to_string()].into();
        strip_hidden("gallery", &mut strict, &visible);
        assert_eq!(strict["cover"], json!("bild"));
        assert!(
            strict.get("sort").is_none(),
            "a sort on a hidden field is dropped: {strict}"
        );
        assert_eq!(strict["card_fields"], json!(["name"]));
    }

    /// A gallery's `title` is a field key and goes with the hidden set; a form's `title` is its
    /// own heading and stays, whatever the visible set holds.
    #[tokio::test]
    async fn strip_hidden_drops_a_gallery_title_but_keeps_a_form_one() {
        let visible: HashSet<String> = ["name".to_string()].into();
        let mut gal_cfg = json!({"cover": "bild", "title": "mit"});
        strip_hidden("gallery", &mut gal_cfg, &visible);
        assert!(gal_cfg.get("cover").is_none());
        assert!(
            gal_cfg.get("title").is_none(),
            "a title naming a hidden field goes: {gal_cfg}"
        );
        let mut form_cfg = json!({"fields": [{"key": "name"}], "title": "anmelden"});
        strip_hidden("form", &mut form_cfg, &visible);
        assert_eq!(form_cfg["title"], json!("anmelden"));
    }
}
