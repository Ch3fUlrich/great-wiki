### Added
- Dataset cell values are now checked by kind before they can be stored: finite numbers,
  real dates, web or mail links only, known select options, de-duplicated tags, no unknown
  columns, and rows capped at 64 KiB. Person, file and relation cells are not yet accepted.
