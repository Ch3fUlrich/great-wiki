<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **A diagram no longer fills the browser console with refusals.** Every drawn diagram used to
  log four *"Refused to apply inline style"* errors in a deployed wiki — and none at all when
  run from a developer's machine, which is the worst place for a difference to live. Mermaid
  measures a label by putting it on the page first and styling it there, and this wiki forbids
  exactly that kind of styling; the refusal was the rule working, but it left noise in the one
  place somebody looks when something else is broken, and it meant the labels were measured
  against the page's lettering rather than the lettering they were about to be drawn in. Sizes
  were a little off in a way padding usually hid. The drawing now happens somewhere else
  entirely (below), so there is nothing left to refuse.

### Changed

- **Diagrams are drawn in a sealed-off corner of the wiki rather than on the page itself.** The
  drawing is done by a library that needs a page to measure text on, and it had been measuring
  on yours. It now gets a blank page of its own — an address of this same wiki, with its own
  rules, that holds nothing but the drawing machinery — and hands back only the finished
  picture. What reaches the page you are reading is what always reached it: an image, in the one
  form no browser will execute anything from, with the diagram's own text as its description.
  Nothing else about a diagram changes: still drawn in your browser, still twice so that the
  light and the dark copy each match their background, still fetched only on a page that
  actually holds a diagram, still the diagram's own source until it is drawn and permanently
  with JavaScript switched off, and still every limit and refusal exactly as before.

- **A diagram that is never answered now says so, instead of never appearing.** If the drawing
  machinery cannot be fetched, or takes it upon itself to stop answering, the block falls back
  to the diagram's own text with one line underneath — the same thing it already did for a
  diagram that is too big or written wrong, rather than a space that stays empty forever.

- **The rule that nothing may be embedded in a page of this wiki is now "nothing but this wiki's
  own drawing corner".** That corner may itself embed nothing, and nobody else's site may embed
  it. It is the only such opening, it is one page deep, and the wiki's own pages are as strict
  as they were before.
