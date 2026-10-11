### Added

- **Saved dataset views can now be forms.** A view saved with kind `form` carries an ordered
  list of the questions it asks — each a column key, an optional label of its own and an
  optional `required` — plus an optional heading. A form is config like any other view: it
  holds no data, saving one needs write on the dataset, and the columns it names are checked
  against the fields that exist and are visible to whoever saves it. A computed column
  (formula or rollup) cannot be on a form at all: the dataset answers those, not the person
  filling it in. Listing a form for a reader strips the entries whose column has since gone
  hidden for them, exactly as a table's or a board's columns are stripped.

- **Submitting a row through a form.** `POST /api/datasets/rows/{path}` now accepts a body of
  `{values, view}`, where `view` is a form's id, and the store enforces what the form
  promises:
  - **there is no anonymous form.** Submitting needs write on the dataset — the same gate as
    adding a row, so a reader is refused and an unauthenticated caller is told nothing that
    differs from an absent page;
  - **a field outside the form is refused, not ignored.** A key the form does not hold — or
    one it holds but this caller may not see — answers `the form has no field "…"` and no row
    is written. A crafted body therefore cannot reach a column nobody agreed to ask for;
  - **a required field must be answered.** Absent, `null` and a string of nothing but spaces
    are all `… is required`. An optional field may be left out;
  - **the row itself is validated by the ordinary add**, unchanged: same types, same relation
    and file checks, same row cap, same event. Only the two rules above are the form's own,
    and both are asked before anything is written.

  A `view` that is not a form of this dataset — another dataset's id, an unknown id, a table
  or board view — is refused without writing; so is a body whose key would have been a
  problem in a plain row add. A `POST rows` body without `view` is the ordinary add it always
  was.
