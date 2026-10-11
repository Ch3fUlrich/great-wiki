<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A dataset can name a person, and can hold a file.** `person` and `file` were columns a
  dataset could not have: the schema endpoint refused them the way it refuses a rollup. Both are
  now storable, and each carries the question its kind implies — *could this person see this
  page*, *is this file one of ours* — asked of the database before anything is written. A
  `person` cell holds a principal id, `file` holds one attachment id, and `relation`/`rollup`
  still wait for their milestone.
  - **A person you cannot see is not a value you may write.** Naming somebody in a row is a
    statement about them in a document they might read, so the write is gated by the same clause
    that gates assigning a task (ADR 0009 clause 2): the account must exist **and** must be able
    to read this dataset's page, and the question is asked *as them*, not as the writer. Writing
    a row needs write on the page; naming somebody who has only read is fine, naming somebody
    with nothing is not. `{"multiple": true}` names several, each answered for itself, up to 50.
  - **The refusal says nothing about whether the account exists.** "Nobody by that id" and
    "somebody who may not read this page" get the same sentence, the same `invalid_values` field,
    the same work done — otherwise a dataset becomes a way to ask, one cell at a time, whether an
    account exists and which pages it has been given. `person is not available for this dataset`.
  - **Clearing a person asks nothing of the person it drops.** Null or `[]` always writes: a
    row that named somebody who has since lost read must stay editable, and *removing* your name
    is not a claim about you. A patch that does not mention the column rechecks nothing either —
    the stored value was checked when it went in, and unrelated edits stay cheap.
  - **A file cell is one of this page's attachments, and reads back as a name.** Writing an
    attachment id is refused unless the row and the file live on the same page, so a dataset on a
    public page cannot point at a scan filed on a private one — the file link is fetched
    through the page that holds it, and this must not become a second way to reach it. Reading a
    row then decorates the id, in `{"id": ..., "filename": ...}`, at the same read-time hook a
    formula is evaluated at. Detach the file and the cell answers `{"id": ..., "missing": true}`:
    the id stays because a reader of the page may still list its attachments and see which one
    went, and a deleted file never turns a row into an error or a list into a 500. What is stored
    stays the id alone; a filename is a read-time answer, so renaming or detaching needs no row
    rewrite. A `person` cell reads back as the ids it holds — resolving names is the board's problem.

### Changed

- **Core validates shape, the store answers questions.** `validate_value` for `person` and `file`
  checks the form only — a non-empty id of at most 128 bytes, an array of distinct ids for a
  multi-person cell — and returns it. It has no database to ask, and pretending otherwise is how
  a check that looks absolute ends up being the only one. Two mutation entries guard the store
  side: removing the read check, or letting a file name another page's attachment, must fail a
  test (`./scripts/mutate.sh person`).
- **Both kinds take a config, and only their own.** A `person` column accepts `{}` or
  `{"multiple": true|false}`; a `file` column accepts `{}` and nothing else. Anything else is
  refused when the column is saved rather than silently ignored, so a typo cannot quietly make a
  single-person column into a list.
