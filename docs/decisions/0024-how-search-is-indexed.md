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

## Decision 5 — the API: one store method, no decision in the handler

`GET /api/search?q=` calls `Store::search_for(principal, q, limit)` and serialises what it
returns; the handler makes no permission decision (rule 2, and the reason `topics.rs` and
`links.rs` are written the same way).

- **Pages.** Every matching candidate is fetched (ceiling 1000) and each is put through
  `document_for_id_with_baseline(principal, id, Read, baseline)`, the baseline resolved once
  for the caller. A `None` is dropped without trace. The visible hits are then ranked in
  Rust and cut to `limit` (see "Ranking" below). A hit is the accessor's title and current path, and a snippet cut from the accessor's
  own body as `[{text, hit}]` segments around the first matching word — no HTML, so the web
  needs no sink. The index row's snippet is never used.
- **Topics and tasks** come from `topics_for` and `board_for(principal, None)`, which already
  filter by the same rule, and are matched in memory on folded words (lowercase, diacritics
  removed, as the tokenizer does; every query word must prefix some word of the name or
  title). A task hit carries the page path only if the board gave the card one.
- **No counts.** No total, no "n hidden", no page offset. A key that cannot exist cannot be
  wrong later, so a test asserts the key set. `TopicHit.documents` is the one number and is
  the length of the list that topic would show *this* caller.
- **Unsearchable is empty, not an error.** Blank, over 200 characters, or without a word:
  `200` with three empty lists, the value a query that matched nothing returns. A query that
  matches only a withheld page is byte-identical to that, status, headers and body; the query
  is never echoed. The raw query string is parsed leniently, so a bad escape or a repeated
  `q` is the same empty answer rather than an extractor's 400.

**Ranking, and a deliberate deviation from "BM25 ordering".** FTS5's `bm25()` weighs a word
by how rare it is across the whole index, withheld pages included. An order that followed it
would be a disclosure oracle: put two readable pages that differ in which word they repeat
side by side, add withheld pages containing one of the words, and the order flips — the
caller has measured pages they may not read. So the index's rank is used only to order
*candidates* (which matters only past the ceiling below), and never reaches the visible
order. That order is a score computed in Rust from the page the accessor returned and the
query alone: ten for each query word that starts a word of the title, plus one for each word
of the text that starts with a query word; ties go by path. [ADR 0003](0003-sqlite-fts5-behind-a-search-trait.md)
keeps bm25 as what the `SearchIndex` trait offers; this decision is about what a response
may be ordered by. **Revisit** if a ranking feature is wanted that needs corpus statistics:
it would have to compute them over the pages the caller may read, per request, or be shown to
add no information (for example, statistics over public pages only).

**Cost.** The permission check follows the index, one accessor call per candidate, up to a
ceiling of 1000. Below it nothing is hidden by a window: a readable page is found however
many withheld pages match, and however they would have ranked. A word on more than a thousand
pages keeps the first thousand by the index's order, so a caller who may read little can get
fewer hits than exist; that fails by under-reporting and says nothing about how many were
dropped. Putting the check inside the SQL would remove the cost and duplicate the permission
rule in a second language; rejected for the reason Decision 2 gives. Topic and task matching
is a scan over what the caller may see, fine for hundreds, to be revisited if a board or
topic list grows to tens of thousands.

**Timing is out of scope**, as it is for [ADR 0022](0022-what-a-refusal-may-say-about-a-page-it-refuses.md): a query
that matches only withheld pages does more work (one accessor call per candidate) than one
that matches nothing, and a caller with a clock can tell. Closing that would mean spending the
same work on every query, which is not worth doing for a wiki this size.

**Query handling.** The query is put in composed form (NFC) once, before both the MATCH
expression and the in-memory matching, so the two cannot disagree about where words are.
Characters Rust calls alphanumeric and `unicode61` calls separators (circled and squared
letters) are not word characters, and a word cut for length keeps its prefix star wherever
it stands. The index is rebuilt at every `Store::open`, not only when rows were filled:
it joins `documents` on an implicit rowid that a `VACUUM` may renumber.
