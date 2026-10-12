### Added

- **A dataset can be read as a month calendar.** A saved view of kind `calendar` places each row on
  the day of its date field, in a Monday-first month grid with previous/next links
  (`?view=…&month=YYYY-MM`). A plain date stays on its own day whatever zone the reader is in; a date
  with a time is placed on the *reader's* local day, so a late-evening entry in Berlin does not
  move a day for someone in Los Angeles. A row with an end date appears on every day of its span;
  an end before the start is treated as a single day rather than dropped. Rows without a date
  are not lost: they are listed under »Ohne Datum« below the grid. A failed read is stated, never
  drawn as an empty month.
- **A view's `title` can no longer be mistaken for a heading.** The store decides whether a
  saved view's `title` names a field (calendar, gallery) or is the form's own heading by the
  view's kind, not by guessing from the shape of its config, so a field hidden from a reader is
  stripped from a calendar or gallery they list, and a form's heading is never touched.
