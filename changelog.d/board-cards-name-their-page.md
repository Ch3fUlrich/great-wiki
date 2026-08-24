<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A card on a board now says which page it was written on.** Until now a card could say
  only that it *came* from somewhere — it carried a bare "written in a page" flag and named
  no page at all — so the board could show you a to-do and give you no way back to the
  sentence it came from. Every card written as a line in a page now carries that page's
  address and its name, which is what an interface needs to make the card a link. "Where did
  I write this?" is the first question anybody asks of a board, and it now has an answer that
  does not involve searching for the words.

  A card **created on a board** names no page, and that is not an omission: no page holds it,
  and naming the project's home page — which is the page that decides who may touch the card
  — would claim a line exists somewhere that never held one.

  The name is looked up when the board is read rather than copied onto the card when it is
  made, so renaming or moving a page does not leave boards saying what it used to be called.
  This is the same rule a link between pages already follows.

### Security

- **A card's page is filtered exactly as the card is, and by the same answer.** A page's
  address and its title are precisely what a restricted page keeps to itself — the whole
  reason a card on a page you may not read is not shown, not greyed out and not counted — so
  naming the page is the same disclosure one layer deeper. It is not a second lookup made
  after the card survived filtering: the page a card names *is* the page the permission check
  handed back, so there is no version of this code that shows you a card without having asked
  whether you may read what it is called. Asking twice is how the two answers start to
  disagree, and the second one is always the one that gets it wrong.
