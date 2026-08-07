# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); newest first.
Entries describe the *effect* of a change, not the diff.

## [Unreleased]

### Added

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
