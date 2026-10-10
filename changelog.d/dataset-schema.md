### Added
- A dataset's columns can now be listed, added, relabelled, reordered and deleted
  (`/api/datasets/schema/{path}`). Anyone who may read the dataset sees them; changing them needs
  write on the page. A column's key and kind never change. A dataset holds at most 100 columns.
  Deleting a column removes its values from every row in one step. A dataset you cannot read is
  refused in the same words as one that does not exist (ADR 0029).
