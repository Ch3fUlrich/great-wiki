# 0024 — How search is indexed

**Status:** Accepted (2026-10-08)

## Context

[ADR 0003](0003-sqlite-fts5-behind-a-search-trait.md) chose SQLite FTS5 behind a swappable
trait. It did not say how the index is kept equal to the pages, what it remembers, or what
reaches the engine's query parser. Those are where a search index goes wrong: it drifts, it
goes stale on the one fact that matters (who may see this), or it executes what a visitor
typed. [ADR 0012](0012-what-a-purge-destroys.md) adds a constraint — a purge destroys, and an
index that keeps a destroyed page's words has not complied.

## Decision 1 — an external-content table, kept current by triggers

`documents.body_text` holds the plain text of the block tree, produced by the one flattener
(`gw_core::body_plain_text`, over `Block::plain_text`) and written in the same statement as
`body` — in `create_document` and `append_revision`, the only two places a body is written.
`search_pages` is an FTS5 table with `content='documents'` over `(title, body_text)` and
`tokenize='unicode61 remove_diacritics 2'`, maintained by `AFTER INSERT`, `AFTER UPDATE OF
title, body_text` and `AFTER DELETE` triggers.

- The text exists once; the index holds tokens.
- Triggers rather than writes in Rust: the writer that forgets leaves a page findable by words
  it no longer has. A purge is a plain `DELETE FROM documents`, so the delete trigger is what
  makes ADR 0012 true of the index; a test asserts it on the FTS shadow tables, not on a query
  that happens to filter.
- The migration indexes existing rows by title (`rebuild`) before any trigger can fire,
  because an external-content `delete` of values that were never indexed corrupts the index
  ("database disk image is malformed" — found by test). `Store::open` then fills `body_text`
  for rows still NULL and rebuilds again; idempotent.

## Decision 2 — the index holds no path and no state

A hit is a rowid. The path, `deleted_at` and, later, permission are read from `documents` at
query time. A move or rename therefore needs no index write beyond the title trigger,
trash/restore is a `deleted_at IS NULL` condition, and there is no permission fact in the index
to go stale. **Rejected:** storing path/state in the index for a faster single-table query —
two copies of a fact that changes under moves and trashing, for a corpus of hundreds of pages.

## Decision 3 — candidates are unfiltered and crate-private

`SearchIndex::candidates` returns `(doc_id, rank, snippet)` and is `pub(crate)`, as are the
trait and `Fts5Index`. The only caller will be a `Store` method that passes every candidate
through `document_for_id` (rule 2); nothing outside `gw-store` can hold an unfiltered hit. The
snippet is from the index's copy of the text and must never be shown — the response takes its
excerpt from the document the accessor returned.

## Decision 4 — user text reaches MATCH only as quoted words

The query is cut into runs of alphanumeric characters (at most 12 words of 64 characters),
each emitted as an FTS5 string `"w"`, the last with a prefix star, joined by `AND`. Operators,
`NEAR(`, column filters and quotes in the input become words to look for. A query with no word
is an empty answer, indistinguishable from no match. `match_expression` is a pure function with
unit tests on hostile strings. **Rejected:** escaping FTS5 syntax — an escaper has to be
right about a grammar it does not own; quoting every word sidesteps the grammar.

## Cost

- **A body written by a path that skips `body_text` is invisible to search** (found by title
  only) until the next start backfills it. Today there is one such path to worry about: none.
  Anything that ever writes `documents.body` without `append_revision` must write `body_text`.
- `content_rowid` is `documents`' implicit rowid, which a `VACUUM` may renumber (the primary
  key is TEXT). Nothing vacuums; if it ever does, `rebuild` repairs it.
- A body that cannot be parsed is indexed as empty text rather than failing the write.

## Switch-back criteria

As ADR 0003: full-text queries beyond roughly 200 ms at p95, or a required ranking feature
FTS5 lacks, measured rather than assumed. Additionally, if a second writer of `documents.body`
appears, make `body_text` a generated column or route it through `append_revision` before
accepting the second path.
