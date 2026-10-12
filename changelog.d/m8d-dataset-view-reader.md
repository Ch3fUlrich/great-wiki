### Added

- **A page that holds a dataset view now shows it.** The `dataset-view` block used to draw its frame
  and nothing else. The page now asks, for each such block (the first eight on a page), whether *you*
  may read that dataset, using your own session and no one else's, and draws the saved view — table,
  board, calendar or gallery — inside the frame when you may. If you may not, or the dataset is gone,
  you see exactly what the author wrote as the label and nothing more: no title, no count, and no
  difference between a dataset you cannot read and one that does not exist. A view embedded in
  another page is read-only (no sorting links, no add-row form, no dragging), and a form view is not
  embedded at all.
