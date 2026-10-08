### Added

- **Comments can be read and written over HTTP.** `GET /api/comments/document/{path}` returns
  a page's comment threads and `POST` adds a comment, optionally a reply (`parent_id`) or one
  anchored to a passage (`anchor` with base64 `start`/`end` positions and a `quote`). Reading
  a page is all it takes to comment on it. A page you may not read answers exactly as a page
  that does not exist, on both verbs. A reply to a comment from another page, or to a reply,
  is a 400. Comments cannot be deleted or edited over the API, and the responses carry no
  total ([ADR 0025](docs/decisions/0025-what-a-comment-discloses.md)).
