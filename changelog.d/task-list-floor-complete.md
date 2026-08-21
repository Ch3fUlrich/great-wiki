<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below.

     Fold this together with `task-list-floor.md`, and read that one first: it announces two
     limitations — "the reader does not draw the checklist at all" and "`great-wiki export`
     refuses the page by name" — that this entry REMOVES. Both were true of the importer
     landing on its own; neither is true of the software as it now stands, and a released
     changelog that states both would be describing a version that never shipped. Drop those
     two clauses from that entry rather than printing this one after them. -->

### Added

- **A checklist is drawn on the page, and every box says whether it is done.** A page holding
  `- [x] Rezept abgeholt` used to show nothing at all where the checklist was — an unknown
  block renders as nothing, which is what keeps unfamiliar content from being emitted raw,
  and it also meant a reader had no way to tell a checklist had ever been written. Each line
  now carries a real checkbox, so the state is announced by a screen reader as "checked" or
  "not checked" rather than being left to whoever can see the tick.

  The boxes are **deliberately not clickable while reading**. A task's state belongs to its
  record on the board and not to the words on the page, and a checkbox wired up here would
  mean needing permission to *edit* the page in order to tick something off — and would file
  a revision nobody typed. Ticking a box will be done on the board, when there is one.

- **`great-wiki export` writes a page holding a checkbox instead of refusing it.** Since the
  importer learned the syntax, every such page had been named in the report and skipped — and
  because one refusal fails the whole run, a single checkbox anywhere would have shut the
  backup path for the entire wiki. Checklists now export as `- [ ] ` and `- [x] ` and come
  back as the same document: mixed lists stay split, a numbered list's plain runs keep the
  numbers they had, nesting keeps its depth, and a bullet whose words merely *look* like
  `[ ] etwas` stays a bullet rather than turning into somebody's new to-do.

### Fixed

- **Opening a page that contains a checkbox no longer destroys it.** This was the serious
  one. The editor builds the document by looking each block up by name, and a name it did not
  know was not skipped — it was **deleted from the shared document**, sent to everyone else
  editing, and saved into the next revision, with nothing shown and nothing logged. So from
  the moment a checkbox could be written, the first person to open that page for editing
  would have silently removed the checklist from it. The editor now knows checklists, keeps
  each box as it was, and keeps the identity a task is tracked by, so nothing is lost by
  opening a page and nothing is lost by editing one.

  Nothing already written was affected: there is not one checkbox line in the content this
  wiki was seeded from, and the editor learned this before any content with a checkbox
  reached it.
