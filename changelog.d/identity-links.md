### Added

- **A link to another page of this wiki survives that page being renamed or moved.** Press
  »Link« while writing, and a dialog opens with both things a link can point at. The upper
  half is a **list of the pages you may read**, searchable by title or address: choosing one
  records *which page it is*, not where it currently sits. The lower half, **Adresse**, is the
  free field for everything else — an address on the open internet, an e-mail address, or a
  wiki path somebody sent you; a path that names a page you may read is exchanged for that
  page's identity when you publish, so it becomes rename-proof too. Either way the reader is
  sent to where the page is **now** and the link carries the page's **current** name as its
  tooltip, worked out afresh on every read rather than frozen into the text when it was
  written.

  Until now this could not be written at all: everything stored an address, so moving a page
  quietly broke every link into it — a breakage you only discover by clicking. Renaming and
  moving pages is not something this wiki can do yet, which is exactly why the links have to
  be ready before it can: the alternative is rewriting everybody else's pages when one page
  moves, filing a revision on each, needing write access to all of them, and leaving the tree
  half-rewritten when one of those is refused.

  **A link never shows you a page you may not read.** If the page it points at is one you have
  no access to — or it has been thrown away, or purged, or was never there — the link renders
  as the words the author wrote, unlinked, with no address and no name attached. The words are
  the author's and stay; the page's current title and address are the page's, and a link that
  went on announcing a page's new name to somebody whose access had been taken away would be a
  worse leak than the one it started as. Those four cases are deliberately told apart by
  nobody: "you may not see this" and "there is nothing here" differ only in confirming that
  something is there. The list you pick from follows the same rule — it holds only pages you
  may read, and it never says how many were left out.

  The full reasoning, including what this costs, is
  [ADR 0019](docs/decisions/0019-how-a-document-reference-is-written-in-markdown.md).

### Changed

- **A backup carries these links, and survives being loaded into an empty wiki.** An exported
  page writes such a link as `[Titel](dok:<kennung> "/aktueller/pfad")`: the page's identity,
  which is what does not break when the page moves, and beside it the address that page had
  when the file was written. Loading the files back into **this** wiki follows the identity and
  ignores the address, even for a page that has moved since — that is the whole point of
  storing identity. Loading them into a **fresh, empty** database gives every page a new
  identity, so no link can match one; each then falls back to its address and finds the page of
  the same address in the restored copy. A restored backup keeps its connections, not just its
  words.

  Two things it cannot do, and both are stated in the `FIDELITY` note that sits in every export
  directory: a link whose target the exporting account could not read carries no address at all
  and reads as plain words, and a restored link whose target file is loaded *later* in the run
  works immediately but does not appear in the graph until that page is next published.

<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named above. -->
