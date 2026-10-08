<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Changed

- **Moving a page no longer throws its editors out.** An open editing session used to be
  closed by any move of its page, because the session was checked against the address it had
  joined on. It is now checked against the page itself, wherever it is: an editor who can
  still write the page at its new place keeps typing, and an editor the move shut out (lost
  write, lost read, or the page was deleted) is disconnected at the next check, exactly as if
  the grant had been revoked.
