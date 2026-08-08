# great-wiki

A self-hosted, collaborative knowledge platform. Rich documents you edit in place,
versioned and permissioned, cross-linked into a navigable knowledge graph, searchable by
keyword and by meaning, with structured data you can query, chart and explore.

> **Status: design and planning.** No implementation yet. The design lives in
> [`docs/superpowers/specs/`](docs/superpowers/specs/) and the implementation plan in
> [`docs/superpowers/plans/`](docs/superpowers/plans/).

## What it does

- **Edit what you see.** Collaborative rich-text editing on the rendered page — text,
  images, video, tables, diagrams, math, charts — with real-time multi-user editing.
- **Nothing is lost.** Every save is a revision. Diff it, restore it, see who changed what.
- **Structured data is first class.** Typed datasets with relations and rollups, viewable
  as a table, board, calendar, gallery or timeline, and chartable without leaving the page.
- **Find things two ways.** Full-text search, and semantic search plus a retrieval-augmented
  assistant that answers questions from your own content — always filtered by what the
  asker is allowed to see.
- **See the shape of what you know.** A derived knowledge graph with labelled edges that
  say *why* two documents are connected, clustered and colour-coded, searchable and
  filterable.
- **Your content stays yours.** Export any page or space as Markdown, PDF, HTML or a
  self-contained archive. Import from Joplin, Obsidian, Notion, MkDocs and Confluence.

## Architecture at a glance

| Layer | Choice |
|---|---|
| Backend | Rust, Axum, SQLite (WAL), FTS5 |
| Frontend | SvelteKit 2 + Svelte 5 |
| Editor | TipTap / ProseMirror with Y.js (CRDT) collaboration |
| Charts & graph | ECharts |
| Identity | OpenID Connect, with local accounts for external collaborators |
| Search | SQLite FTS5 + chunk embeddings (`nomic-embed-text`, 768-dim) |
| Media | Content-addressed blob store behind a swappable trait |

The database is the source of truth; Markdown is an import/export format. Documents are
stored as structured blocks so the editor, the renderer and the exporters all agree on what
a document *is*.

## Repository layout

```
crates/gw-core     pure domain — document model, markdown conversion, sanitisation
crates/gw-store    SQLite, migrations, revisions, search, dataset queries
crates/gw-auth     OIDC, sessions, local credentials, the permission engine
crates/gw-media    blob store, uploads, derivatives, text extraction
crates/gw-graph    edge derivation, similarity, clustering
crates/gw-jobs     background workers
crates/gw-api      HTTP API and CLI
crates/gw-mcp      MCP server, so AI agents edit content natively
web/               SvelteKit application
content-example/   sample content — the app runs and tests out of the box
docs/              specs, plans, ADRs, operations
```

## Getting started

Requires Rust (stable) and Node.js. Nothing is runnable yet — see the plan.

## Security

Every retrieval path filters by the caller's permissions at query time. Report security
issues privately rather than in a public issue.

## Licence

The code is MIT — see [LICENSE](LICENSE).

The bundled typefaces are not. **IBM Plex Sans**, **IBM Plex Mono**, **Literata** and
**JetBrains Mono** are licensed under the SIL Open Font License 1.1, and OFL condition 5
requires that they stay entirely under it — living in an MIT repository does not make a
font MIT. Each family's `OFL.txt` sits beside its files in `web/static/fonts/<family>/`.

They are self-hosted, and nothing on a page ever requests a font from a third party:
a font fetched from a foreign host on every page load discloses the reader's IP address
and the page they are on to that host. IBM's licence reserves the name *Plex*, which
under the OFL means those files must not be subset — see the note at the top of
[`web/src/lib/styles/fonts.css`](web/src/lib/styles/fonts.css).

Two checks keep that honest. `cd web && npm test` verifies, with no extra tooling, that
every shipped face contains a real ẞ (U+1E9E) rather than a faked SS, and that the three
choices still map to the families the control promises.
`python3 web/scripts/check-fonts.py` (needs `pip install 'fonttools[woff]'`) adds the
licence checks — the notices inside each binary, and that the reserved-name files are
byte-for-byte upstream.
