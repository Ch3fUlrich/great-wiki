<!--
  A changelog fragment, to be folded into `CHANGELOG.md` under `## [Unreleased]` by the
  commit that assembles this milestone — see AGENTS.md on why this is a file of its own
  rather than an edit to CHANGELOG.md while several agents are working at once, and why it
  is not a line in somebody's report.
-->

### Added

- **Aufgaben**, at `/aufgaben` and linked from the header beside »Projekte« and »Graph«: every
  task you may see, in three columns — Offen, Läuft, Fertig — narrowable to one project. The
  columns are fixed and the same everywhere, so a card's column means the same thing on
  whichever board you meet it.
- **The same board also sits on a project's own home page**, below the page and above its
  subpages. That is where you look when you are thinking about that project, and being sent
  somewhere else to see its tasks breaks exactly that. The global board is still the only
  place a task belonging to no project appears at all, which is why both exist.
  The two are **one board with a filter, not two boards**: one request to the server, one
  rendering, one way to move a card. Two implementations would be two answers to "which tasks
  exist", and since a card says that a page exists and what somebody wrote on it, a second
  answer is a second chance to disclose one.
- **A card can be moved without a pointer.** Every card carries a named button for each of the
  two columns it is not in — "»Kabel bestellen« nach Läuft verschieben", not "Verschieben"
  repeated down a column — and pressing one is an ordinary form submission to the server. It
  works with JavaScript switched off, and afterwards the page comes back to the board the card
  was moved on, filter and all, with the cursor placed on a sentence saying what happened, so
  the move is read out rather than merely drawn. Dragging a card does the same thing by
  pressing the same button: it is an addition, never the only way in.
- **A card you may see but may not move is shown, and says so.** Hiding it would hide nothing
  — if it came from a page, its checkbox is on that page for anyone who may read it — and a
  task that quietly vanishes from a board is the failure this whole design exists to prevent.
- **A card whose line has been deleted stays, marked »Abgelöst«**, and the marker says what is
  still true of it: the page no longer holds the words, and the due date and the person it
  rests on are still somebody's. That is the whole reason such a card is kept rather than
  discarded, so it is what the card says rather than a grey badge to be interpreted.
- **Due dates are shown, and an overdue one says "Überfällig seit …" in words.** The colour is
  the second channel, never the only one — the same line this project holds in the diff views
  and the sortable tables. A date with no time is a whole day, so a task due today is due
  today until the day ends rather than overdue from one second past midnight.
- **A card names the page it was written on and links there**, or says it was made on the
  board and belongs to no page. Both are facts about the card; neither is a blank.

### Security

- **Both boards answer from one permission-filtered request, and neither adds to the answer.**
  You see a card only if you may read the page that governs it — decided page by page, by the
  same check a page read goes through — and the interface renders exactly the cards it was
  given: no total, no "und 3 weitere", no hint that anything was left out. A number on an
  aggregate view is a fact about pages you are not allowed to read, and it is the one thing
  the filtering cannot take back. A wiki with no tasks and a wiki whose every task is somebody
  else's read the same, and the conflation is the point.
- A project id typed into the address bar is matched against the projects you were actually
  shown before it is used as a filter, so the address bar cannot become a second way to ask
  whether a project exists; an id that matches nothing shows the whole board and says the
  filter was not applied, without confirming or denying anything about it.
- After moving a card the browser is sent back to where the move was made — and only ever to
  a page of this wiki. The address is carried in the form, so it is whatever anybody put
  there; anything that is not a path here is refused rather than repaired.

### Fixed

- A failed request for the tasks is never reported as a board with nothing on it. On the
  global board it says so plainly; on an ordinary page it says only that *if* a board belongs
  here it could not be loaded — because a request that failed cannot tell a project's home
  page from any of the other pages in the wiki, and claiming one either way would be inventing
  the half it never learnt.
