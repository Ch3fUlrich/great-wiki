### Fixed

- **Digest lines can no longer be forged by a title, path or name with a line break.** A
  page title, page path or actor name containing a newline, carriage return, tab, ANSI
  escape or Unicode line separator now reaches the plain-text mail with each such character
  replaced by a space, so one event still yields exactly one digest line.
- Invisible bidirectional and zero-width format characters (U+200B..=U+200F, U+202A..=U+202E,
  U+2060..=U+2064, U+2066..=U+2069, U+FEFF) are also replaced by a space to prevent
  text reordering or hiding in plain-text mail. The digest's mail library is lettre 0.11.23 (MIT).
