<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A daily email digest of what happened to you, off by default.** Once an operator turns
  it on, each person with an email address gets at most one German plain-text mail a day
  listing who did what on which page, grouped by kind. It names no comment text, and it only
  lists pages you can still read when it is built: lose access and the line is gone. Nothing
  is sent unless `GW_DIGEST_ENABLED` is `1` or `true`; switched on with incomplete mail
  settings, the wiki refuses to start.
