### Added

- `GET /api/templates` lists the templates the caller may read; `POST /api/pages` creates a page
  (optionally from a template) — the first way to create a page over HTTP. It needs write on the
  parent (administration of the whole wiki for the top level), refuses reserved addresses
  (`admin`, `api`, … at the top, `history` anywhere) and titles over 200 characters, and a parent
  or template the caller cannot read is refused in the words of a missing one. ADR 0028.
