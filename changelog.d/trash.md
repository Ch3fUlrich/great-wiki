<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A Papierkorb, so a page can be deleted at all.** Nothing in this wiki could be removed
  before: a page created by mistake, or imported from the wrong folder, stayed. Deleting one
  now takes it out of the tree, out of the graph, off every board and out of the export —
  and keeps it, with its whole history, until somebody says otherwise. Whoever may **write**
  a page may delete it, because deleting is an edit; the page keeps its access rules while it
  sits there, so a page you could not see before it was deleted is not one you can see in the
  Papierkorb, and the entry's page count is the pages **you** may read in it.

- **A page goes to the trash with everything under it, and comes back the same way.**
  Deleting `/handbuch` takes its subpages too, as one entry — "Handbuch, 12 Seiten" rather
  than twelve lines — and restoring it brings back exactly those twelve. Not a convenience:
  a page in the trash whose children are not is a tree with a hole in it, and the hole is
  silent. The children disappear from the navigation and from the markdown export while
  staying readable at their own addresses, so an export would report their files as stale and
  invite somebody to delete them. Because the subtree moves together, deleting needs write on
  **every** page that moves — a subpage somebody has deliberately fenced off cannot be swept
  away by whoever writes the page above it, and the refusal says so rather than failing
  silently.

- **A page thrown away last week is not resurrected by restoring its parent.** Each delete is
  its own entry, and a restore puts back what went down with *that* act and nothing else. A
  page whose parent is still in the trash refuses to come back and names the parent to restore
  first, rather than being put back somewhere it cannot be reached.

- **`endgültig löschen`: a second, deliberate act, for an administrator.** Destroying a page
  is not deleting it, and it is not the same permission: whoever may write a page may put it
  in the trash and take it out again, and destroying it needs **admin on that page** — the
  same check that decides who may publish a page to the open internet. It is the only
  operation in this system that loses anything, and it is the whole reason the trash exists in
  front of it.

- **A purge says what it is about to destroy, by name and by count, before it happens.**
  Every page, listed; and the versions, cards, projects, links and topic filings that go with
  them, counted. The description is not a second query that resembles the deletion — it **is**
  the deletion, run and then rolled back, so the numbers somebody confirms cannot be different
  numbers from the ones that happen. Recorded as
  [ADR 0012](docs/decisions/0012-what-a-purge-destroys.md), including what a purge reaches
  inside a subtree that has been narrowed away from its administrator, and what would make
  that worth revisiting.

### Changed

- **A page in the trash keeps its address.** Nothing else can be created there until it is
  restored or destroyed, and an import that tries says exactly that instead of failing with a
  database constraint on a path it has just been told is free.

### Fixed

- **Every operation refuses to leave a page stranded under a deleted one.** The condition is
  checked inside the transaction that could create it, on the way out of both deleting and
  restoring, and a purge refuses a subtree that still holds a live page rather than destroying
  something that was never deleted. `documents` has carried a `deleted_at` column since the
  first migration, with a comment saying the trash would arrive later; until now nothing wrote
  it, and nothing would have noticed if something had.
