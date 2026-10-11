### Added

- **A page can now point at one saved view of a dataset.** A new `dataset-view` block carries
  the dataset's page id, the view's id and the author's own label — nothing else. It is a
  *reference*: no row is ever copied into the page that holds it, so the rows a reader sees are
  fetched when the page is read, through the same permission-checked door a dataset read itself
  ends in, and reading the embedding page never grants reading the dataset. A reader who may not
  read the dataset sees the frame and the label, and not even how many rows there are. In
  Markdown it is written the way an embed is — `![Laborwerte](datensatz:<id>#<id>)` alone on a
  line — and a destination that is not two ids stays an ordinary broken picture rather than
  becoming a half-made block, which is what keeps an existing page exportable. Only the storage
  and the round trip exist with this change; the editor's control, the collaborative document
  and the reader's rendering are the commits that follow it (M8 D1b–D1d), and until the editor
  knows the node, a page that gets one from the editor loses it.
