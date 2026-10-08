### Added

- **An edit by someone else to a page you wrote or last edited now reaches the event
  bus.** When a revision is published, or an older one restored, the author of the page's
  first revision and the author of the revision just before it are recorded as having
  something to hear about. Your own edits tell you nothing, and a run of edits to one page
  keeps a single row, bumped to the latest editor and unread again, rather than one per
  save. Pages first written by an import have no account as writer, so only the last editor
  is told. Nothing of the edit's text is stored, and whether a recipient may still read the
  page is asked when they look, not when the edit is made
  ([ADR 0025](docs/decisions/0025-the-event-bus-records-who-might-hear-and-asks-again-at-delivery.md)).
  A failure to record is logged and never fails the save.
