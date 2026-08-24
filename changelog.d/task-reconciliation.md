<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A checkbox written in a page now becomes a card, and stays the same card.** Publishing a
  page reconciles its checklist lines against the task records: a line nobody has a record
  for gets one, a line whose words changed updates the record's title, and a line that is
  gone leaves its record behind with a marker. It happens inside the same transaction that
  writes the revision, so there is no moment where a page says one thing and the board says
  another, and a publish that fails leaves neither.

  The words come from the page and nothing else. Status, assignee, due date and the card's
  place in its column live on the record and are never written by a publish — which is what
  lets a card be dragged without filing a revision nobody typed, and stops the next save from
  quietly undoing the drag. A ticked box is read exactly once, when the record is first
  created, so a checklist imported from markdown arrives with its finished lines finished.
  After that the record decides, and a page's own checkbox is a stale copy that publishing
  ignores.

- **A to-do that loses its line is marked, not deleted.** Delete the line and the card stays
  on its board saying the page no longer mentions it — with the due date and the assignee
  somebody put on it still there. Retyping the line makes a *new* to-do and leaves the old one
  visibly detached, rather than one task quietly turning into another. Putting the same line
  back — an undo, or restoring an older version of the page — re-attaches the card it had,
  with its state, instead of starting a second one beside it.

- **Identity is minted by the store, once.** A checklist line acquires an invisible id the
  first time its page is published, and that id is stored with the page, so publishing the
  same page again finds the same to-dos rather than a fresh set. A line that arrives with no
  id — everything imported from markdown does, and `seed --update` re-converts the same file
  on every run — adopts the record for its words rather than minting a second one. Without
  that, every save would shed every card on the page, with its dates and its assignments, and
  nothing would have gone wrong loudly enough to notice. Two lines reading the same words keep
  two records, and a checklist copied and pasted in the editor becomes a to-do of its own
  rather than a second line sharing the first one's card.

  One rough edge, and it needs its own fix rather than time: `seed --update` decides whether a
  file changed anything by comparing the stored block tree against the freshly converted one,
  and a markdown file cannot carry those ids. So a page holding a checkbox now looks changed on
  every run and gets a revision that says nothing — the thing that comparison exists to
  prevent. The cards themselves are unharmed (they are found again by their words, which is
  what the adoption above is for), pages without checkboxes are untouched, and there is not one
  checkbox line in the content this wiki was seeded from. The fix is for the comparison to
  ignore a task item's id the same way `export` already does.

- Creating a to-do this way needs exactly what changing the page needs and nothing weaker:
  reconciliation runs behind the same write check publishing has always made, asks no second
  question of its own, and a reader whose publish is refused causes no records at all. It also
  keeps its hands off every record it did not write itself: only a to-do that came from a line
  is one that a line can disappear from under.
