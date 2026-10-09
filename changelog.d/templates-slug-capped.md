<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **A new page's address segment is capped at 100 characters**, as its title is at 200. A
  long `slug` could otherwise make a path of any length; a slug of only punctuation is
  refused like an empty title.
