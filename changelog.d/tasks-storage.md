### Added

- **To-dos and boards now have somewhere to live.** A task is one record with an optional
  anchor to the line in a page that authored it, so a to-do written while planning and a card
  created on a board are the same kind of thing rather than two that have to be kept in step.
  The workflow state — status, assignee, due date, position — is on the record; for a task
  written into a page the words stay in the page, which is what lets a card be dragged without
  filing a revision nobody typed. The three columns are fixed and built in — **Offen, Läuft,
  Fertig** — and the database itself refuses a fourth, so a status this software does not
  understand cannot be written by anything, including a repair script.
- **A project is a home page and the pages beneath it.** Creating one is an act on that page:
  whoever may write it may make it the home of a project, and the board reads back the tasks
  written into its pages alongside the loose cards filed on it directly.

### Security

- **A board card is a disclosure and is treated as one.** Every card, every project in a
  listing and every task read back by itself is filtered through the same permission check a
  page read uses, per document — never once for the whole subtree. A project is deliberately
  allowed to span pages with different access, so a card on a page you may not open is not
  shown, not greyed out and not counted: any of those would say that the page exists, and the
  card's title is that page's own words.
- **Who may assign whom is now answered rather than left open.** You may create or change a
  task, including putting somebody's name on it, if you may **write** the page that governs
  it — its own page, or its project's home page. You may only assign it to somebody who may
  **read** that page: assigning a colleague to a task on a page they cannot open would create
  an obligation they can never see, and would tell them what a page they may not read is
  called. Clearing a name needs only the write, so an assignee who has since lost their access
  can still be taken off the card — otherwise revoking somebody's access would pin their name
  to it for good. Moving a card to another board is governed by both boards, and refuses to
  carry an assignee onto a page they may not read.
- **Purging a page destroys its cards with it**, the same way it already destroys its history
  and its editing state. A card holds a copy of the page's words; leaving it behind would keep
  restricted text on a board after the page and the access rules protecting it were gone.
  Deleting the *line* is a different matter and does not lose the record — that is what the
  detached marker is for, and it lands with the editor work that reads it.
