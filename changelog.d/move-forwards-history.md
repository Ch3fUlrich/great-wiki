<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **The history of a moved page follows the page.** `/alt/history` used to answer "page not
  found" after `/alt` moved, though `/alt` itself forwarded. It now redirects (307) to
  `/neu/history`, keeping `?von=`/`?bis=`/`?ansicht=`. The rule is the page's: only somebody
  who may read the page at its new address is told where it went; anyone else gets the same
  missing-page answer as for an address that never held anything.
