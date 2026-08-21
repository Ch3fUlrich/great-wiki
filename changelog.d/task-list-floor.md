<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A checkbox written in a page is a checkbox now.** `- [ ] Stuhlprobe einschicken` used to
  come back as an ordinary bullet whose *words* were "[ ] Stuhlprobe einschicken": the
  brackets sat on the page, went into the search index, and would have gone into the anchor
  of any heading written that way. The importer never saw a checkbox at all — the syntax was
  switched off, so there was nothing to lose and nothing said anything had been lost. A
  checkbox line now imports as a checklist, ticked or unticked exactly as it was written,
  under the same names the editor uses for one.

  This is the first of the layers a to-do board needs and **only the first**, so two things
  are true of a page holding a checkbox until the rest land: the reader does not draw the
  checklist at all — an unknown block renders as nothing, which is what keeps unknown content
  from being emitted raw — and `great-wiki export` refuses the page by name rather than
  writing a file that would come back a different document. Nothing existing is affected:
  there is not one checkbox line in the content this wiki was seeded from.

### Changed

- **A list that mixes checkbox lines with plain ones stays mixed.** It comes back as a
  checklist and an ordinary list side by side, in the order written, rather than as one list
  with every line turned into a to-do. In this wiki a checkbox line *is* a to-do, so an
  unticked box invented on a line nobody marked would put an item on a board that nobody
  wrote — which is the one cost this design was weighed against and accepted on the grounds
  that it does not happen.
- A **numbered** list holding checkboxes keeps the checkboxes and loses the numbering on
  those lines, because a checklist has no numbers. That is now reported with everything else
  the import could not carry, instead of changing the page quietly. The plain lines of such a
  list keep the number they had, so nothing renumbers behind your back.
