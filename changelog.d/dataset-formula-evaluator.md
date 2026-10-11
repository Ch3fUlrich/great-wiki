<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A dataset formula is a language with a ceiling, not a script.** `gw_core::formula` parses
  and evaluates the expression a `formula` field carries: numbers, text, `true`/`false`/`null`,
  a field reference written `{key}`, `+ - * / %`, the six comparisons, `and or not`, and
  `if(c,a,b)` `concat` `round` `min` `max` `abs` `len` `today()` `coalesce`. Nothing else —
  no loops, no calls between formulas, no way to reach the database. It is pure, so the whole
  language is testable without a server, and the evaluator takes its clock as an argument:
  `today()` answers with the date the caller injects, which is what keeps a formula's result a
  function of the row alone rather than of the moment it was read.
  - A formula is refused **when it is saved**, not when a row is read: a `{key}` naming a field
    this dataset does not have never gets stored. The keys it does read are listed for the
    dependency graph, which is where a cycle between two formula fields will be caught.
  - Three limits, all of them far above anything a real column needs: 256 nodes of syntax,
    10 000 steps per row, 32 levels of nesting. Over any of them the expression is rejected
    outright, and a hostile one — a hundred thousand open brackets — is refused at the limit
    rather than by running out of stack.
  - Anything that goes wrong at read time becomes a cell, not a failure: dividing by zero,
    adding text to a number, a result too large to be a number. The cell reads
    `#ERR(divide by zero)` and is drawn as plain text — a broken formula costs one ugly cell in
    one row, never a page that 500s or markup from a value nobody typed into the editor.
