<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A formula or a rollup can be filtered and sorted like a stored column.** Until now the row
  query refused them outright — `computed field \`x\` cannot be filtered or sorted yet` — because
  their value is not in the database and SQL cannot reach it. For a caller who may see the column,
  the answer is now computed and used: `sum` of what a page of rows points at, `twice` of what they
  hold, ordered, paged and counted like any other column. Nothing about the access rule changed —
  a column hidden from the caller is still named in the exact words used for a key that does not
  exist, and a visible `relation` keeps its "…cannot be filtered or sorted yet", since its cell is a
  list of row ids and no ordering of one is meaningful yet.

  How it works is worth knowing because it is not free. Such a query cannot be answered in SQL, so
  the stored filters run first, the rows they leave are decorated and computed exactly as a normal
  read would, and the computed conditions and the ordering are applied to that in memory. Numbers
  compare as numbers, text as text, a missing value or an error cell sorts last in either direction,
  and rows that tie keep their stored order. An error cell matches no condition — it is not equal to
  anything, does not contain anything, is never "not empty" — so a broken formula cannot be selected
  by guessing at what it divided by.

- **Such a page is paged by an offset, not by a keyset cursor**, and says so in its `next` value.
  A page computed in memory has no last-row value to resume from, so the cursor carries the number
  of rows already skipped. The consequence is honest and now documented: insert or delete a row
  while paging and the next page can repeat or miss one. The keyset cursor a stored column still
  answers with is unaffected and stays stable.

- **The number of rows a computed filter or sort may walk over is bounded**, at the dataset's row
  cap (50 000). If the stored filters leave more rows than that, the query is refused with
  `too many rows to filter or sort on a computed field` rather than loading the dataset into memory
  to answer it. Filtering on a field of your own page should be a bounded question; this keeps it one.

- **A saved view can name a grouping column** (`group_by`) whatever its kind, for the board-style
  views to come. It must name a column the caller may see and that is stored: a formula or rollup is
  answered per row and has no enumerable set of groups, so `computed fields cannot be grouped`, and a
  relation is `…cannot be grouped yet`. A column hidden from the caller is refused as an unknown one.
  Listing views strips a `group_by` the reader may not see, like every other field a config names.

- **The schema now tells a caller where a relation points.** Every visible `relation` column carries
  `config.target_path` — the target page's current path — beside the `target_doc` id it already
  reported. Binding a cell needs an address, and the stored config only has the id, because the id is
  what survives a rename; so the path is looked up each time the schema is serialized and never
  stored. Move the target and the next read of the schema says where it is now. A column hidden from
  the caller has no path for the same reason it has no column, and a target that has gone to the
  trash takes its column with it, so there is never an address without a readable page behind it.
