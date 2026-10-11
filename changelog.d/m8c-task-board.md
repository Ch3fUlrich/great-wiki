### Changed

- **The task board is one rendering on top of the generic board.** `/aufgaben` now mounts
  `TaskBoard`, which is the same three columns, the same cards and the same words, but built on
  the board component a dataset table also uses rather than on its own copy of a grid. Nothing
  about the board changed for a reader: the heading and the level it sits at, the line naming
  the project, the sentence a move answers in, »Nichts hier.« in an empty column, a card's
  source page, its due date in words, who it rests on, why it is read-only, and the form that
  moves it are all still here and still work without JavaScript. The board's own test suite is
  unchanged but for the file it imports, which is the point: the assertions were carried over
  as the specification, not rewritten to fit the new parts.
  Two small consequences of the new parts are visible. A card you may not move is now slightly
  faded as well as marked in words, and the board carries a status line of its own under the
  columns, which is where a move that could not be completed says so. Dropping a card onto the
  column it already holds still does nothing, silently, exactly as before.
