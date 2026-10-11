<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A dataset can carry a formula column, and reading a row answers it.** A `formula` field is
  now added through the schema endpoint like any other column (needs write; `person`, `file`,
  `relation` and `rollup` still wait for their milestones). Its config is exactly
  `{"expr": "..."}` — one string, at most 1000 characters — and the expression is parsed when the
  column is saved, so a `{key}` that names no field of the dataset is refused then, not when a
  row is read. A formula may read any other column, including a rollup and other formulas.
  - **A formula is evaluated at read and never stored.** The row in the database holds only what
    somebody typed; opening or listing a row adds each formula's cell to what is returned. A
    formula that reads another is evaluated in dependency order, so `{a} * 10` sees the value of
    `{a}`, not an empty cell. Anything that goes wrong at read — a divide by zero, text added to
    a number, a rollup that has no value yet — becomes one `{"error": "#ERR(reason)"}` cell, the
    same way the evaluator renders a failure: never a panic, never a lost row, never a stored
    value. Writing a value to a formula column is still refused.
  - **Cycles are refused when the column is saved.** Self-reference, two formulas that read each
    other, and longer loops are caught by walking the dependency graph — iteratively, so a hundred
    formula columns cannot grow the stack — and answered `formula cycle: a -> b -> a`. A formula
    reading a stored or rollup column is never a cycle, so those edges are ignored.
  - **A filter or a sort on a formula is refused for now** (`computed field \`x\` cannot be
    filtered or sorted yet`), because its value is not in the row SQL can look at. Reading it is
    what this step delivers; ordering and searching by it come next.
