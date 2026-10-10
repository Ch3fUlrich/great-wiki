### Added
- A dataset's rows can be listed with their count (`GET /api/datasets/rows/<path>`, optional `limit` and `offset`). A dataset the caller cannot read answers exactly as a missing one does - same status, same body - for guests, other groups and admins viewing as them.
