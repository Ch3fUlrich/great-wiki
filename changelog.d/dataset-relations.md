<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A dataset column can point at rows of another dataset.** `relation` fields hold the id (or
  ids) of rows in a target dataset, chosen when the column is created. Writing a cell needs read
  on the target as well as write on the source, and every refusal — unreadable target, a row of
  some other dataset, an id that does not exist — carries the same words, so a cell is no way to
  ask what another dataset holds.
  - **A reader who cannot see the target does not see the column.** It is absent from the schema
    and from every row, not shown empty; writing it is refused as an unknown field.
  - **Deleting a target row removes the links to it**; the source cell simply stops listing it.
    Rows pointing at a given row are found through an index (migration 0022).
