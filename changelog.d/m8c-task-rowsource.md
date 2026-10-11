### Added

- **The generic board grew the seams the task board needs, and the task board now has a source.**
  Cards render as a `<ul class="karten">` of `<li>`s per column (an empty column renders no list),
  a caller can hang extra attributes on each card and each column — values reach the page escaped —
  and an empty column can carry its own snippet. `TaskRowSource` feeds the three fixed task
  statuses through that same board: the API's cards keep their order and a detached or read-only
  card stays on it, and a move is refused before any request for a card the reader may not write,
  then sent exactly once to whatever `submit` the caller supplies.
