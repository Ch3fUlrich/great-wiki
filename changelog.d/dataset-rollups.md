<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A dataset column can total, average or list the rows another dataset points at.** A `rollup`
  column says `{"relation": "<key of a relation column here>", "target_field": "<key of a field
  there>", "fn": "count|sum|min|max|avg|list"}` and is added through the schema endpoint like any
  other column: write on this dataset, and read on the one the relation reaches. It is checked when
  the column is saved, not when a row is read — the relation must be a relation column of *this*
  dataset, its target must be readable by the caller, and the field it aggregates must be a stored
  column of a type the aggregate can answer (`sum` and `avg` a number, `min` and `max` a number or
  a date, `list` anything stored, `count` nothing at all). `count` ignores the field it names but
  still needs the string, so every rollup carries exactly those three members and an extra or
  mistyped one is refused rather than quietly dropped. `rollup target is not available` is the one
  refusal for an unreadable target, a deleted one and a relation whose stored config has gone bad:
  saving a column is no way to ask what sits behind somebody else's relation.
  - **The value is asked for when a row is read and never kept.** A rollup is a question about
    other rows, so storing its answer is how it later disagrees with them; the row in the database
    holds only what somebody typed. Writing a value into a rollup cell is refused, as a formula
    cell already was.
  - **A reader who cannot see the target does not see the rollup.** No key in the schema, none in
    the row — not `0`, not `null`, not `[]`, because each of those is an answer about a dataset they
    were never given, and a zero would say "there is a column here and it counts nothing". Absent
    behaves exactly as the relation column it sits on does, and the access question is asked
    *before* anything is aggregated, not by blanking an answer afterwards. Two mutation entries
    guard it (`./scripts/mutate.sh rollup`): the gate removed, and the omission replaced by a zero.
  - **The aggregates.** `count` is the number of live links — a number, `0` when there are none —
    and does not look at the target's cells; a link whose row was deleted is already gone. `sum`
    adds the numbers it finds, skipping text and empty cells, and is `0` over nothing. `avg` is the
    mean of those numbers and `null` over nothing, because the mean of nothing is not nought.
    `min`/`max` compare numbers as numbers and dates as the ISO text they are, which is calendar
    order, and answer `null` over nothing. `list` returns the non-empty cells in the order the cell
    lists its links, flattening a multi-valued cell one level and stopping at 100. A result that is
    not a finite number becomes `{"error": "#ERR(not a finite number)"}`, the cell a formula error
    already leaves behind.
  - **A formula may read a rollup**, because a row is decorated in one fixed order: the relation
    cells first (which is where the caller's access to the target is asked), then the rollups, then
    the formulas. Not the other way round: a rollup reading a formula would re-read the rows it is
    standing on.
  - **Filtering or sorting on a rollup is refused** the way a formula is (`computed field \`x\`
    cannot be filtered or sorted yet`), since its value is not in the SQL to look at — and as
    `no field \`x\`` when the rollup is hidden, which is the same sentence a made-up key gets.

### Changed

- **`rollup` is no longer refused as a kind that "is not available yet".** Every stored kind plus
  `formula` and `rollup` are offered at the schema endpoint; a rollup with no config is still
  refused, by its own shape check rather than by the placeholder.
