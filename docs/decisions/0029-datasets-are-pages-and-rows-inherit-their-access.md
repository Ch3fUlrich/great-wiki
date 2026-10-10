# 0029 — Datasets are pages, and rows inherit their page's access

**Status:** Draft (2026-10-10; defaults taken by the planning lane, owner away — see Open
questions). Spec: [datasets and views](../superpowers/specs/2026-10-10-datasets-views-design.md).

## Context

M8 adds typed datasets and six views (D14, D19). Every aggregate view is a disclosure surface
(ADR 0009, 0011, 0026): a row, a count, a relation target or a rollup total can reveal that a
page exists. The choices below fix *where the access decision lives* so no view can invent
its own.

## Decision

1. **A dataset is a page** (content type `dataset`). History, permissions, search and links
   come free. **Rows inherit the dataset page's access; there is no per-row ACL in M8.**
   Read the page → read every row. Write the page → write rows and schema. Cost: a table
   mixing audiences must be split into two datasets. Revisit if that bites.
2. **Field keys match `^[a-z][a-z0-9_]*$`, checked at parse time** by one function in
   `gw-core` (`FieldKey::parse`). SQL never receives a key except as a bound parameter or
   after passing that type; there is no string-built identifier anywhere.
3. **Storage: `dataset_field` rows (typed metadata) + `dataset_row.values` as one JSON
   object per row, typed per field.** No per-dataset SQL tables: no DDL at runtime, one
   migration set, one backup shape. Cost: filters/sorts go through `json_extract` with a
   cast, so no index per field; bounded by a row cap (50 000) and keyset pagination.
4. **Formula: a new pure evaluator in `gw-core`, not a reuse of ADR 0017.** *The brief's
   default assumed 0017 contains an evaluator. It does not:* 0017 is KaTeX typesetting of
   ` ```math ` fences. The dataset formula is therefore new code, and 0017's *discipline* is
   what is reused: pure, no I/O, no host calls, bounded work (node and step budget), an error
   value instead of a panic, never rendered as markup.
5. **Relation and rollup reach only datasets the caller may read.** A relation field names a
   target dataset; at read time the target is resolved through the same permission-checked
   accessor a page read uses. Unreadable target → the field is **omitted** from the row and
   the schema view for that caller, and any *write* naming it is refused with the uniform
   "there is no dataset at …" of ADR 0022. A rollup never aggregates over rows the caller
   cannot read; an empty-because-hidden rollup is indistinguishable from absent.
6. **Views: table first, then board (absorbing the task board), form, calendar, gallery,
   timeline.** A view is saved config (`dataset_view`: kind + JSON), never data; it cannot
   widen what its dataset exposes. Table + core field types ship alone as **M8a**.
7. **Tasks: adapt first, migrate later.** `tasks`/`projects` (migration 0010) stay as the
   store of record: a task is anchored to a *block* and reconciled on publish (D-2), detached
   not deleted (D-8), and assignment is governed by ADR 0009 — none of which a free-form row
   has. The generic board view reads a `RowSource` trait; a `TaskRowSource` adapter presents
   tasks through the canonical schema (title, status, assignee, due, project). Migration to
   real rows is M8e and optional (Open question 1).
8. **A `dataset-view` block** (embed a view in a page) adds a `BlockKind`. That is the
   five-layer floor of the tasks plan: `gw-core` Block, export/import, `render.ts`/
   `BlockView.svelte`, `SERVER_BLOCK_KINDS` + `extensions.test.ts`, and
   `fixtures.rs::one_per_kind`. The block carries `{dataset_doc, view}` only — never rows;
   the reader fetches through the permission-checked endpoint.

## Consequences

- Every list/count/rollup path funnels through one `readable_dataset(caller, doc)` accessor;
  `just mutate` pins it (disable the check → a test must fail).
- Row writes emit events (ADR 0025) naming the dataset page; delivery re-asks permission.
- Search indexes the dataset *page* (title, field names) in M8; row text is deferred (Open
  question 3) because each indexed row is a copy that must follow access changes (ADR 0024).

## Open questions (default marked *)

1. Tasks: *adapt now, migrate in M8e / migrate in M8c / never migrate.*
2. Row history: *audit + events only, no per-row revisions / full row revisions.*
3. Row text in search: *defer / index with page ACL.*
4. D19's other two timelines (per-page idea development, corpus activity): *outside M8,
   own milestone / fold into M8e.*
