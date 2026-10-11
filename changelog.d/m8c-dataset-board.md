### Added

- **A dataset page can show a saved board view.** The page now lists the dataset's saved
  views above its body — »Tabelle« plus one plain link per view, chosen by `?view=…` in the
  address alone, so the bar works without any script and a link can be handed to somebody.
  Opening a board view renders the same rows as the generic board the task boards use, in
  columns named by the view's select field, with an extra »Ohne Wert« column last for rows
  that carry no value; the board asks for all its cards at once (five hundred) and ignores
  the address's table sort and filter, which are not its questions. Where the caller may
  write the page, a card drags between columns and the drop saves through the same row
  endpoint the row editor uses; where they may not, the cards are shown without any move
  control at all.
- **Nothing about views can break the page that was there before.** If the views list cannot
  be read, the bar simply shows nothing and the table loads as ever; an unknown `?view=` id
  is the table, not an error; and a board whose group field is missing or is not a select
  field says so in words instead of drawing an empty board — as does a failed row read.
