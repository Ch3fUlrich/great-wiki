# M8 — Datasets & views: design

Roadmap M8; design D14 (content type `dataset`), D19 (timeline), §3.2 block `dataset-view`,
§3.3 `gw-store` "dataset queries". Decisions: [ADR 0029](../../decisions/0029-datasets-are-pages-and-rows-inherit-their-access.md).
Plan: [m8-datasets](../plans/2026-10-10-m8-datasets.md). **Docs only; nothing here is built.**

## 1. Model

```
documents (content type 'dataset')            -- the dataset IS this page
  └ dataset_field(id, doc_id, key, label, kind, config JSON, position, required)
  └ dataset_row(id, doc_id, values JSON, position, created_by, created_at, updated_at, version)
  └ dataset_view(id, doc_id, kind, name, config JSON, position)
  └ dataset_relation(row_id, field_id, target_row_id)   -- reverse index for relations
```

Migrations (next free is 0020): `0020_datasets` (field, row), `0021_dataset_views`,
`0022_dataset_relations`. The `documents` content-type column is checked first; if `dataset`
is not an allowed value, 0020 adds it.

### Field kinds
`text number bool date select multi_select tags url person file relation rollup formula`.

| kind | stored in `values[key]` | notes |
|---|---|---|
| text, url | string | url: scheme allow-list `http https mailto`; never `javascript:`/`data:` |
| number | JSON number (finite) | NaN/Inf refused |
| bool | true/false | |
| date | `YYYY-MM-DD` or RFC 3339 | `config.with_time` |
| select / multi_select | option id(s) | options live in `config.options[{id,label,color}]`; rename ≠ rewrite rows |
| tags | array of tag ids | reuses the existing tag table (0011), not new strings |
| person | principal id(s) | must be able to read the dataset page (ADR 0009 clause 2) |
| file | attachment id | an attachment of the dataset page (0013); same blob rules |
| relation | row ids of `config.target` | stored in values **and** `dataset_relation` |
| rollup | not stored | `{relation, target_field, fn}` ; fn ∈ count,sum,min,max,avg,list |
| formula | not stored | expression string, parsed at schema save |

Computed kinds (rollup, formula) are evaluated at read, never persisted — a persisted rollup
is a copy that outlives the target's access change.

### Keys, limits
- Key `^[a-z][a-z0-9_]*$`, ≤ 48 chars, unique per dataset, immutable after creation (rename =
  label only). Reserved: `id`, `created_at`, `updated_at`, `version`.
- ≤ 100 fields, ≤ 50 000 rows, ≤ 64 KiB per row JSON, ≤ 200 rows/page (keyset cursor on
  `(position,id)`), ≤ 50 options per select.

### Typed validation
One `gw_core::dataset::validate_value(kind, config, json) -> Result<Value, FieldError>`; the
store calls it on every write, the importer later reuses it. Unknown keys in a row are
refused, not stored.

## 2. Access (the part to mutation-test)

- `readable_dataset(caller, doc)` → page read permission (the existing accessor). Everything —
  rows, counts, view lists, relation targets, rollups, the `dataset-view` block endpoint,
  events, search — passes through it.
- Row create/update/delete, schema change, view save: write on the page. Reading a dataset
  you cannot read answers "there is no dataset at …" (ADR 0022), not "forbidden".
- Relation write: caller reads source **and** target dataset; target row must belong to
  `config.target`. Cross-dataset targets the caller cannot read are never resolved, counted or
  named — field omitted in schema and row output for that caller.
- Rollup/formula over a relation aggregates only visible rows.
- **Person fields:** setting a person who cannot read the page is refused (ADR 0009 cl. 2);
  unassign/clear is always allowed with write.
- Views are saved config only. Filtering/sorting hidden fields is refused (hidden = omitted
  by access, so the key is unknown to that caller).
- Optimistic concurrency: row `version`; stale write → 409 with the current row (readable by
  definition).

## 3. Query layer (`gw-store::datasets`)

`query(caller, doc, ViewQuery{filters, sorts, cursor, limit, group_by})`. Filters/sorts name
fields by key → parsed to `FieldKey` first; SQL uses `json_extract(values, ?)` with the path
`$.<key>` built from the validated type and passed **as a bound parameter**; typed casts per
kind (`CAST(... AS REAL)` for number/date). Operators per kind are a closed enum
(eq, ne, lt/gt/le/ge, contains, is_empty, in). Unvalidated key reaching SQL = compile error
(no `&str` constructor on the query-builder input).

## 4. Formula (`gw_core::formula`)

Pure: lexer → parser → evaluator over `Value`. Literals, field refs (`{key}`), `+ - * / %`,
comparisons, `and or not`, `if(c,a,b)`, `concat`, `round`, `min`, `max`, `abs`, `len`,
`today()` (injected clock), `coalesce`. No loops, no recursion between formulas (dependency
graph checked at schema save, cycle → refused), node budget 256, eval step budget 10 000, depth 32.
Errors evaluate to a typed `#ERR(reason)` cell, never a panic or a 500. Output is a value,
rendered as text — never markup (the lesson of ADR 0017).

## 5. Views

All views are one component family over `GET /api/datasets/{doc}/rows?view=…`.

| view | milestone | config |
|---|---|---|
| table | M8a | visible fields, order, sort, filters, column widths |
| board | M8c | `group_by` select field; drag = row update (needs write) |
| form | M8c | field subset, order, labels; create-only; honours `required`; same validator |
| calendar | M8d | one date field (+ optional end) |
| gallery | M8d | cover = file field (image attachments only), title field |
| timeline | M8e | date/date-range field, grouping field (D19 view 3 only) |

The existing generic markdown `TableView.svelte` (sort/filter on page tables) is left alone;
the dataset table is a new component and may later share its sort helpers.

**Board absorbs the task board:** one `Board.svelte` over `RowSource`; the tasks page binds
`TaskRowSource` (ADR 0029 §7), the dataset view binds `DatasetRowSource`. Same component, two
sources — not two boards (D-12's "one query with a filter" rule).

## 6. Embedding

`dataset-view` block `{dataset_doc, view_id}`: new `BlockKind`, with the five-layer floor
(see ADR 0029 §8 and the tasks plan's table). Rendered through the endpoint; the embedding
page's permissions never grant the dataset's.

## 7. Events, search, history

Row/schema/view writes emit `dataset.*` events naming the page (ADR 0025). Schema changes also
write an audit line. Page search indexes title + field labels (ADR 0024); row-text indexing is
deferred. No per-row revision history in M8 (audit + events only).

## 8. Out of scope
Per-row ACL, row comments, import recipes (`ingestion_recipe`), CSV import/export, charts
(M9), D19 views 1–2, per-view permissions, formulas across datasets, persisted rollups.

## 9. Risks
- Aggregate leak (top): mitigated by single accessor + mutation entries (plan T-A7, T-B6).
- JSON-column filter cost: row cap + cursor; revisit with generated columns if a view is slow.
- Block-kind floor: a missed layer silently deletes embeds in the editor — plan T-D1 has all
  five layers as one task with the CRDT fixture first.
- Schema edits vs existing rows (change kind): kind is immutable; "convert" = new field.
