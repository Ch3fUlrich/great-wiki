<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Papierkorb**, at `/papierkorb` and in the header beside »Themen«, »Projekte« und
  »Aufgaben«: everything that has been deleted and that you may see, who deleted it and when,
  and the control that brings it back. The trash existed in the database and in the API and
  nothing could reach either. The place matters more than it looks: a deleted page is out of
  the navigation, out of the export and out of the search, so a "put it back" control that
  lived on the page itself would sit behind an address somebody had to have kept. Deleting
  happens where the page is; recovering happens somewhere you can find without knowing a URL.

- **»Löschen« on the page, beside »Bearbeiten« und »Verlauf«.** It is a link that asks first,
  and the question says the part nobody would guess: the page goes with **everything under
  it**. That is not tidiness — a page left behind under a deleted parent is not merely hidden
  from the navigation, it is unreachable in it, absent from the export, and still readable at
  its own address. So the whole branch moves together and the whole branch comes back
  together. Deleting is offered only where the page says you may write it and you are signed
  in, because the Papierkorb records who emptied a shelf and "nobody" is not an answer. A
  branch containing one page that is not yours to write is refused outright, and the refusal
  says so rather than half-deleting anything.

- **»Endgültig löschen« says what it is about to destroy, by name, before it does it.** Not
  "diese Seite und 3 weitere": every page, with its title and its address, plus what hangs off
  them — versions, cards, projects, links, topic filings, and the topics that no page would
  carry any more. Those numbers are not an estimate assembled beside the deletion; they come
  from the deletion itself, run and rolled back, so the number you confirm cannot be a
  different number from the one that happens. This is the only operation in this wiki that
  loses data, and it is the only one with a confirmation that reads back what will be lost.
  The confirmation also says what *survives* it: the access rights on that path, which belong
  to the path and not to the page, so a page created there later inherits them again.

- **Everything works before any script arrives.** The list is a table, deleting, restoring and
  destroying are ordinary form submissions, and the question before each of them is an address
  you could send somebody. Afterwards the page comes back with the cursor on the sentence
  saying what happened, so it is read out rather than merely drawn.

- **No count of what you were not shown.** An entry says how many pages *you* may see in it,
  and nothing else — no total, no "und 3 weitere". A page you could not see before it was
  deleted is not one you can see in the trash.

### Fixed

- **A control that will be refused is not offered.** »Wiederherstellen« appears only where the
  wiki has already said this entry is yours to put back — the same verdict a restore would
  come back with — and where it does not, a sentence says why rather than leaving a gap that
  reads as a fault. »Endgültig löschen« needs more than a write right: it needs administering
  that page, which is a different permission on purpose, and no answer from the wiki says who
  administers what. So the list offers a *question* and never the act, and the control that
  destroys appears only once the wiki has already agreed to describe the destruction — behind
  exactly the same gate the destruction itself is behind.

- **A link that carries the cursor to a place on the page now actually carries it there.**
  Links in the interface's own chrome had their `#…` folded into the last query parameter, so
  the address still worked and the cursor never moved — which is how a question meant to be
  read out was drawn silently instead. Two of those links additionally ask the browser for a
  real page load, because moving the cursor to a place on the page is something only a real
  navigation does.

- **The header wraps on a narrow screen.** The row of whole-wiki links could not break onto a
  second line, and with the Papierkorb as the fifth of them a phone-width window scrolled the
  whole document sideways.
