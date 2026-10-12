### Added

- **Saved dataset views can now be galleries.** A view saved with kind `gallery` names the file
  column its cards are covered by (`cover`, required, and only ever a file field — which of those
  attachments is an image is decided when the row is read, by the attachment's media type, not by
  the config), which column titles a card (`title`, a text field), which fields show under the
  title (`card_fields`, any stored column; a computed one is refused) and which rows reach the
  gallery (`filters`, `sort`). Everything the table, board and calendar views already guarantee
  holds for it: the config only names fields of its own dataset, a hostile key is refused, an
  unknown member of the config is an error rather than a silently ignored setting, and an update
  is re-validated as the gallery it is — a text column cannot become an existing gallery's cover.
  A field the reader may not see is stripped from the gallery's config when they list it, and the
  gallery itself still lists. Reading needs read, saving needs write, an unreadable dataset
  answers as an absent one, and no migration was needed: the schema's `kind` check already allowed
  `gallery`. Timeline and gantt stay refused until their milestone (ADR 0029).
