<!--
  A changelog fragment, to be folded into `CHANGELOG.md` under `## [Unreleased]` by the
  commit that assembles this milestone — see AGENTS.md on why this is a file of its own
  rather than an edit to CHANGELOG.md while several agents are working at once, and why it
  is not a line in somebody's report.
-->

### Added

- **Projekte**, at `/projekte` and linked from the header beside »Graph«: the list of every
  project, and the form that starts a new one. A project is made by naming the page it belongs
  to — its Startseite — and that is the whole of it; there is no separate object to fill in,
  because a project *is* a page and the pages beneath it. The path may be typed with or
  without its leading slash, or pasted whole out of the address bar, which is how people
  actually say "this page".
- **The list is the page, deliberately.** The alternative was a »zu einem Projekt machen«
  button on each page — cheaper, and it would have buried the one place you would go to ask
  *which projects exist*. Putting it in the admin console was the other option, and it would
  have made a project something you have to ask an administrator for, which is how a thing
  meant to be used every week ends up used twice a year.
- **Creating and deleting a project work with JavaScript switched off.** Both are ordinary
  form submissions to the server, and the browser's own submit is what carries them; a control
  that only comes alive after hydration is a control that looks live and does nothing.
  Creating comes back as a redirect, so reloading the list does not offer to make the same
  project a second time, and the confirmation afterwards is checked against the list rather
  than against the address bar.
- **A refusal is a sentence, not a number.** Naming a page that is already the home of another
  project says exactly that and says which of the two ways out to take — the project it
  collides with is on the list right there. A page you may not edit says that the write right
  is missing, not that "ein Fehler" occurred; a page that does not exist is named, so a typo is
  visible as a typo. Deleting asks first, names the project, and says what goes with it: the
  cards made on its board, and neither the pages nor the tasks written as lines in them.
- **You see a project only if you may read its home page.** That is decided page by page, by
  the same check a page read goes through, and it is the same check that decides whether you
  may open the board — so the list and the board cannot disagree about what exists. The page
  adds nothing to that answer: no total, no "und 3 weitere", no hint that anything was left
  out. A wiki with no projects and a wiki whose every project is somebody else's read the
  same, and the conflation is the point, because a count would be a fact about pages you are
  not allowed to read.
- Every control on the list is reachable and named — the delete link says *which* project it
  deletes rather than repeating the word "Löschen" down the column — a failed field is marked
  in words and announced as well as outlined in red, and a project with no tag says so instead
  of leaving a cell blank.
