### Added

- **Being given a to-do, a task falling due, an accepted invitation and a changed grant now
  reach the event bus.** Handing a card to somebody else records that they were given it
  (handing it to yourself tells nobody), and a sweep, run on a timer in a later change,
  records each assigned, unfinished card due within a window — once per card and due date,
  however often it runs. When an invitation is taken up, or a grant is added or removed, the
  people who administer that path are recorded as having something to hear about.

  Recording is not telling. Nothing is checked when an event is written; every list and
  count asks again, for the reader and at that moment, so an administrator who has since
  lost the path, or somebody who may not read the page, sees nothing and learns nothing
  ([ADR 0024](docs/decisions/0024-the-event-bus-records-who-might-hear-and-asks-again-at-delivery.md)). A failure to record is logged and never
  fails the assignment, invitation or grant itself. "Your tasks" means the ones assigned to
  you: a card does not remember who created it.
