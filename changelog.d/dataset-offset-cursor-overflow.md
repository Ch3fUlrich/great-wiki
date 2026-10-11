<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Fixed

- **A forged page cursor on a computed-field query no longer crashes the request.** Filtering or
  sorting a dataset by a formula or rollup pages by an offset the client sends back; an offset
  near the largest integer overflowed the next-page arithmetic (a 500 in tests, a wrapped bogus
  cursor in release). It is now the empty last page.
