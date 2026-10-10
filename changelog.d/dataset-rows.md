### Added
- Dataset rows can be added, read, changed and deleted over the API. Each row carries a version; saving over a newer version is refused and shows the current row. A dataset holds at most 50 000 rows, and row changes notify the dataset's authors who can still read it.
