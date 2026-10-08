<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Changed

- **`just behaviour` now drives the move dialog in a browser (Group S).** It checks the
  keyboard path, that a move opening a restricted page to everybody shows who gains and has no
  confirm button, that a narrowing move is confirmed and its old address (and the old history
  address) answers 307 to a reader and 404 to a stranger, and that dragging a page onto another
  in the sidebar opens the prefilled dialog without moving anything. The fixture seeds two
  extra pages for it from `web/scripts/behaviour-extra`.
