# 0025 — What a comment discloses

**Status:** accepted · **Date:** 2026-10-08 · Builds on ADR 0011 and ADR 0022

## Decision

A comment is governed by exactly one page, its `doc_id`, like a task (ADR 0009/0010).

- **Anyone who may read the page may read and write comments on it.** Read is the only
  check; there is no comment-specific permission.
- **Every access goes through `Store::document_for_id(…, Read)`** — lists, counts, thread
  fetch, resolve. No count is computed in SQL across pages. A page the caller may not read
  yields the same answer as a page that does not exist: 404 via `withheld_or_absent`, same
  bytes (ADR 0022); `withheld.rs` sweeps the comment routes.
- **Comment ids are uuids; nothing is keyed by sequence number**, so there is no id gap to
  count by. Responses carry no total beyond the returned rows.
- **Author names** are resolved only for a reader who may read the page (as assignee names).
- **Mentions** (`@username`) emit a bus event (ADR 0024) to the named principal; the mention
  grants nothing — a mentioned person without read access never receives it (delivery check).
- **Anchored comments** store a Yjs relative-position pair plus a short quoted snippet.
  The snippet is page text: it is returned only to readers of the page, and never placed in
  an event, a notification row or the digest.
- **Orphaned, never deleted.** When the anchored passage is gone the comment is flagged
  `orphaned` and shown in the page thread; text is kept. **Resolved, never deleted:** anyone
  in the thread resolves it; it is collapsed and kept (history). No author delete exists —
  no DELETE route, no store function; `comments:` mutation entries pin that.
- Purging a page cascades its comments (like revisions); trashing keeps them.

## Rejected

Writers-only comments (a reader who spots an error cannot say so); a comment count on tree
or search results (a per-page aggregate outside the accessor); storing anchor as absolute
offsets (does not follow edits).
