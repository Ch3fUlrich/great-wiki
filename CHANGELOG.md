# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); newest first.
Entries describe the *effect* of a change, not the diff.

## [Unreleased]

### Added

- Identity storage: principals (from OpenID Connect or local accounts), teams and
  memberships, path-scoped access grants that inherit down the document tree, and an audit
  log.
- Default reach follows the verified Authelia group, held in a `group_roles` table rather
  than in code, so mapping a new group is a row and not a release. `admins` reach
  restricted content, `users` reach internal, anything unmapped reaches public only —
  expressed as the *absence* of a row, so a forgotten configuration can never widen access.
  The admin baseline confers reads only; writing still needs an explicit grant.
- Grants do not union up the tree: the nearest ancestor holding any grants wins outright,
  which is what makes it possible to narrow access on a subtree rather than only widen it.

- The proxy shared secret is now an enforced per-request check rather than a startup
  assertion. Requests without it, or with a wrong value, are refused before routing — so an
  unknown path returns 403 rather than 404 and cannot be used to probe what exists.
  Comparison is constant-time; an empty configured secret returns 503 rather than allowing
  through, because an unconfigured secret must never silently disable the boundary.
- Enforcement is derived from the bind address: loopback disables it, so local development
  needs no proxy in front, and anything public requires it.
- `gw-auth`: the permission engine as its own crate. One `can()` decides every
  authorisation in the system and checks authentication *before* group or team membership,
  so a forged group on an anonymous request cannot pass. Local account credentials use
  argon2id with Authelia's exact parameters, and a malformed stored hash denies access
  rather than panicking.

- `great-wiki seed --content <dir>` loads a folder of markdown files with YAML frontmatter
  into the database, so there is real content to develop against before the editor exists.
  The markdown-to-block conversion it needs is also half of the export round-trip, so this
  is foundation rather than scaffolding.
- Seeding refuses to guess. A missing title, absent frontmatter, a colliding path or a
  parent document that does not exist are each reported by name with the reason, and the
  command exits non-zero — deriving a title from a filename would silently publish a page
  at a path nobody chose.
- Markdown constructs the block model cannot yet represent are reported and their text
  kept, never dropped: table cells become one paragraph per row, and emphasis is flattened.
- `content-example/` makes the repository runnable straight after cloning, and gives CI
  something real to validate.

- Reader interface: layout with a skip link, dark and light themes following system
  preference, recursive tree navigation, document pages with an on-this-page outline, and
  error pages. Blocks render through a component that emits only the kinds it knows, so no
  untrusted HTML is ever constructed and there is no sanitisation step to get wrong.

- Document content model: a ProseMirror-shaped `Block` tree with plain-text extraction and
  a heading outline whose anchor ids are transliterated to ASCII. `Visibility` defaults to
  `Restricted`, so a document arriving with no stated visibility is never world-readable.
- SQLite store with the initial schema: documents keyed by a materialised path, with
  sibling ordering, soft delete, and a UNIQUE path so a slug collision fails loudly instead
  of silently overwriting a page.
- `great-wiki` binary with `serve` and `check`, and fail-closed startup validation: a
  synthesised development identity cannot be combined with a non-loopback bind, a public
  bind without a proxy secret is refused, and port 8090 is rejected outright because
  `omnigraph-viewer` already owns it.
- Read API — `/api/health`, `/api/tree`, `/api/documents/{*path}` — with visibility enforced
  in the retriever. Restricted documents return 403 rather than a misleading 404, and
  restricted titles are filtered out of the navigation tree entirely.
- Integration tests that exercise the real router rather than calling handlers directly, so
  a route registered without its permission check cannot pass the suite.

- Rust workspace with `gw-core`, the pure-domain crate, and the `cargo test` /
  `clippy -D warnings` / `fmt --check` gate that every later task must pass.
- `slugify` with German transliteration (ä→ae, ö→oe, ü→ue, ß→ss), so German titles produce
  readable, collision-free slugs. `Präbiotika` becomes `praebiotika`, not `pr-biotika`.
- SvelteKit 2 / Svelte 5 application skeleton with the Node adapter, Vitest, and the
  `npm run check` type gate. Node 24.19.0 pinned in `.nvmrc`.
- `slugify` in TypeScript, mirroring the Rust implementation, with a test corpus shared
  verbatim between the two so they cannot drift apart.

- Repository foundations: MIT licence, public-repo-safe `.gitignore`, `.gitattributes`
  enforcing LF on scripts and configs, and `.graphifyignore`.
- Agent instruction files — `AGENTS.md` (hub: skills, memory, architecture rules,
  verification commands) and `CLAUDE.md` (Claude-specific delta only).
- MCP wiring: project-scoped `.mcp.json` pinning the Omnigraph graph to `great-wiki`, with
  the approval gate in untracked `.claude/settings.local.json`. Graphify is deliberately
  left to user scope; Serena is configured by `.serena/project.yml`.
- `.serena/project.yml` with a conservative language list. TypeScript is omitted on purpose
  until the running image is verified to carry its language server — an absent server takes
  the entire Serena instance down rather than degrading.
