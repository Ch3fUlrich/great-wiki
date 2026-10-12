### Added

- **A dataset can be read as a gallery of cards.** A saved view of kind `gallery` shows each row as
  a card: its cover picture, its title and the extra columns the view names. The cover comes only
  from an *image* attachment of the dataset page — a PNG, JPEG, GIF, WebP or TIFF the page really
  holds. A PDF or an SVG is never drawn as a picture; a file that was removed or is in the trash
  shows a labelled placeholder (»Datei fehlt«) instead of a broken picture, and a row without a
  file shows »Kein Bild«. The picture's alternative text is the row's title, never the file name.
  A failed read is stated, never drawn as an empty gallery.
