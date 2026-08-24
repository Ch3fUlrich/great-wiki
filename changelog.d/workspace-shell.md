### Added

- **A workspace, with tabs**, at every address the wiki already had. Several things can be
  open at once — a page, another page, the global board, a project's board, `/projekte`, the
  graph, a page's history — and the strip of what is open sits above the panel that shows the
  active one. Which tabs are open lives **in the address**, so a workspace is a link somebody
  can send, it survives a reload and a bookmark, the back button walks through it, and the
  strip arrives complete in the first response rather than after a bundle. A tab **is** an
  address: there is no list of openable things to keep in step, no per-kind payload, and a
  view added later is openable in a tab the day it has a URL.
  Everything in the strip is a plain link — switching, closing, reordering, and »Neuer
  Reiter«, which opens the start page exactly as a browser's own new tab does and lets you
  navigate from there. So all of it works with JavaScript switched off, which is the same
  standard this interface already holds its edit link and its boards to: a control that only
  works once a script arrives is a control that looks live and does nothing. Hydration adds
  the keyboard pattern and nothing else — arrow keys, Home, End and Delete move and close
  within the strip, with the selected tab announced and marked by weight and shape as well as
  colour. Opening a page that is already open switches to it rather than opening a second
  copy, and a long strip scrolls inside itself: the page body still never scrolls sideways.
  When an address carries no tab set — following an ordinary link inside a document, which is
  the one place the URL cannot reach — the last set is restored from the browser and the page
  you landed on **replaces the tab you were in** rather than becoming a new one, so browsing
  inside a tab does not grow a strip. The address bar is then corrected to match, so the
  workspace is a shareable link again immediately. Every read and write of that store is
  wrapped: private browsing, blocked site data and a full quota all render correctly, and
  without a script at all the workspace is simply the one tab the address named, which is how
  this application behaved before it had any.
  A tab set arrives from the address bar and from browser storage, so it is filtered where it
  is parsed rather than where it is rendered: an entry has to be a path inside this wiki, and
  a scheme, a `//host`, a `/\host` or a control character is dropped rather than turned into
  a link wearing this application's chrome. The number of tabs is capped. A tab's name comes
  from the page tree — which is already filtered to what the reader may read — or, failing
  that, from the address they typed themselves, and never from anywhere less careful.

### Changed

- **The interface fills the screen.** It was a centred column with the page tree redrawn
  inside each document; it is now a real application frame — the tree on the left on every
  view, the tab strip and the active panel to its right — sized to the viewport, with the
  panel scrolling on its own so the navigation stays put in a long document. `/graph`,
  `/aufgaben` and `/projekte` had no tree at all and were dead ends you left by the back
  button; they have one now, and it is the same component and the same request, asked once by
  the shell instead of twice by two pages.
  **The reading column is still a reading column.** Running prose stays capped at the measure
  and is now centred rather than pressed against the left edge of a column sized to the
  viewport — this repository already called that "a band of dead space beside it, which reads
  as a bug, because it is one". What fills the room instead is everything that is not a
  sentence: a board, the list of projects, a page's revisions and its diffs, and a table
  inside a document, which now spans the whole reading column around the same centre line
  while the paragraphs above it stay readable. A 200-character line is worse to read, not
  better, and that is why the cap exists.
  The graph is the one view deliberately not widened. Its picture is laid out on the server
  at a fixed width chosen so that one unit is one pixel on screen; stretching it to a very
  wide monitor would scale every label with it and make the whole diagram proportionally
  taller, which is not more graph, it is the same graph further away. Its heading, lede and
  filter fill the view like every other one, and the drawing centres in the room.
- **What is true *about* a page now sits beside it rather than under it.** Its visibility,
  its subpages, what links to it and its outline were stacked below the prose, which put
  »Verweist hierher« below however many thousand words the page happened to contain. They are
  a column of their own on a wide screen, and below the document on a narrow one.

### Fixed

- **Printing a page printed one screen of it.** The application frame is undone for print, so
  a document of any length prints as a document again, without the tree and without a
  column's worth of blank paper down the right-hand side.
