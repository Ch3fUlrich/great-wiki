<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **A code block can no longer take its page out of the backup.** `great-wiki export` writes
  every page as markdown and reads each file straight back before writing it, refusing any page
  that would not come back as the same document — and a fenced code block carrying anything
  beyond its language failed that comparison, because markdown has a spelling for the language
  and for nothing else. The page was then missing from every export from then on, permanently,
  while the directory still carried the note calling itself a faithful copy of the database.
  Nothing in the wiki writes such an attribute; anyone with write access on one page could put
  one there through the editing connection, which is the whole reason this is a fix rather than
  a precaution. The exporter now compares the language and forgives the rest, exactly as it
  already does for a checklist's identity and a link's address — and a fence whose language
  markdown itself could not write back is still refused rather than quietly changed.

  It matters now because diagrams, formulas and coloured listings have just given people a
  reason to write fenced blocks: the same hole had been open since the first code block and
  nobody had a reason to walk into it.
