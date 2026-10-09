### Fixed

- **Digest lines can no longer be forged by a title, path or name with a line break.** A
  page title, page path or actor name containing a newline, carriage return, tab, ANSI
  escape or Unicode line separator now reaches the plain-text mail with each such character
  replaced by a space, so one event still yields exactly one digest line. The digest's mail
  library is lettre 0.11.23 (MIT).
