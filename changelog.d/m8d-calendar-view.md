### Added

- **Saved dataset views can now be calendars.** A view saved with kind `calendar` names the date
  column rows are placed by (`date`, required, and only ever a date field — a datetime is the
  same kind with `with_time`, so it counts), an optional end column for spans (`end`, also a date
  field and a different column from `date`), which column titles a row (`title`, a text field;
  left out, the web falls back to the dataset's first text column) and which rows reach the
  calendar (`filters`, `sort`). Everything the table and board views already guarantee holds for
  it: the config only names fields of its own dataset, a hostile key is refused, an unknown member
  of the config is an error rather than a silently ignored setting, and an update is
  re-validated as the calendar it is — a text column cannot become an existing calendar's dates.
  A field the reader may not see is stripped from the calendar's config when they list it, and the
  calendar itself still lists. Reading needs read, saving needs write, an unreadable dataset
  answers as an absent one, and no migration was needed: the schema's `kind` check already allowed
  `calendar`. The other kinds (gallery, timeline, gantt) stay refused until their milestone
  (ADR 0029).
