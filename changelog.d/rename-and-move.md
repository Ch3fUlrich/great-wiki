<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A page can be renamed and moved, and it takes everything under it.** One move is one act
  with one entry in the audit log, whatever the size of the subtree, and nothing else in the
  wiki changes: a link to the page, or to any page under it, is stored as the page's identity
  and resolved when it is read (ADR 0019), so every link keeps working and no other page is
  edited, re-versioned or re-authorised to make that true. Pages in the trash under the moved
  page move with it too, so restoring one later puts it back where its parent now is.

- **Access follows the new place, and you are shown who that changes before you confirm.**
  A page reads its access from the nearest page above it that carries any, so moving it can
  let people in and shut people out. The move is measured before it happens — who gains
  reading access to how many of the moved pages, and who loses it, including "everybody
  without an account" when the destination is shared publicly. The preview is the move
  itself, carried out and rolled back, so it cannot describe a different move from the one
  you then confirm. A move that lets anybody in needs admin rights on the destination; one
  that only narrows needs write on the moved pages and on the new parent, because the preview
  has already told you who loses. Access rules written on the moved pages themselves travel
  with them, as do invitations still waiting to be accepted for them.

- **The old address keeps working, for whoever may read the page.** Opening a bookmark or a
  pasted link to where a page used to be takes a reader to where it is now; somebody who may
  not read it is told there is nothing there, exactly as at any other address (ADR 0022). A
  page moved twice forwards from both old addresses to the current one. The forward ends the
  moment another page takes that address, including the page itself moving back.
