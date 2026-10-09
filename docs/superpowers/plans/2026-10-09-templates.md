# Templates — plan (2026-10-09)

ADR: `docs/decisions/0028-page-templates-and-creating-a-page.md`. Migration: none.

1. **Store** `gw-store/src/templates.rs`: `templates_for`, `create_page_for`, placeholder fill. Rust tests.
2. **API** `gw-api/src/routes/templates.rs`: `GET /api/templates`, `POST /api/pages` `{parent,title,slug,template}`; integration tests (404/409/403 mapping, ADR 0022).
3. **Web**: create form `/neu` (GET form + POST action, works without JS; template `<select>`, keyboard usable); vitest.
4. **Behaviour** group `templates` in `just behaviour`; **mutations** `templates:` in `scripts/mutate.sh`, all killed.
5. Docs + changelog fragment with each task.
