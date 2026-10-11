### Added

- **Saved dataset views can now be boards.** A view saved with kind `board` names the select
  column its lanes group by (`group_by`, required, and only ever a select field), which fields
  the cards show (`card_fields`), which rows reach the board (`filters`) and the order within
  (`sort`). Everything the table view already guarantees holds for it: the config only names
  fields of its own dataset, a hostile key is refused, an unknown member of the config is an
  error rather than a silently ignored setting, and a field the reader may not see is stripped
  from the board's config when they list it — the board itself still lists. Reading needs read,
  saving needs write, and an unreadable dataset answers as an absent one, all as before.
  Other future kinds (calendar, gantt) stay refused until their milestone (ADR 0029).
