# great-wiki — Design Specification

**Status:** Accepted · **Date:** 2026-08-07 · **Supersedes:**
`Server/docs/superpowers/plans/2026-08-06-kbase-knowledge-platform.md`

## 1. Purpose

A self-hosted collaborative knowledge platform for a homelab: rich documents edited in
place, versioned, permissioned, cross-linked into a navigable knowledge graph, searchable
by keyword and by meaning, with structured data that can be queried, charted and explored.

It replaces the `kbase` plan, which was written around markdown files as the source of
truth. That premise is incompatible with the central requirement here — that pages, text,
images, video, tables and graphs are all editable through the rendered page — so the
storage model is inverted and the plan is superseded rather than amended.

### 1.1 The shape of the thing

**Timeline-based development of knowledge, toward publication. Research-level precision,
designer-level formatting, easy to use.**

That sentence governs every trade-off below, so it is worth unpacking:

- **Timeline-based development.** A page is not a document that gets edited; it is an idea
  that matures. The system treats that maturation as first-class — a note becomes research,
  research acquires evidence, evidence supports a claim, a claim gets published. The
  timeline shows how the knowledge got to where it is, not merely that bytes changed.
- **Toward publication.** There is a difference between the working state and the published
  state, and the system knows it. Publishing is a deliberate act producing a designed
  artefact, not just "the latest revision made visible".
- **Research-level precision.** Claims carry provenance. Sources are cited, not merely
  linked. A figure knows which dataset produced it. "Where did this come from" always has
  an answer.
- **Designer-level formatting.** The published output should look like someone designed it,
  without the author having to be a designer. Typography, spacing, hierarchy and colour are
  the system's responsibility.
- **Easy to use.** Everything above must be reachable by someone who just wants to write a
  page. Precision that requires ceremony gets skipped, and skipped precision is worse than
  none because it looks present.

Three consequences bind the design. Document **status** is a first-class axis independent
of visibility (§4) — `draft`, `developing`, `review`, `published` — so "not finished" and
"not permitted" never get conflated. Every claim-supporting artefact — a citation, a
dataset, a figure — is a **first-class entity with its own identity**, not text inside a
paragraph, which is what makes provenance queryable rather than merely written down. And
the design system is applied, not authored: the editor offers semantic choices ("this is a
callout", "this is a figure caption") rather than typographic ones.

### 1.2 Success criteria

1. A person with no technical knowledge can open a page, click into it, change a sentence,
   paste an image, and see the result — without learning markdown.
2. Two people editing the same page at the same time both keep their changes.
3. An AI agent can read and edit content programmatically, and its edits merge with a
   human's rather than overwriting them.
4. A signed-in user with no privileges cannot retrieve restricted content through *any*
   path — search, the assistant, the graph, a feed, the API, or an export.
5. Any document, or the whole corpus, can be exported to open formats and re-imported
   without loss.

## 2. Decisions

Recorded with rationale so they are not re-litigated. Non-obvious ones also get an ADR.

| # | Decision | Rationale |
|---|---|---|
| D1 | **Database is the source of truth**; Markdown is import/export | Required by in-place editing. A file-backed store forces lossy round-tripping of rich blocks and creates a second write path |
| D2 | **Rust (Axum) backend, SvelteKit 2 / Svelte 5 frontend** | User constraint. Rust gives a single deployable binary and strong invariants at the permission boundary |
| D3 | **SQLite with WAL**, FTS5 for text search | Single-writer workload, no concurrent writers by design. FTS5 keeps the security filter in the same transaction as the visibility column |
| D4 | Search behind a `SearchIndex` trait | Keeps Tantivy or an external engine a swap rather than a rewrite if the corpus outgrows FTS5 |
| D5 | **TipTap / ProseMirror** editor with **Y.js (`yrs`)** collaboration | Strict schema means a stored document is always renderable. The CRDT also solves agent-vs-human concurrent editing |
| D6 | **OpenID Connect** against Authelia, plus local accounts | Verified `groups` claims instead of trusting proxy headers; removes the entire header-forgery surface |
| D7 | great-wiki does **not** write Authelia's user database | `homelab-accounts` is architected as its sole writer. A second writer is a data-loss bug |
| D8 | **ECharts** for both charts and the graph view | One library, one bundle, one mental model. Its `graph` series covers force layout, categories and edge labels |
| D9 | Graph edges are **derived**, and every edge carries a stated reason | Curated-only graphs start empty and stay sparse. Derived edges are always current; the reason is what makes them meaningful |
| D10 | Related documents are **hybrid** — lexical + embeddings | Lexical works cold and when the model is down; embeddings catch relationships that share no vocabulary, which matters for mixed German/English |
| D11 | Blobs on the filesystem, content-addressed, behind a `BlobStore` trait | Matches the homelab's bind-mount convention. Deduplication is free. S3 stays a swap |
| D12 | Runs on **cloud.vm**; developed on coding.vm | Only cloud.vm has both bulk storage (`/mnt/cloud`, 2.1 TB) and the embedding model |
| D13 | Docker is the **last** milestone | Packaging over something already proven, not the only environment it has ever run in |
| D14 | Content types: `page`, `research`, `project`, `dataset` | Carried from kbase; they differ in listing, default template and URL prefix |

## 3. Architecture

### 3.1 Core spine

Six subsystems. Everything else plugs into these; almost every feature is a registration
against one of them rather than new plumbing.

| Spine | Responsibility |
|---|---|
| **Document graph** | Tree (`parent_id` + materialised path), revisions, CRDT state, translation groups, soft delete |
| **Identity & access** | Principals (OIDC + local), teams, tree-inheriting ACLs, API tokens, share tokens |
| **Blob store** | Content-addressed media and attachments, derivatives, extracted text |
| **Index** | FTS5, chunk embeddings, derived edges — all permission-scoped at query time |
| **Job queue** | Embeddings, extraction, imports, exports, AI suggestions, backups, git sync |
| **Event bus** | One stream feeding notifications, webhooks, feeds, analytics and edge recomputation |

The event bus exists so notifications, webhooks, RSS and analytics do not each grow their
own change-detection mechanism.

### 3.2 Extension registries

Five registries. Adding a diagram type, a calendar view or a Confluence importer means
implementing one trait and registering it — the core does not change.

- **Block types** — paragraph, heading, list, quote, code, table, image, video, file,
  Mermaid, canvas, math, dataset-view, chart, embed, document-reference, citation,
  bibliography
- **Dataset views** — table, board, calendar, gallery, timeline, form
- **Importers** — Joplin, Obsidian, Notion, MkDocs, Confluence, Markdown, Office, web clip, email
- **Exporters** — Markdown, PDF, HTML, archive bundle, slides
- **Integrations** — REST API, webhooks, feeds

### 3.3 Crates

```
gw-core    pure domain: document model, block schema, markdown <-> document,
           slugs (with transliteration), sanitisation. No I/O.
gw-store   SQLite, migrations, revisions, FTS5, dataset queries
gw-auth    OIDC client, sessions, local credentials, the permission engine
gw-media   BlobStore trait + filesystem impl, uploads, derivatives, text extraction
gw-graph   edge derivation, similarity, clustering
gw-ai      LiteLLM/embedding clients, chunking, permission-aware retrieval
gw-jobs    background workers
gw-api     HTTP API, WebSocket (CRDT sync), CLI
gw-mcp     MCP server exposing content operations to AI agents
web/       SvelteKit application
extension/ browser web clipper (separate build artefact)
```

`gw-core` is free of I/O so the document model and markdown conversion can be tested
exhaustively without a database — that is where round-trip fidelity is proven.

## 4. Data model

**Identity** — `principal` (kind: oidc | local, subject, username, display_name, email,
status), `credential` (local accounts only, argon2id), `team`, `team_member` (role),
`api_token`, `share_token`.

**Content** — `space`, `document` (space, parent, materialised path, sort_key, type, slug,
title, language, translation_group, status, visibility, current_revision, deleted_at),
`revision` (immutable: document, parent_revision, block tree, crdt_state, summary, author,
created_at), `template`.

**Access** — `acl` (subject: team | principal | oidc-group; on space or document;
permission: read | comment | write | admin). ACLs inherit down the tree unless a descendant
overrides. Three visibility levels: `public`, `internal` (any authenticated), `restricted`.

**Media** — `blob` (sha256, size, mime, kind, dimensions/duration, extracted_text),
`document_blob` (usage tracking, enabling garbage collection).

**Structured data** — `dataset`, `dataset_field` (typed: text, number, bool, date, select,
multi_select, tags, url, relation, rollup, formula, person, file), `dataset_row`,
`dataset_view` (kind + saved config), `ingestion_recipe`.

**Status** is a first-class axis on `document`, independent of `visibility`:
`draft` → `developing` → `review` → `published`, plus `archived`. Conflating "not finished"
with "not permitted" is a common wiki failure — it makes every unfinished page look secret
and every secret page look unfinished. They are separate columns and separate filters.

**Knowledge intelligence** — `entity` (a concept, term, dataset, figure or citation with its
own identity, so a reference to it is resolvable and renameable), `entity_mention` (where an
entity is referenced, with position), `claim` (an assertion a document makes), `evidence`
(what supports or contradicts a claim, pointing at a reference, dataset or another
document), and `claim_edge` (`supports`, `contradicts`, `supersedes`, `cites`) with a
`confidence` grade of `stated`, `inferred` or `uncertain`. See §11.5.

**Knowledge** — `tag`, `document_tag`, `doc_link` (authored references), `edge` (derived:
from, to, kind, weight, reason), `embedding` (chunk-level), `reference` (citations: DOI,
PubMed, URL, BibTeX), `document_reference`.

**Operations** — `audit_log`, `job`, `event`, `notification`, `comment` (range-anchored).

Revisions are append-only and immutable. That is what makes diff, restore, blame and the
timeline trivial rather than features.

## 5. Storage and deployment

| What | Where | Why |
|---|---|---|
| Database, search index, vectors | `$APPS_ROOT/great-wiki/` on NVMe | Small, hot, transactional. **Never a database on NFS** |
| Blobs (media, attachments) | `/mnt/cloud/great-wiki/media/`, content-addressed | 2.1 TB available; matches how Immich, Nextcloud and Paperless split hot from bulk |
| Offshore backup | restic → Backblaze B2 | Client-side encryption, deduplication, incremental snapshots. Scoped application key, bucket versioning and Object Lock so ransomware cannot delete history |
| Git export | Private companion repository | A portable, human-readable second copy that survives the application |

Bind to `0.0.0.0` on a free port — Caddy runs on OPNsense, a different host, so a loopback
bind is unreachable from it. **Not port 8090**: that is `omnigraph-viewer`. Because the port
is therefore LAN-reachable, proxy-only access is enforced in the application by a shared
secret header, failing closed, not by the bind address.

## 6. Security model

### 6.1 The governing invariant

**Every retrieval path filters by the caller's permissions at query time, inside the
retriever.** Search, RAG, graph, feeds, API, share links, exports and analytics. Never as a
post-filter: once content is in a model's context window or a response body, filtering is
too late. The corpus will hold server runbooks; the failure mode is a signed-in guest
asking the assistant a question and receiving them.

### 6.2 Layers

1. **Authentication** — OIDC authorization-code with PKCE. Local accounts use argon2id with
   the same parameters as Authelia (m=65536, t=3, p=4). Server-side sessions;
   `HttpOnly; Secure; SameSite=Lax` cookies.
2. **Authorization** — one `can(principal, action, resource)` function, deny by default,
   used by every handler. No handler makes its own decision.
3. **Transport** — proxy shared-secret header verified in constant time before any identity
   is read; unset secret is a startup refusal, not a silent disable.
4. **Content** — strict block schema, sanitisation on import, Content-Security-Policy,
   CSRF tokens on all mutations, rate limits on authentication, search and upload.
5. **Uploads** — magic-byte type detection (a filename is not evidence of content), per-kind
   size caps, extraction in a sandboxed job, blobs served with `Content-Disposition` from an
   isolated path.

### 6.3 Development identity

`GW_DEV_IDENTITY=<user>:<groups>` synthesises an identity so private content and admin
surfaces are testable without a proxy, and **refuses to start unless the bind address is
loopback**. Without it, half the application is untestable outside production.

## 7. Content storage and export format

The database stores documents as a block tree. The **git export** writes three files per
document plus shared media, chosen so the portable part is genuinely human-readable and the
non-portable part is isolated rather than smeared through it:

```
<space>/<path>/<slug>.md            prose in CommonMark; rich blocks appear as fenced
                                    directives carrying only a reference id
<space>/<path>/<slug>.meta.yml      identity and metadata: id, type, parent, sort key,
                                    tags, language, translation group, visibility, ACL,
                                    timestamps, authors
<space>/<path>/<slug>.design.json   layout and design settings, plus payloads for blocks
                                    with no markdown equivalent (canvas scenes, chart
                                    specs, dataset view configs), keyed by those reference ids
media/<sha256>.<ext>                blobs, referenced from the markdown
```

A document opened in any text editor reads as normal markdown; the design lives beside it
rather than inside it. Importing the triple reconstructs the document exactly — round-trip
fidelity is a tested property of `gw-core`, not an aspiration.

## 8. Data ingestion and chart building

### 8.1 Ingestion

Sources: CSV, TSV, Excel (`.xlsx`/`.xls`), JSON, and read-only database connections
(PostgreSQL, MySQL, SQLite) with saved queries.

A staged wizard, each stage previewing real data:

1. **Source** — upload a file or choose a connection and query
2. **Preview** — first rows, detected delimiter, encoding, header row
3. **Schema** — rename columns, set types, mark the key, exclude columns
4. **Rows** — filter, trim, split, derive computed columns, deduplicate
5. **Target** — create a new dataset, or append/upsert into an existing one
6. **Import** — with a report naming every rejected row and why

The mapping is saved as an **ingestion recipe**, so re-importing or scheduling a refresh
replays the same transformation rather than repeating the wizard.

### 8.2 Chart building

A **Create plot** button on any dataset or table opens a builder: choose chart type, map
columns to axes, series and aggregation, with a live ECharts preview. Save it as a chart
block on a page, or as a saved view on the dataset. Deliberately spreadsheet-like — the
point is that someone who can build a chart in Excel can build one here.

## 9. History, timeline and transparency

Per document:

- **Timeline** — every revision in order with author, time, summary and size delta.
- **Three diff modes** — *prose* (word-level), *structure* (blocks added, moved, removed),
  and *design* (structural diff of layout and block configuration). Design changes are
  visible as changes, not hidden inside an opaque payload.
- **View source** — the exported markdown, metadata and design files for any revision.
- **Restore** — restoring creates a new revision; history is never destroyed.
- **Compare** — any two revisions, not only adjacent ones.

Per space: an activity feed built from the event bus.

## 10. Capabilities

**Editing** — collaborative rich text (Y.js), templates, duplicate, drag-to-reorganise,
trash with retention and restore.

**Blocks** — Mermaid diagrams, freeform canvas, LaTeX (KaTeX), syntax-highlighted code,
embeds, document references, citations and bibliography.

**Structured data** — typed fields with relations and rollups; table, board, calendar,
gallery, timeline and form views; tasks and projects modelled as a dataset with a canonical
schema plus a board view, not as a separate subsystem.

**Search and AI** — FTS5 keyword search with snippets; chunk-level embeddings; a
retrieval-augmented assistant that cites its sources; an in-editor writing assistant
operating on the selection with explicit accept/reject; background suggestion jobs for
tagging, summarising and linking that **propose rather than apply**. All LLM calls route to
LiteLLM (`deepseek-v4-flash`); embeddings use `nomic-embed-text` (768-dim).

**Knowledge graph** — derived edges with stated reasons (`links to`, `shares tags: …`,
`similar content 0.87`, `same space`, `cites same source`), server-side community detection
for colour-coded clusters, a per-document neighbourhood view and a global view, both
searchable and filterable by type, tag, cluster and edge kind.

**Collaboration** — range-anchored comments that survive edits, mentions, in-app notifications.

**Exchange** — export to Markdown, PDF, HTML, archive bundle and slides; import from Joplin,
Obsidian, Notion, MkDocs, Confluence, Markdown and Office files; tokenised, revocable,
optionally expiring and password-protected public share links.

**Reach** — documented REST API with scoped tokens, outbound webhooks with HMAC signatures
and retries, feeds, email-in via a polled IMAP mailbox (no MX changes, no exposed mail
port), and a browser web clipper.

**Platform** — installable PWA with offline *reading*, multi-language documents as linked
translation siblings, soft delete, and hygiene analytics (stale, orphaned, broken links).

## 11. Knowledge graph, code integration and the MCP stack

Three servers already exist in this estate — Serena, Graphify and Omnigraph. They are
complements, not alternatives, and the design uses each for the one thing it is good at.

### 11.1 Graph storage lives in SQLite, not Omnigraph

Omnigraph is agent *memory*, not this product's storage layer. Four reasons, in weight order:

1. **Availability.** `structured-memory` states plainly that there is no fallback and that
   memory is "an accelerator, not a correctness dependency". That is right for memory and
   wrong for a product's primary read path. Applying any schema change stops the server for
   *every* graph — `apply-cluster.sh` does `docker stop` to release the state lock. A wiki
   whose pages stop rendering during an unrelated graph's migration is not acceptable.
2. **Permission filtering.** `.gq` has no row-level security. §6.1 requires filtering inside
   the retriever on every path; that can be made structural in SQLite (a permission-joined
   view, a builder that cannot emit an unfiltered read) and can only be a convention in `.gq`.
3. **Expressiveness.** `.gq` has no variable-length traversal — multi-hop must be written
   hop by hop. SQLite's `WITH RECURSIVE` does exactly this, and FTS5 and vector scanning sit
   in the same store.
4. **Maturity for this role.** The live cluster holds 621 nodes total across five graphs,
   with 253 of 256 `Decision` vectors null — semantic search is currently dead cluster-wide
   — four unfixed embedding defects, no API to delete an individual edge, and an hourly cron
   job whose purpose is removing duplicate edges. An acceptable risk profile for memory; not
   for storage.

**What Omnigraph keeps doing:** holding this project's memory (Decisions, Rules,
Conventions, Components, Tasks) exactly as the skill prescribes. Optionally, a *one-way,
best-effort* projection of the document graph is pushed to it (`load`, `mode: merge`) so
great-wiki's structure is queryable alongside the other repos' graphs. Export, never store —
if it is down, the wiki does not notice.

**What is worth stealing from it:** typed nodes and typed edges with no generic
`relates-to`; `@key`-based idempotent upsert; the "a node whose only edge is to the hub is
under-linked" lint, which is a genuinely good invariant for a wiki; and the viewer's
server-side-token proxy pattern, where the browser never sees a credential and the page
loads zero external assets.

> **Prerequisite, currently unmet:** the `great-wiki` graph does not exist —
> `GET /graphs/great-wiki/schema` returns 404. `add-project-graph.sh great-wiki &&
> apply-cluster.sh` must run first, and `apply-cluster.sh` refuses while any non-`main`
> branch exists (there are currently 12 stale `mem/homelab-server/*` branches to merge and
> delete). Until then the `.mcp.json` bridge points at a graph that is not there.

### 11.2 Graphify supplies the code graph

Graphify produces a complete code-structure graph as a static `graph.json`: roughly 2.5
seconds for a mid-size repository, no API key, no network, no LLM for the code path.
great-wiki ingests that file directly — a serde struct and a loop — with **no graphify
dependency at all**.

What it yields: file and symbol nodes with line numbers; `calls`, `imports_from`, `contains`,
`implements`, `references`, `method` edges each carrying a confidence grade; precomputed
Louvain communities with names; god-node degree rankings; and `rationale` nodes, which are
docstrings already extracted and already edged to the function they describe.

Imported nodes go into **the same graph tables as document nodes**, so one traversal spans
prose and code.

Two gotchas that must be designed around, both verified:

- **Svelte is parsed shallowly and its import edges are broken.** A `.svelte` file produces
  one node; functions inside `<script>` are not extracted, and its imports resolve to
  duplicate stub nodes with a `repo_`-prefixed id that never unify with the real module.
  Normalise or drop those on ingest, or the graph fills with phantom modules. TypeScript,
  by contrast, is parsed fully — interfaces, classes, methods, calls and imports.
- **Graphify node ids are not stable across versions.** Persistent document→code links must
  be keyed on `(repo, source_file, label)`, never on graphify's `id`.

Graphify also has an HTTP transport (`--transport http`, with a per-call `project_path`),
despite local documentation claiming stdio-only — so one long-running instance can serve
every repository's graph if direct file ingestion ever becomes inconvenient.

### 11.3 Serena is a precision oracle, used narrowly

Serena answers point questions exactly: a symbol's real signature, its true LSP references
with surrounding code, its implementations. It has no bulk export — synthesising a graph
from it would mean one LSP round-trip per symbol through a single-threaded manager.

Use it for **authoring-time enrichment only**: resolving a symbol as an author types a
reference, and validating that a linked symbol still exists. Never on the page-render path.
It is loopback-only, speaks MCP JSON-RPC over SSE rather than REST, holds one activated
project at a time, and one failing language server takes the whole manager down. Every call
sits behind a short timeout with a cached fallback.

### 11.4 Documents linked to code — the actual feature

Everything above is plumbing; this is the product idea. An author writes a symbol reference
in a page, and the page gains a live view of the code it documents:

| Panel | Source |
|---|---|
| **In the code** | symbol, file, line, degree, community |
| **Calls** | one hop out on `calls` |
| **Called by** | one hop in — the backlink a wiki cannot compute for itself |
| **Blast radius** | reverse traversal: "changing this reaches N symbols across M modules", intersected with document→symbol links to add "…and 3 other pages document symbols in that radius" |
| **Related modules** | the symbol's community, each member linkable to its own page |
| **Why** | the `rationale` node edged to the function, rendered as an epigraph |

Edge kinds for authored links: `documents` (page → symbol), `example_of`, `supersedes`,
`decided_by` (symbol → ADR).

**Staleness is a first-class state.** Store the commit the graph was built at. When
re-ingestion finds a linked symbol gone, mark the link *broken* and surface it in the
maintenance report — the same shape as a broken wiki link, a concept users already
understand. This is the payoff: a code change and a documentation change become mutually
discoverable.

`graphify export wiki` already renders roughly this as markdown. It must not be shipped —
it would violate §1's storage model — but its implementation is a working reference for the
layout and is worth reading before designing the panels.

### 11.5 Content intelligence — the same three capabilities, turned inward

Serena, Graphify and Omnigraph solve, for code, exactly the three problems this platform
has for prose. great-wiki does **not** call those servers to serve its own users — they are
loopback-only developer tooling, single-project, and unavailable during maintenance (§11.1).
It reimplements the three *capabilities* over its own content, reusing their approaches and,
where licensing allows, their code.

| Their tool | Its question | great-wiki's equivalent |
|---|---|---|
| **Serena** | "Where is this symbol defined, and what references it?" | **Entity resolution.** Concepts, datasets, citations, figures and terms are addressable entities. Jump to definition; list every page that references a term; rename an entity and update every reference |
| **Graphify** | "What connects X to Y, and what does this change reach?" | **Corpus structure.** Whole-corpus graph with Louvain communities, hub ("god") nodes, cohesion scores, surprising connections, and blast radius — "changing this claim reaches N pages" |
| **Omnigraph** | "What did we decide, and why?" | **Typed knowledge.** Claims, evidence, decisions and open questions as typed nodes with typed edges (`supports`, `contradicts`, `supersedes`, `cites`), so the reasoning behind a page is queryable, not buried in prose |

**What is reused rather than reinvented:**

- **Graphify is MIT-licensed Python**, and its analysis layer is directly portable:
  `cluster()` (Louvain with a hub-exclusion percentile), `god_nodes()`, `cohesion_score()`,
  `surprising_connections()`, `graph_diff()` and `compute_pr_impact()` — the last being
  exactly the blast-radius computation, generalised from changed files to changed entities.
  Its node/edge schema is also worth adopting wholesale: a `confidence` grade
  (`EXTRACTED` / `INFERRED` / `AMBIGUOUS`) on every edge, and a `rationale` node type edged
  to what it explains. Both map onto prose better than they map onto code.
- **Omnigraph's discipline, not its storage** (ADR 0004): typed nodes and typed edges with
  no generic `relates-to`; key-based idempotent upsert; and the lint that a node whose only
  edge is to its hub is under-linked.
- **Serena's model**: a language server resolves symbols precisely on demand rather than
  maintaining a whole-corpus index. The prose equivalent resolves an entity reference at
  authoring time and validates it on change — the same "precise oracle, not bulk index"
  split that keeps the render path fast.

**Semantic extraction** — turning prose into entities, claims and typed edges — runs through
**LiteLLM (`deepseek-v4-flash`)**, the same route graphify's own document extraction uses.
It is a background job, never in the request path, and its output is **proposed** for human
confirmation rather than applied: an extractor silently asserting that one page contradicts
another is how a knowledge base acquires confident nonsense.

This is what makes the "development of knowledge" in §1.1 tractable. Without typed claims
and evidence, a timeline can only show that text changed. With them, it can show that a
claim gained support, lost it, or was superseded — which is the thing actually worth
watching.

## 12. Interface requirements

Dark and light themes with system detection and a manual override. **Accessibility is
tested, not assumed**: `aria-sort` on sortable columns, labelled controls, table captions
and scopes, skip links, managed focus, full keyboard navigation. Responsive down to phone
widths including tables. Syntax highlighting and sane print output. German and English
content throughout, which means transliterating umlauts in slugs (`ä→ae`, `ö→oe`, `ü→ue`,
`ß→ss`), diacritic-folding search, and locale-aware sorting and formatting.

## 13. Non-goals

- **Video transcoding.** Uploads are stored and served as-is with range requests.
- **Offline editing.** The PWA is read-only offline in v1; the CRDT makes this extensible.
- **Scanned-PDF OCR.** Paperless already does this; integration is cheaper than rebuilding.
- **Replacing the MkDocs site.** Out of scope — that corpus is ~490 files with dozens of
  colliding titles and extensions this renderer does not support. Its own project.
- **Managing homelab accounts.** See D7.
- **Multi-tenancy.** One instance, one organisation.

## 14. Risks

| Risk | Mitigation |
|---|---|
| Permission-aware RAG leaks restricted content | Filter in the retriever; test with an explicit "signed-in guest asks about runbooks" case that must return nothing |
| Scope is very large | Milestones are independently shippable; M1 puts a real URL in front of the user early |
| cloud.vm root is at 95 % | Database and index are small; blobs go to `/mnt/cloud`. Monitor before M18 |
| CRDT state growth | Snapshot and compact at publish points; revisions store the block tree, not full CRDT history |
| Serena has no verified TypeScript server | Frontend uses built-in tools until the image is checked; do not add the language speculatively |
| Per-user cloud credentials for backup | Encrypted at rest with a key held outside the database |

## 15. Milestones

M0 Foundations · M1 Vertical slice on a real URL · M2 Identity & access · M3 Editing core ·
M4 Block registry · M5 Media & attachments · M6 Comments & notifications · M7 Search & AI ·
M8 Datasets & views · M9 Charts & graph · M10 Citations & presentation · M11 Translations ·
M12 Exchange · M13 Importers · M14 Reach · M15 Web clipper · M16 PWA & analytics ·
M17 Durability · M18 Packaging.

M0–M3 are planned in task-by-task detail; M4–M18 are outlined and re-planned as they are
reached, because detailed plans at this range go stale before they are executed.
