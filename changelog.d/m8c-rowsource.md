### Added

- One board component can now serve both datasets and tasks: rows arrive through a `RowSource`
  seam, and dataset rows are already exposed as columns, cards and moves — a drag is one PATCH
  of the grouping select field, refused without a request when the reader may not write, and
  answered with the row as it now stands when the version went stale.
