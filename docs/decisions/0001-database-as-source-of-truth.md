# 0001 — The database is the source of truth, not markdown files

**Status:** Accepted (2026-08-07)

## Context

The predecessor plan (`kbase`) made markdown files on disk the source of truth: agents
edited files, a Rust service ingested them into SQLite, and SQLite was a disposable derived
cache. That plan explicitly ruled out in-browser editing, reasoning that *"a WYSIWYG editor
would create a second write path and a merge problem."*

The requirement for great-wiki is the opposite: pages, text, images, video, tables and
graphs must all be editable through the rendered page, so the result is visible as it is
being made. A files-first store cannot satisfy this without lossy round-tripping — a canvas
scene, a chart specification or a saved board view has no faithful markdown representation,
and forcing one degrades both the file and the editor.

## Decision

Documents are stored in the database as a structured block tree, with immutable revisions.
Markdown is an **import and export format**, not storage.

Two consequences are accepted deliberately:

1. **Git no longer supplies version history for free**, so revisions, diff, restore and a
   timeline are first-class features rather than something inherited.
2. **Agents no longer edit by writing files.** They edit through the API and an MCP server.

The second turns out to be an improvement rather than a cost. Because collaborative editing
is built on a CRDT (Y.js via `yrs`), agent edits and human edits are transactions against
the same shared document and merge automatically. The "second write path" problem that
justified banning the editor does not arise — there is only one write path, and everything
goes through it.

Portability is preserved by a scheduled git export: prose as CommonMark, metadata as YAML,
and design settings as a referenced sidecar (see the design spec, §7). The export is
round-trippable, and that round-trip is a tested property of `gw-core`.

## Consequences

- Revisions, diffing and restore must be built. They were going to be needed regardless.
- Backup is now the application's responsibility: restic to Backblaze B2, plus the git
  export as an independent human-readable copy.
- The block schema becomes a compatibility surface. Adding a block type is additive;
  changing one needs a migration over stored revisions.
- Content survives the application, because the export is open formats with no proprietary
  container.

## Switch-back criteria

If the editor is abandoned and all authoring returns to files, revert to files-as-truth and
keep the database purely derived. Nothing in this decision prevents that: the git export is
already the full corpus in file form.
