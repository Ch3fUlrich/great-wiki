<!--
  A changelog fragment, to be folded into `CHANGELOG.md` under `## [Unreleased]` by the
  commit that assembles this milestone — see AGENTS.md on why this is a file of its own
  rather than an edit to CHANGELOG.md while several agents are working at once, and why it
  is not a line in somebody's report.
-->

### Added

- **Every page now has a history you can read, compare and restore from.** »Verlauf«, beside
  »Bearbeiten«, lists every published fassung of the page — newest first, with who wrote it,
  how long ago, what they said they were doing, and how much the page grew or shrank. Until
  now the wiki had thirty-four versions of some pages and no way to look at a single one of
  them.
- **Two versions can be compared three ways, and three is the point.** A **Prosa** diff shows
  which words changed; a **Struktur** diff shows which blocks were added, removed, moved or
  rewritten in place; a **Design** diff shows what changed about how the page looks — a
  heading's level, a table column's alignment, a sentence somebody made bold. A word-level
  diff on its own answers "keine Änderungen" for a page that was plainly restyled or
  reordered, and a history that says nothing changed is worse than no history, because it is
  believed. A block that moved is reported as **one** change rather than as a deletion plus an
  addition, so tidying a page does not read like rewriting it. Additions and removals are
  marked with a word and a symbol as well as a colour, so the diff is legible without colour
  vision, in print, and in a black-and-white screenshot.
- **Any version can be read as a whole file**, in the same three files an export writes: the
  markdown, the metadata, and the block tree the database actually holds. When a version
  cannot be written as markdown faithfully — an image, a link the tree cannot express — it
  says so rather than showing a quietly lossy file.
- **Restoring publishes the old version as a new one and deletes nothing.** What you restored
  past is still in the history afterwards, so the restore is itself undoable — by restoring
  the other one. It asks first, and the question names the version and says what happens to
  the current one.
- Reading a page's history needs exactly the right that reading the page needs, and nothing
  more; restoring needs write, which is never implied by being able to read. A history is not
  metadata about a page — it says the page exists, who works on it and what every earlier
  draft said — so every one of these answers is filtered by the same permission-checked
  accessor a page read goes through.
