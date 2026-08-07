# 0004 — Build graph storage in SQLite; keep Omnigraph as agent memory

**Status:** Accepted (2026-08-07)

## Context

This estate already runs Omnigraph, a structured graph server with a typed schema language,
branches, vector search and an HTTP API. great-wiki needs a knowledge graph. The obvious
question is whether to use the graph server that already exists rather than building graph
storage inside the application.

The live deployment was measured rather than assumed:

- **Scale.** All five graphs together hold **621 nodes and 1080 edges**. For comparison,
  Graphify produces 2720 nodes for a *single* repository in 2.4 seconds. Omnigraph has never
  been exercised near the scale a wiki would reach, and there is no benchmark to lean on.
- **Semantic search is currently dead cluster-wide.** 253 of 256 `Decision` nodes have a
  null embedding; a live `nearest()` query returns zero rows. Four unfixed defects prevent
  (re-)embedding a populated graph on v0.8.1 — notably that `load --mode merge` *erases* an
  existing vector rather than leaving it stale, so editing a record silently un-indexes it.
- **Edges have no key.** A retried edge insert duplicates, no API deletes an individual
  edge, and an hourly cron job exists specifically to remove duplicates.
- **Schema changes are a cluster-wide outage.** `apply-cluster.sh` stops the server to
  release the state lock, so every graph is unavailable during any one graph's migration.
  It also refuses to run while any non-`main` branch exists.
- **`.gq` has no variable-length traversal.** Multi-hop must be written out hop by hop.
- **`.gq` has no row-level security.**

## Decision

great-wiki stores its knowledge graph in its own SQLite tables and computes over it in
process. Omnigraph remains what its own documentation says it is: agent memory.

The two decisive points are not performance:

**Availability.** `structured-memory` states that Omnigraph has no fallback and that
"memory is an accelerator, not a correctness dependency: a session without it is slower,
not wrong." That is the correct framing for memory and the wrong one for a product's
primary read path. A wiki whose pages stop rendering because an unrelated graph is being
migrated is not acceptable.

**Permission filtering.** The governing security invariant is that every retrieval path
filters by the caller's permissions inside the retriever. In SQLite that can be made
*structural* — a permission-joined view, a query builder that cannot emit an unfiltered
read. In `.gq` it could only ever be a convention that every generated query remembers to
follow, and the failure mode is silent disclosure.

SQLite additionally supplies what `.gq` lacks: `WITH RECURSIVE` for variable-length
traversal, FTS5 in the same store, and vector scanning alongside. At wiki scale, backlinks,
orphan detection, tag co-occurrence, shortest path, community detection and PageRank are
all millisecond operations over an in-memory graph built from one recursive query.

**Omnigraph keeps two roles.** It holds this project's memory as the skill prescribes. And
it optionally receives a *one-way, best-effort* projection of the document graph
(`load`, `mode: merge`) so great-wiki's structure is queryable alongside the other
repositories' graphs. Export, never store: if it is unavailable, the wiki does not notice.

## Consequences

- Graph algorithms are implemented in `gw-graph` rather than delegated. This is a small
  amount of well-understood code, not a research problem.
- The wiki has no runtime dependency on Omnigraph. It renders with the graph server down.
- The projection is additive and can be dropped without affecting the product.
- Four design ideas are adopted from Omnigraph rather than its implementation: typed nodes
  and typed edges with no generic `relates-to`; key-based idempotent upsert; the lint that a
  node whose only edge is to its hub is under-linked; and the viewer's server-side-token
  proxy pattern, where the browser never holds a credential.

## Switch-back criteria

Reconsider if cross-repository graph federation becomes a primary requirement rather than a
convenience, *and* the embedding defects are fixed upstream, *and* schema application stops
requiring a cluster-wide stop. All three would need to be true.
