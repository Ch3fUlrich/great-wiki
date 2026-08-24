<!--
  A changelog fragment, to be folded into `CHANGELOG.md` under `## [Unreleased]` by the
  commit that assembles this milestone — see AGENTS.md on why this is a file of its own
  rather than an edit to CHANGELOG.md while several agents are working at once, and why it
  is not a line in somebody's report.
-->

### Added

- **Boards, tasks and projects can now be reached over the API**, which is what the interface
  is built on next. A project is made by naming the page that is its home; its board is that
  page's subtree plus the loose cards filed under it. A card can be created on a board,
  moved, given a due date, handed to somebody, and thrown away.
- **Three columns, on every board, always: Offen, Läuft, Fertig.** A board answers with all
  three even when two of them are empty — "nichts läuft gerade" is something a board has to
  be able to say. A status that is not one of the three is refused, and the refusal names the
  three rather than quietly filing the card as *Offen*, which would silently reopen something
  somebody had finished.
- **Moving a card changes the card and nothing else.** No page is rewritten, no version is
  filed, and nobody needs permission to *write* a page in order to drag a card that came from
  it into another column. The page owns the words; the record owns the state.
- **A card whose line has been deleted from its page stays on the board, marked.** It is not
  quietly thrown away with the due date and the assignee somebody set on it, and the board
  says which cards are in that state rather than showing them as if they were still written
  somewhere.
- **You may only hand a task to somebody who can open the page it is on.** Doing otherwise
  would give them an obligation they cannot see — and the card's title would tell them what a
  page they may not read is called. The refusal says so and says what to do about it, and it
  names nobody. Taking a name *off* a card is always allowed, so somebody who has since lost
  their access can be cleared rather than staying stuck to it for ever.
- **A board shows only the cards whose own pages you may read** — decided page by page, not by
  the project. One project deliberately spans pages with different access, so a board that
  trusted the subtree would hand over the very words a restricted page was keeping. Nothing
  in the answer counts what was left out either: no total, no "und 3 weitere", no identifier
  for a card that was filtered away, and a board you may not see answers exactly what a board
  that does not exist answers.
