# 0003 — SQLite FTS5 for full-text search, behind a swappable trait

**Status:** Accepted (2026-08-07)

## Context

The predecessor plan chose Tantivy, justified as handling *"millions of documents, BM25
ranking and faceting, and keeping the index out of the transactional DB so a reindex never
blocks reads."* Examined against the actual requirements, each clause is weak:

- **Millions of documents.** The corpus is in the hundreds. That is three orders of
  magnitude of headroom bought before there is any evidence it is needed.
- **BM25.** FTS5 has `bm25()` with per-column weights — more than the plan used, which
  never boosted title over body.
- **Faceting.** Cited as a reason and then never used; the plan's group filtering was a
  hand-rolled post-filter instead.
- **A separate index.** Presented as an advantage, but it was the direct source of three
  defects in that plan: an exclusive writer lock that made its own scheduled reindex job
  unrunnable, a schema-migration trap that would refuse to start after an upgrade, and a
  security filter that failed *open* because the reindex query never selected the column it
  filtered on.

Meanwhile the security model requires that visibility and group filtering happen inside the
retriever, in the same query as the content — the single most important property for a
corpus containing server runbooks.

## Decision

Use SQLite FTS5, in the same database as the documents, behind a `SearchIndex` trait.

FTS5 gives `snippet()` and `highlight()` for result excerpts, per-column BM25 weights so
titles can outrank body text, and `unicode61 remove_diacritics=2`, which folds German
umlauts — relevant for a mixed German/English corpus that Tantivy's default English
tokenizer would handle poorly. Most importantly, the permission filter becomes an ordinary
`WHERE` clause in the same transaction as the visibility column, rather than a
security-critical post-filter over a separate index that can silently drift.

The trait boundary keeps an external engine a swap rather than a rewrite.

## Consequences

- One store to back up, one to migrate, one writer lock that does not exist.
- Semantic search is a separate concern: chunk embeddings are stored alongside and scanned
  in process. At this corpus size a vector database would be pure operational overhead.
- If the corpus grows by orders of magnitude, or ranking quality demands features FTS5
  lacks, implement the trait against Tantivy or an external engine. The interface is
  designed for that; nothing above it changes.

## Switch-back criteria

Revisit when full-text queries exceed roughly 200 ms at p95, or when a required ranking
feature has no FTS5 equivalent — whichever comes first, measured rather than assumed.
