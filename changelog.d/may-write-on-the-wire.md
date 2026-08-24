### Added

- **A page now says whether you may edit it, before you press anything.** Every control that
  needs write — »Bearbeiten«, »Neues Projekt«, »Löschen«, and moving a card between columns —
  was offered to whoever was signed in, and the real answer only arrived as a refusal once
  somebody had already pressed the button, or in the case of an edit, once they had typed
  into it. That was never a decision anybody made; it was the only thing the interface could
  do, because nothing in what it was told said "you may write this". Five separate pieces of
  work wrote that down and left it alone, each correctly refusing to guess.

  Now the answer travels with the page, the project and the card, so a control can be offered
  to the people it will actually work for, and a to-do you may see but may not move can say
  so rather than simply losing its buttons.

  **It is the same answer that refuses you, not a second one that agrees with it today.**
  That distinction is the whole of the work. A separately worked-out "can I write this" is a
  second opinion, and a second opinion can drift from the one that decides — at which point
  the interface either offers somebody a thing that is then refused, which is where we
  started, or hides a control from somebody who was entitled to it, which is worse because
  nobody reports it. So the verdict falls out of the permission check the page read already
  performs: the same rule, on the same grants, asked one question further along. Nothing is
  looked up twice and no extra work is done for it — a board of forty cards across four pages
  still asks four times, and there is a test that counts the queries and fails if that stops
  being true.

  It answers about pages you can already open, and nothing else: a page you may not read
  still refuses you outright and says nothing at all, and a card whose page is closed to you
  is still absent from the board rather than shown greyed out. What is new is that you are
  told your own reach before you spend effort on it instead of after — you could always have
  found out by pressing. Who is told, and where this bit stops short of a promise, is
  [ADR 0010](docs/decisions/0010-telling-the-caller-whether-they-may-write.md).

  One place it deliberately stops short: filing a version of a page also needs you to be
  signed in, because a version records who wrote it. On a page shared by link, that means an
  anonymous visitor with the right to edit really can edit and really cannot publish — which
  was already true and is now written down where the next person will find it.
