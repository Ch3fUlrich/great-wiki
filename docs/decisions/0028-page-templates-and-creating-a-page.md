# 0028 — Page templates, and creating a page over HTTP

**Status:** Accepted (2026-10-09; defaults taken by the lane, owner away)

## Context

The spec asks for a starting layout per new page (D14, "Editing: templates"). Reading the code
showed a premise missing: **no permission-checked "create a page" existed** — not in the store
(`create_document` decides no authorisation; its only caller is the importer), not in the API,
not in the web. So this change builds the create door and the templates together.

## Decision

- **A template is an ordinary page under `/vorlagen/…`** (`TEMPLATE_ROOT`). History,
  permissions, search and links come free; no storage type, no `template` column. Cost: the
  reserved name is a convention, not a constraint; anyone with write on `/vorlagen` makes
  templates. Revisit if templates need metadata (a description, a default type).
- **The picker lists templates the caller may read**, from `tree_for` — the same retriever
  every listing uses, so an unreadable template is never produced, not produced and hidden.
- **Create from template copies the body once.** No live link; editing either never changes the
  other. A copy is the only thing that cannot disclose later edits of a template.
- **Same gate as a move's destination**: signed-in active account; write on the parent;
  administration of the whole wiki for the top level. A parent the caller cannot read is
  refused as "there is no page at …" (ADR 0022).
- **A template the caller cannot read, or that is not under `/vorlagen`, answers
  "there is no template at …"** — one answer for absent, unreadable and not-a-template.
- **Placeholders `{{titel}}` and `{{datum}}` (TT.MM.JJJJ)**, filled in one pass over text leaves;
  nothing else is evaluated, and inserted text is not rescanned (a title spelling `{{datum}}`
  stays as typed). The date is the server's, passed in so tests control it.
- **A new page is `restricted`**, whatever the template's visibility (fail closed, ADR 0008).
  Rejected: inheriting the template's visibility — a public template would publish every page
  made from it.

- **An occupied address is refused, and the refusal says so.** Creating at an address a page
  already holds — live, withheld from the caller, or in the trash — answers "taken". That tells
  a writer of the parent one bit about an address under it, the rule ADR 0023 states for a
  move's destination and `crate::trash` for a restore; a prober without write on the parent
  never gets that far. The address segment is capped at 100 characters, like a title at 200,
  so a long `slug` cannot make a path of any length.

## Not built (later)

Default template per content type (D14): needs a per-type pointer to a template path; the
picker's "empty page" option stays until then.
