### Fixed
- Losing edit access now ends an open editing session immediately. Before, a collaborator who
  was removed, deactivated, moved out of reach of a page, or whose page was trashed could keep
  typing — and have their changes sent to the other editors — for up to ten seconds, and another
  editor's publish in that window filed those changes in the page's history. A session that is
  only listening is closed at once, and an update is checked against the current permissions
  before it is applied, so nothing from a revoked editor reaches the room or a later publish.
- An administrator who starts viewing as someone else no longer keeps an editing session they
  had open before (it is closed at once).
- A session whose access may just have changed is also sent nothing and relays nothing — no
  document diff, no other editors' keystrokes or cursors — until its access has been re-checked.
