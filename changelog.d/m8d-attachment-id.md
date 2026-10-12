### Added

- **A page's attachment list now names each file's id.** A dataset `file` column stores the
  attachment's id, and until now no answer carried it, so a gallery cover could not be set by any
  client. The id is minted when the file is attached (it is not derived from the content, so it says
  nothing about where else the same bytes are held), appears in the attach answer and in the list a
  reader of the page gets, and a caller who may not read the page still gets the same refusal as for
  a page that does not exist.
