<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Code is coloured.** A fence that names its language — ` ```rust `, ` ```sh `, ` ```yaml ` —
  is read the way an editor reads it, so a keyword, a string and a comment are told apart at a
  glance instead of by reading the line. It works with JavaScript switched off, because the
  colouring happens while the page is being built rather than after it arrives, and it follows
  the light/dark control: every run of text carries a colour for each theme and the stylesheet
  picks, so a listing is never bright type on a dark page for the length of a redraw.

  **Eight languages, on purpose:** shell, YAML, JSON, SQL, Rust, TypeScript, Python and
  Markdown — and each one's usual short names, so ` ```sh `, ` ```yml `, ` ```ts ` and ` ```py `
  all work. This corpus is overwhelmingly prose, so the set is what people here actually write
  rather than the several hundred that were available, and a ninth is a decision rather than an
  import. **Nothing of the colouring library is downloaded**: the work happens while the page is
  built, and only its result travels — so a page with no code on it costs nothing at all, and
  one with code costs no waiting while a bundle arrives.

- **A language the wiki does not know says so, quietly.** ` ```kotlin ` renders exactly as it
  did before — correct, monospaced, uncoloured — with **Unbekannte Sprache: kotlin** in small
  type under the block. An author who wrote that and saw no colour otherwise has no way to tell
  whether the wiki does not know Kotlin, whether they misspelled it, or whether the colouring is
  broken, and one line answers all three. A fence that names **no** language gets nothing: the
  author said nothing, and the page has no business arguing with them. ` ```text ` and
  ` ```plain ` are the way to ask for no colour and get no comment either.

- **`just build` now refuses a front end the production image could not run.** The check that
  the server bundle imports no package the runtime container lacks lived only inside
  `docker build`, so a mistake of that kind passed every gate command green and surfaced at
  image build, after review. It is one script now, run in both places.

### Changed

- **A long fence, a very long line, a crowded page and a broken grammar all cost colour and
  nothing else.** Colouring happens on the server every reader shares, so it is bounded there:
  a fence past 20 000 characters, a fence holding a line past 400 characters (a pasted minified
  blob, which is what actually costs seconds), more than 100 fences on one page, and a page that
  has used up its quarter-second of colouring or its share of the response are all printed plain
  with a small line saying which limit it was. Anything that goes wrong inside the highlighter
  leaves the block uncoloured instead of taking the page down — which matters because that page
  is also the only way to edit the page that caused it. A fence whose text would not come back
  out of the highlighter character for character is printed plain as well: the whitespace in a
  code block is its content, and nothing here is allowed to rewrite it quietly.
