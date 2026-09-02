<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Diagramme werden gezeichnet.** A code block whose language is `mermaid` is no longer
  printed as source — it is drawn. Flowcharts, sequence diagrams, state machines, Gantt charts
  and the rest of what Mermaid understands; write the diagram between ` ```mermaid ` fences the
  way you would write any other fenced block. Nothing about the stored block changes, so a page
  holding one still exports, re-imports and round-trips as exactly the file it always did — and
  the example tour gained one, under **Was schon geht**.

- **Every diagram is drawn twice, once for each theme.** A picture cannot follow the light/dark
  control the way text can, so both are drawn and the one that matches the background you are
  reading on is the one shown. The cost, since it is real: two renderings per diagram and both
  of them in the page, of which one is never painted. It buys a diagram that is never white
  lines on a white page, or grey on black, for the length of a redraw.

- **The drawing happens in your browser, and only if a page actually holds a diagram.** Mermaid
  is the largest thing this wiki could ask a reader to download, so it is fetched when a diagram
  is on screen and never before: a page without one costs nothing at all. Until it is drawn —
  and permanently, with JavaScript switched off — the block shows the diagram's own text in a
  code block, which is what it did before this change.

### Changed

- **A diagram that will not draw shows its own source and says why, in the same small type as a
  code block's note.** Over 10 000 characters, more than 200 connections or 200 lines of
  diagram, or simply written wrong: the block prints the text as written with one line
  underneath. **It is never a broken picture** — every drawing is handed to the browser's own
  image decoder before it is put on the page, so a diagram the browser could not read shows its
  source instead of the grey icon that means "something is wrong with the network" — and never a
  page that fails to load, which matters because that page is also the only way to edit the page
  that caused it. The limits are generous on purpose; the diagram in the example tour is under
  300 characters.

- **`<br>` in a label breaks the line, as Mermaid's own documentation says it does.** It did
  not: a two-line label made the whole picture undisplayable, in every browser, with nothing in
  the wiki able to notice. Labels are now drawn as diagram text rather than as a scrap of web
  page, which is also why a diagram can no longer style its own labels — that was never
  something a diagram was allowed to do here.

- **A diagram far wider than it is tall is scrolled instead of being squashed.** Shrinking a
  drawing sixty times wider than it is tall into the width of the column turns it into a grey
  line; past about eight-to-one it keeps its own size and you drag it sideways, inside the
  diagram's own box, without the page itself moving.

- **A diagram is a picture the page cannot be reached through.** The drawing arrives as an
  image, in the one form no browser will execute anything from — the same containment an
  uploaded SVG has had since attachments landed, applied now to something this wiki generated
  from text somebody typed. Nothing in it can run, style the page around it, or call home; a
  diagram cannot even choose its own colours, because that is what would make the light and dark
  copies come out the same. The consequence, stated: the text inside a diagram is a picture, so
  it is not selectable and the browser's find will not see it. The diagram's own source is
  carried as the image's description instead.
