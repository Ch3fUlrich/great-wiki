### Added

- **Notifications can be read over HTTP.** `GET /api/notifications` lists the newest events
  the caller may still see, `GET /api/notifications/unread-count` gives the badge number,
  and `POST /api/notifications/{id}/read` and `/api/notifications/read-all` mark them read.
  Whether the caller may still read the page an event is about is asked when they look, so a
  reader whose access was removed sees an empty list and a count of 0, and an event that is
  someone else's, withheld, or does not exist all answer the same 404. The responses carry
  no total or hidden-count, and anonymous callers get 401
  ([ADR 0024](docs/decisions/0024-the-event-bus-records-who-might-hear-and-asks-again-at-delivery.md)).
