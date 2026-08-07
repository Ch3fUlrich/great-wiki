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

### 1.1 Success criteria

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

## 11. Interface requirements

Dark and light themes with system detection and a manual override. **Accessibility is
tested, not assumed**: `aria-sort` on sortable columns, labelled controls, table captions
and scopes, skip links, managed focus, full keyboard navigation. Responsive down to phone
widths including tables. Syntax highlighting and sane print output. German and English
content throughout, which means transliterating umlauts in slugs (`ä→ae`, `ö→oe`, `ü→ue`,
`ß→ss`), diacritic-folding search, and locale-aware sorting and formatting.

## 12. Non-goals

- **Video transcoding.** Uploads are stored and served as-is with range requests.
- **Offline editing.** The PWA is read-only offline in v1; the CRDT makes this extensible.
- **Scanned-PDF OCR.** Paperless already does this; integration is cheaper than rebuilding.
- **Replacing the MkDocs site.** Out of scope — that corpus is ~490 files with dozens of
  colliding titles and extensions this renderer does not support. Its own project.
- **Managing homelab accounts.** See D7.
- **Multi-tenancy.** One instance, one organisation.

## 13. Risks

| Risk | Mitigation |
|---|---|
| Permission-aware RAG leaks restricted content | Filter in the retriever; test with an explicit "signed-in guest asks about runbooks" case that must return nothing |
| Scope is very large | Milestones are independently shippable; M1 puts a real URL in front of the user early |
| cloud.vm root is at 95 % | Database and index are small; blobs go to `/mnt/cloud`. Monitor before M18 |
| CRDT state growth | Snapshot and compact at publish points; revisions store the block tree, not full CRDT history |
| Serena has no verified TypeScript server | Frontend uses built-in tools until the image is checked; do not add the language speculatively |
| Per-user cloud credentials for backup | Encrypted at rest with a key held outside the database |

## 14. Milestones

M0 Foundations · M1 Vertical slice on a real URL · M2 Identity & access · M3 Editing core ·
M4 Block registry · M5 Media & attachments · M6 Comments & notifications · M7 Search & AI ·
M8 Datasets & views · M9 Charts & graph · M10 Citations & presentation · M11 Translations ·
M12 Exchange · M13 Importers · M14 Reach · M15 Web clipper · M16 PWA & analytics ·
M17 Durability · M18 Packaging.

M0–M3 are planned in task-by-task detail; M4–M18 are outlined and re-planned as they are
reached, because detailed plans at this range go stale before they are executed.
