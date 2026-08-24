<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **One board, showing every to-do you may see** — across all your projects and the pages
  that belong to none. Until now a card could only be found on the board of the project it
  was filed under, which left the ones written on a page nobody had claimed with nowhere at
  all to appear: exactly how a to-do goes missing. It can be narrowed to one project, or
  asked for by the page a project is homed on — so the board shown *on* a project's page and
  the board you open on its own are the same board, filtered, rather than two things that
  will one day disagree with each other. Three columns, always: Offen, Läuft, Fertig, and a
  card whose line has been deleted from its page still appears, still marked.

  Narrowing it to a page that is nobody's home gives you an **empty** board, not the whole
  wiki's. That is what an ordinary page's board is — nothing — and it is the answer a page
  has to be able to give without knowing in advance whether it is a project's home.

### Security

- **The widest view in the wiki is filtered exactly like the narrowest one, by the same
  query.** A board card says that a page exists, what it is called, and — because a card's
  words are the page's own — what somebody wrote on it. A view over *every* task there is
  would be the easiest place in the system to lose that filtering, so it is not a second
  query that could lose it: it is the project board's own query with the project left
  unnamed, and every card on it, without exception, is admitted by asking the same question
  a page read asks, about that card's own page.

  One consequence is worth stating because it is the thing that would have been got wrong. A
  card created **on a board** belongs to no page, so nothing about it can be checked against
  one — its project's home page is the only thing that decides who may see it. A board bound
  to a single project already knew the answer for all of them, having been let in at that
  home page a moment earlier; a board bound to nothing spans every project and can assume
  nothing. It asks about each. Keeping the shortcut would have handed over the loose cards of
  every project whose home page you may not open, and it would have looked correct, because
  for one project it is.

  The response carries no total, no "und 3 weitere" and no identifier for anything left out,
  and a project you may not reach answers exactly what a project that does not exist answers.
