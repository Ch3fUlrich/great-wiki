### Added

- **A page holding a dataset view now survives its own backup, and the view is drawn by
  the dataset's own permission.** The owner's Markdown export used to refuse any page that
  held a `dataset-view` block as "newer than this exporter"; it now writes the block and reads
  it back as the same block, with whatever the author's label contains. The new
  `GET /api/datasets/embed/{dataset}/{view}` answers one question — may *this caller* read
  *that dataset* — and never asks about the page the block sits on. A reader who cannot read
  the dataset gets the same two words (`unavailable`) a request for a dataset that does not
  exist gets, byte for byte: no title, no path, no count. A reader who can read it gets the
  saved view's configuration with any column hidden from them removed; the rows still come
  through the ordinary row endpoint under their own identity, so this answer is not a second
  door onto the data.
