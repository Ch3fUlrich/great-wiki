### Added

- **A generic board now renders any `RowSource`.** Dragging and the "move to" select go
  through one pure `applyMove`: the same column is a no-op that never asks the source, a card
  the source won't move is never asked either, and a refused, failed or stale move leaves the
  card where it was and says why in a status line — a stale one in words and through an
  `onStale` hook so the caller can reload. Card titles reach the page as escaped text.
  `card`, `actions`, `columnHead` and `empty` snippets are the extension points the tasks
  adapter will render through; a read-only card is still shown, marked as such, with no
  drag handle and no move control.
