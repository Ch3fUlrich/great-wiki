<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **Code on a page is shown the way it was written again.** Every fenced block on this wiki
  was being displayed as a single line with its indentation removed — a shell command and its
  output run together, a configuration snippet flattened into one unreadable row, a nested
  structure with nothing left to say what was nested inside what. It had been that way for as
  long as the reading view has existed, and it is the display that was wrong and nothing else:
  the text was stored, edited, exported and re-imported intact the whole time, so every
  affected page corrects itself the moment it is next read. Nothing has to be repaired and
  nothing was lost.

  The cause is worth one sentence, because it will come up again. The reader was asking for a
  block's text through the same function that builds a heading's anchor id, the table of
  contents and a table's column labels — and that function collapses runs of whitespace on
  purpose, which is right for a sentence and destroys a listing. A fence is now read
  separately, exactly as typed.

- **A version that only changes whitespace inside a fence is no longer reported as no change
  at all.** Under »Struktur«, the page history compares blocks by what they say, and it too
  was comparing a fence with its whitespace collapsed — so re-indenting one, or running its
  lines together, produced "Keine Änderungen" in all three tabs for an edit that visibly
  changed the page. Whoever was looking for the version that broke a listing could not find
  it. Such a version is now listed as a changed block, and can be opened and restored like any
  other.

  »Prosa« stays silent about it, deliberately: it compares words, and re-indenting a listing
  adds and removes none. Moving whitespace is a change of structure, and that is the tab that
  now says so. »Gestaltung« is unaffected and keeps answering for its own half — an edit that
  renames a fence's language *and* re-lays-out its lines is now two lines in the history, one
  in each tab, rather than one tab losing the language change to the other one's new
  sharpness.

### Known limitations

- **A fence is still shown as plain, unhighlighted text**, and ` ```mermaid ` is text rather
  than a drawing. The language written after the opening fence has always been stored and
  exported faithfully; nothing yet reads it. That is the next piece of work, and it is what
  made this fix urgent rather than tidy — a diagram whose newlines are gone draws nothing.
