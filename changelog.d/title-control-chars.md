<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **A page title cannot carry control or invisible formatting characters.** Creating or
  renaming a page with a line break, a tab, a right-to-left override or a zero-width
  character in its title is refused with a reason, instead of storing a title that reads
  differently from what it contains. Ordinary letters, umlauts and punctuation are unchanged.
