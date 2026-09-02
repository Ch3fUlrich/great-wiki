# great-wiki — Implementation Roadmap

> **For agentic workers:** this file is the map, not a plan. Each milestone has (or will
> have) its own plan file. Use `superpowers:subagent-driven-development` or
> `superpowers:executing-plans` against a *milestone plan*, never against this file.

**Goal:** Build a self-hosted collaborative knowledge platform, milestone by milestone,
where each milestone ends in working software rather than a layer.

**Spec:** [`../specs/2026-08-07-great-wiki-design.md`](../specs/2026-08-07-great-wiki-design.md)

## Global constraints

These bind every task in every milestone. They are not repeated per plan.

- **Rust edition 2021**, toolchain pinned in `rust-toolchain.toml`. No nightly features.
- **The database is the source of truth.** Markdown is import/export. Never add a write path
  that bypasses the revision system.
- **Every retrieval path filters by the caller's permissions at query time, inside the
  retriever** — search, RAG, graph, feeds, API, share links, exports, analytics. Never a
  post-filter.
- **Fail closed.** Unset secret → refuse to start. Unknown permission → deny. Missing
  visibility → private.
- **This repository is PUBLIC.** No real credential in a tracked file. Only `.env.example`.
- **LLM calls go to LiteLLM** (`http://192.168.178.159:4000/v1`, `deepseek-v4-flash`), never
  a provider SDK, never Ollama as primary. Embeddings use `nomic-embed-text` (768-dim) at
  `cloud.vm:11434`.
- **Storage split:** database, index and vectors on NVMe; blobs on `/mnt/cloud`. Never a
  database or object store on NFS.
- **Deployment binds `0.0.0.0`** — Caddy runs on a different host, so loopback is
  unreachable from it. **Never port 8090** (`omnigraph-viewer`). Proxy-only access is
  enforced in the application by a shared-secret header, failing closed.
- **Every task ends green:**
  ```
  cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
  cd web && npm run check && npx vitest run
  ```
- **Every task commits.** Small, revertible, message states intent. `CHANGELOG.md` updated
  in the same change; an ADR in `docs/decisions/` for any non-obvious choice.

## Milestones

| # | Milestone | Ends when | Plan |
|---|---|---|---|
| **M0** | Foundations | `cargo test` and `npm run check` both pass in CI; `just dev` runs; the Omnigraph graph and Graphify graph exist | [m0-foundations](2026-08-07-great-wiki-m0-foundations.md) |
| **M1** | Vertical slice on a real URL | You log in at `kb.ohje.ooguy.com` via Authelia OIDC and read a page from the document tree | [m1-vertical-slice](2026-08-07-great-wiki-m1-vertical-slice.md) |
| **M2** | Identity & access | A guest account can be created, put in a team, granted read on one subtree, and provably cannot reach anything else | [m2-identity-access](2026-08-07-great-wiki-m2-identity-access.md) |
| **M3** | Editing core | Two browsers edit one page simultaneously and both changes survive; every save is a revision you can diff and restore | [m3-editing-core](2026-08-07-great-wiki-m3-editing-core.md) |

### Outlined — planned when reached

Detailed plans go stale before they are executed at this range. Each entry states its
deliverable, its dependencies and the decisions already settled by the spec.

**M4 — Block registry.** Mermaid, freeform canvas, KaTeX math, syntax-highlighted code,
embeds, document references. *Depends on M3.* The block-type trait and registry are defined
in M3; M4 is registrations against it. Mermaid and canvas bundles are lazy-loaded — both are
large and most pages need neither.

**M5 — Media & attachments.** `BlobStore` trait with the filesystem implementation,
content-addressed by SHA-256; upload pipeline with magic-byte type detection; image
derivatives; video served by range request; sandboxed text extraction from PDF and Office
files feeding search and the graph. *Depends on M3.* Blobs land on `/mnt/cloud`; metadata on
NVMe. Extraction never runs in the request path — these parsers are a known attack surface.

**M6 — Comments & notifications.** Range-anchored comments that survive edits (ProseMirror
position mapping), mentions, the event bus, in-app notifications. *Depends on M3 and M2.*
The event bus lands here because it is the first feature that needs it, and M7, M14 and M16
all consume it rather than growing their own change detection.

**M7 — Search & AI.** FTS5 with `unicode61 remove_diacritics=2`, snippets and per-column
BM25 weights, behind the `SearchIndex` trait; chunk-level embeddings; permission-aware
retrieval; RAG assistant with citations; in-editor writing assistant with explicit
accept/reject; background suggestion jobs that propose rather than apply. *Depends on M2 for
the permission engine.* The acceptance test that matters: a signed-in guest asking the
assistant about server runbooks retrieves nothing.

**M8 — Datasets & views.** Typed field system (text, number, bool, date, select,
multi_select, tags, url, relation, rollup, formula, person, file), relations and rollups
across datasets, and the table/board/calendar/gallery/timeline/form views. Tasks and
projects are a dataset with a canonical schema plus the board view — not a subsystem.
*Depends on M3.* Column keys are validated `^[a-z][a-z0-9_]*$` at parse time; server-side
queries never interpolate an unvalidated key.

**M9 — Charts & graph.** ECharts chart blocks and the Excel-style *Create plot* builder;
edge derivation with stated reasons; Louvain clustering; per-document neighbourhood and
global graph views, searchable and filterable. *Depends on M7 for embeddings, M8 for chart
data.* Graph rendering must load zero external assets — Graphify's own `graph.html` pulls
vis-network from a CDN and would not survive the CSP.

**M10 — Citations & presentation.** Reference entities (DOI, PubMed, URL, BibTeX) with
lookup by identifier, inline citation blocks, CSL-formatted bibliography, and slides mode as
a render mode over existing blocks. *Depends on M4.*

**M11 — Translations.** Translation groups, language switcher, AI-assisted translation jobs.
*Depends on M3 and M7.*

**M12 — Exchange.** Export to Markdown, PDF, HTML, archive bundle and slides; the
round-trippable git export (`.md` + `.meta.yml` + `.design.json` + media, per spec §7);
tokenised, revocable, expiring, optionally password-protected share links. *Depends on M5.*
Round-trip fidelity is proven by property tests in `gw-core`.

**M13 — Importers.** Joplin, Obsidian, Notion, MkDocs, Confluence, Markdown, Office.
*Depends on M12* — the importer and exporter share the conversion layer, so building export
first means each importer is a mapper rather than a parser plus a mapper.

**M14 — Reach.** Documented REST API with scoped tokens, outbound webhooks with HMAC
signatures and retries, feeds, email-in via a polled IMAP mailbox. *Depends on M6* for the
event bus. IMAP polling rather than SMTP: no MX changes, no exposed mail port.

**M15 — Web clipper.** Manifest V3 browser extension with Readability extraction, posting to
the API. *Depends on M14.* Separate build artefact with its own release path.

**M16 — PWA & analytics.** Installable app, offline *reading* (service worker + IndexedDB),
and hygiene reporting: stale, orphaned and broken-link documents. *Depends on M6.* Offline
editing is explicitly out of scope; the CRDT leaves it open.

**M17 — Durability.** Scheduled git export to the private companion repository;
whole-instance restic backup to Backblaze B2 with a scoped application key, bucket
versioning and Object Lock; per-user export with credentials encrypted at rest under a key
held outside the database; and a **tested restore drill** — a backup that has never been
restored is a hypothesis.

**M18 — Packaging.** Dockerfile, hardened compose (non-root, `no-new-privileges`,
`read_only` with explicit tmpfs, dropped capabilities, memory limits, local log driver with
rotation), deployment and operations documentation. *Last, deliberately* — packaging over
something already proven rather than the only environment it has ever run in.

## Code and graph integration

Not a milestone. Graphify ingestion (`graph.json` → the same graph tables as documents) and
document→symbol links attach to **M9**, since that is when the graph tables and views exist.
Serena enrichment attaches to **M3**, since that is when the editor can resolve a reference
as it is typed. Both are additive; neither blocks anything.

## Owner's re-ordering, 2026-09-02

Two answers that change what the milestones above are worth, recorded here because the
original ordering was written when neither was known.

### Other people are coming, soon

The wiki has had one account since it existed, and several planned features are only worth
anything with more than one person in them: comments and notifications (M6), task assignment
and its "who may assign whom" rule, the read-only board card, the whole `may_write` bit.
Each was built correctly anyway — the permission model has never assumed a single user —
but they have never been *exercised* by a second person.

Family members are expected shortly. That promotes M6 above where it sat, and it makes one
unglamorous thing urgent that is on no milestone at all: **the invite flow has 42 tests and
has never been walked end to end by a real second human.** A first invitation that fails is
the worst possible first impression of a wiki somebody was asked to trust with medical
notes. Walk it before they arrive, not after.

It also means the disclosure decisions stop being theoretical. ADR 0009 (who may learn a
card's assignee), ADR 0011 (what a topic discloses) and the per-document filtering in every
aggregate view were all written against a threat model with exactly one person in it. They
are about to have a second.

### M7 is search first; the assistant is a separate decision

M7 as written is "Search & AI" — full-text search *and* a RAG assistant answering with
citations. Those are different in size, in risk and in what they depend on, and the owner
wants the first without committing to the second.

So: permission-aware full-text search, and stop. The retrieval half is the hard part and it
is shared with any assistant built later, so nothing is wasted by deferring the model.
Searching *inside* attachments is explicitly not in this — it needs sandboxed extraction in a
background job, which the roadmap already flags as an attack surface, and it can be added to
a working search rather than delaying one.
