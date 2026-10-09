<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Every page can now be found by the words in its title and its text.** Nothing yet shows a
  search box; this is the part underneath it. A page is findable the moment it is created or
  edited, stops being findable by words it no longer contains, and is found by `Müller` when
  somebody types `muller` (and the reverse). A title counts for more than the same word in the
  body, so a page about diabetes comes before a page that mentions it in passing.

- **Renaming, moving, trashing and restoring a page need nothing from the index.** It keeps no
  address and no state, only words: a page that moved is found at its new address, a page in
  the Papierkorb is not found, and restoring it brings it back. A purge removes the page's
  words from the index with the page itself, so nothing of a destroyed page can be searched
  for or quoted back afterwards.

- **What somebody types is looked for as words, never run as a search language.** `AND`,
  `NEAR(`, `title:x`, quotes and stars are just words to look for, so a malformed or hostile
  query returns "nothing found" rather than an error, and cannot be used to shape the search.

- **Wikis that already hold pages are indexed on the next start.** Existing pages are read
  once, their text is filled in and the index is built; later starts find nothing to do.

### Security

- **The index decides nothing about who may see a page.** What it returns are candidates only,
  reachable from inside the storage layer and nowhere else; the permission check on each one is
  the next change's job and is the only way a hit will leave it.
