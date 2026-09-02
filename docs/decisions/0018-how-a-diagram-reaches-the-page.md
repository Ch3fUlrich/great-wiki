# 0018 — How a diagram reaches the page

**Status:** Accepted (2026-09-02)

## Context

D-19 of [the rich-blocks plan](../superpowers/plans/2026-09-02-rich-blocks.md) makes a
` ```mermaid ` fence draw. Mermaid turns the fence's text into an SVG, and this is the first
time this repository renders **generated markup** — markup that is a function of text somebody
with write access to one page typed, produced by a dependency, and then somehow put on a page
another person reads.

[ADR 0014](0014-what-a-file-has-to-be-to-be-attached.md) already answered the neighbouring
question for an *uploaded* SVG, and its switch-back criteria anticipate this one by name:

> An SVG needs to render in the page. `<img src>` on the existing address already works and is
> safe… What would need this decision changed is wanting the markup in this wiki's DOM, and the
> answer to that is a rasterised copy, not a sanitiser.

The reader is the other half of the context. `BlockView.svelte` renders a document by matching
on block kind and skips a kind it does not know, so **no untrusted HTML is constructed anywhere
in it** — which is why nothing there sanitises anything, and why
`scripts/check-html-sinks.sh` can hold an empty exemption list. That sentence is load-bearing
for a wiki whose pages are written by one person and read by another, and a diagram renderer is
one of the three pieces of work most likely to end it.

## Decision

### The rendered SVG becomes an `<img src>`, and never markup in this document

`mermaid.render(id, text)` — the **two-argument** form, which returns `{ svg }` as a string —
is called in `web/src/lib/blocks/mermaid.ts`. The string is percent-encoded into a
`data:image/svg+xml` URI and bound as the `src` of a plain `<img>` in `DiagramView.svelte`, with
the diagram's own source as `alt`.

`img-src ['self', 'data:']` already admits that address (`web/vite.config.ts`), so **no policy
directive moves for this feature**. An attribute is not markup, so the sink check finds nothing
to permit and its exemption list stays empty: the KaTeX leaf remains the only place in this
reader where a string is parsed as HTML, and it is permitted by line rather than by file.

Rejected: `{@html svg}` or `el.innerHTML = svg`, which is precisely the mechanism ADR 0014
forbids — execution *in this origin, with the session cookie in reach*. Generated SVG is not
exempt because we generated it.

Also rejected: **`mermaid.render(id, text, container)`**, the three-argument form. Verified in
the installed package (`mermaid@11.17.2`, `dist/mermaid.core.mjs`): its first act on the
container it is handed is `svgContainingElement.innerHTML = ""`, and the diagram is then built
inside *your* element in *this* document. The insertion ADR 0014 forbids would happen one stack
frame down inside a dependency, where no grep for our own spellings would see it — which is why
`scripts/check-html-sinks.sh` greps for a three-argument `render(` call in any file mentioning
mermaid, and why `diagram.test.ts` asserts the same thing from the other side.

### There are two barriers, and the CSP is the one that holds while the diagram is drawn

This has to be stated honestly, because the tempting version of it is false. Mermaid needs the
DOM to measure text. Verified in the installed package rather than taken from its documentation:
`render` does `let root = select(document.body)`, appends a temporary `<div id="d…"><svg id="…">`
to it, and inserts a `<style>` element built from the theme *into that SVG while it is still in
the live document*. So during rendering the diagram — labels included — really is inside
`document.body`, and the `<img>` protects nothing yet.

**Barrier one, during `render()`: the Content-Security-Policy** ([ADR 0007](0007-content-security-policy.md)).

- `script-src ['self']`, no `'unsafe-inline'` and no `'unsafe-eval'` — refuses an inline
  `onerror`/`onload` that escaped mermaid's own DOMPurify pass.
- `style-src ['self']` — refuses an injected `<style>` ELEMENT, which is the CSS-injection class.
- `img-src ['self', 'data:']` — refuses a remote beacon.
- `object-src`, `frame-src`, `base-uri` are `['none']` — close the rest.

Mermaid neither calls `eval` nor instantiates WebAssembly (checked across every `.mjs` it
ships), so `script-src 'self'` costs it nothing.

**Barrier two, afterwards: the `<img>`.** No browser executes script in one, and it reaches no
DOM. This is ADR 0014's containment, applied to bytes we produced rather than bytes we stored.

**Barrier one visibly fires, and that is it working.** Verified in a real browser against a
production build (`node build/index.js`, the real nonce-based policy on the response): every
diagram logs

```
Refused to apply inline style because it violates the following Content Security Policy
directive: "style-src 'self' 'nonce-…'"
```

— once per render, so twice per diagram. That is the `<style>` element mermaid inserts while it
measures, refused exactly as an injected style element should be. **The drawing is unaffected**:
the same `<style>` is serialised into the returned string, and inside the `<img>` it is the
image's own business, where this page's policy does not reach. What the refusal actually costs is
that mermaid measures text against the page's own font rather than the one it is about to draw
with — both are proportional sans faces at 16 px, mermaid's node padding absorbs the difference,
and the checked-in example renders correctly with nothing clipped.

**The fix for that console line is never `'unsafe-inline'` in `style-src`.** Beyond being the one
loosening ADR 0007 refused, it would make `widenCspNonceToStyles` skip the directive
(`web/src/lib/csp.ts`) and silently strip the nonce TipTap depends on — unstyling the *editor*,
in production only.

**And barrier one is weaker in development, which is the sentence this record exists to leave
behind.** SvelteKit adds `'unsafe-inline'` to `style-src` under `npm run dev` so it can inject
its own component styles (`web/src/lib/csp.ts` records this, and records that it was found by
running the dev server rather than by reading the spec). So the CSS-injection class is *open*
under `npm run dev` and closed in production. A production-only difference is the worst kind,
and this is the second one in this repository — hence: **a diagram is verified against a
production build and a real browser, never against `npm run dev`.**

### `securityLevel: 'sandbox'` is unavailable here, and that is not reconsiderable

Every Mermaid advisory recommends `securityLevel: 'sandbox'` as its workaround. It emits
`<iframe src="data:text/html;base64,…">`, and this application's `frame-src` is `['none']` —
written on the grounds that *"nothing is embedded and nothing embeds this"*. Loosening it to
admit `data:` would hand a general XSS-hosting primitive to the policy in exchange for a library
setting. The plan's own gate applies: if a directive turns out to be needed, that is the signal
to reconsider the feature rather than the policy.

So `securityLevel: 'strict'` it is, `'loose'` and `'antiscript'` are never used, and
`bindFunctions` is **never called** — it is what would wire a diagram's `click` interactions to
the page, and there is nothing in an `<img>` to wire them to anyway. Not calling it is the belt
to strict's braces. An explicit `secure` list names the configuration keys a diagram's own
`%%{init: …}%%` directive may not set: mermaid's six defaults, plus `dompurifyConfig` (which
would weaken the sanitiser meant to be protecting the page from that diagram), `themeCSS`,
`themeVariables`, `fontFamily`, `altFontFamily`, `htmlLabels` and `theme`. Each of those is
asserted in `web/src/lib/blocks/diagram.test.ts`, because a configuration nobody tests is a
default waiting to come back.

### Every diagram is drawn twice, once per theme (D-24)

An `<img>` inherits neither `prefers-color-scheme` nor this wiki's own `[data-theme]` control, so
one fixed picture can only ever match one background. Both are drawn — mermaid's `default` and
`dark` themes — both data URIs go into the markup, and `DiagramView.svelte`'s stylesheet shows
whichever applies, using the same pair of rules `tokens.css` uses for every other colour here.

**The cost is stated rather than hidden: two renders per diagram, and two copies in the DOM of
which one is never painted.** It is paid in the reader's own tab, once per page load, on a wiki
of tens of pages, and it buys a diagram that is never wrong against the background it is read
on.

Rejected: one neutral look (it would read as deliberately plain rather than as wrong, but
"acceptable on both grounds" is a compromise nobody asked for on a page they are trying to
read), and re-drawing on a theme change (it trades a fixed cost for a flash of the old picture
at exactly the moment somebody is looking).

### Generous caps, and a refusal that names the limit (D-22)

The availability class is the one the `<img>` does not close: a parser that loops forever hangs
the reader's tab, and mermaid cannot be moved into a Web Worker because it needs the DOM.

- `DIAGRAM_CHARACTER_LIMIT` is 10 000 characters, checked **before the library is fetched at
  all** and passed to mermaid again as `maxTextSize`. Over it, the fence renders as its own
  source with a German line naming the limit — which is in the first response, because the cap
  is a property of the text and nothing has to run to apply it.
- `maxEdges` is 200 rather than mermaid's 500: edge count is what the layout algorithms are
  superlinear in, and 200 edges is already past what a reader can follow. That one is enforced
  by the library, and it throws a sentence about *its own configuration*, in English, at
  somebody who was drawing a flowchart — so it is recognised by name and answered in German.
  Matching on a dependency's message is brittle deliberately: when it stops matching, the cost
  is the general sentence rather than the specific one, never a wrong one.
- `DIAGRAM_STATEMENT_LIMIT` is 200 as well, and it exists because **`maxEdges` protects
  flowcharts and nothing else**. This paragraph said otherwise until 2026-09-02, and the
  measurement is what corrected it: in Chromium under the production policy, a `classDiagram`
  of about 750 `C <|-- D` relations — 9 805 characters, inside every cap named here — drew in
  **16.8 s**, and seven animation frames were served during that window against a 61 fps
  baseline. That is a frozen tab, not a slow one. The same 9.8 kB costs 9 478 ms as a
  `stateDiagram-v2`, 7 419 ms as a `mindmap`, 4 461 ms as 300 nested subgraphs and 1 498 ms as
  a `journey`. So the count is ours, syntax-agnostic (a statement is a line, or a
  `;`-separated part of one, ignoring blanks and `%%` comments), and asked before the library
  is fetched — which puts that refusal in the first response too. It over-counts a label
  containing a semicolon, deliberately: this is a bound on work, not a parser for six grammars.
- Everything is inside a `try`, and a failure renders the source with one German line.
- **Malformed source is never a broken image and never an exception** — and that sentence was
  false as first written, which is why it now has a mechanism under it rather than a promise.
  `diagramDataUri` is a pure function that sees a string; whether a `data:image/svg+xml`
  decodes is a question only an XML parser can answer. Mermaid serialises the finished SVG
  with `innerHTML`, the HTML serialiser, so an `htmlLabels` label containing the documented
  Mermaid line break — `A[Erste Zeile<br>Zweite Zeile]` — came back with a `<br>` that has no
  closing tag: well-formed HTML, and not well-formed XML. Chromium, Firefox and WebKit all
  showed two `<img>` with `naturalWidth === 0`, and `DOMParser` on the decoded source answered
  *"Opening and ending tag mismatch: br line 1 and p"*. `<br/>` behaved identically, because
  the serialiser normalises it back. Two things now hold the sentence up:
  **`htmlLabels: false`**, so a label is SVG `<text>` and mermaid splits it on `<br>` into
  `<tspan>`s — the break works as documented — and **the browser's own image decoder**, which
  every address is put through (`new Image()` plus `decode()`) before it reaches the page.
  Same bytes, same code path, same answer as the `<img>` is about to give; a rejection is the
  German line and the fence's own source. A `DOMParser` would be the obvious alternative and is
  deliberately not used — `parseFromString` is one of the spellings
  `scripts/check-html-sinks.sh` refuses, and its exemption list is empty and stays empty.
  `content-example/` now seeds a `<br>` label, and behaviour check L3 is the browser that
  proves it.

**There is deliberately no per-page diagram count limit**, and the asymmetry is the reason: a
` ```math ` fence is typeset on the shared server, where `Store::open` holds
`max_connections(1)` and a slow page load is a lever on the whole deployment
([ADR 0017](0017-what-a-formula-may-do.md) caps it hard for exactly that reason); a diagram is
drawn in the reader's own tab, where the same mistake costs one tab. Renders are serialised
through one queue so that a page of twenty diagrams yields to the event loop between them
instead of being one long block of script.

That is still the decision after the statement cap, and the residual is stated rather than
implied: a page of twenty diagrams each sitting on the cap is minutes of intermittently frozen
tab, one diagram at a time. It costs the reader a tab they can close, a count limit would
refuse a legitimate page of many small diagrams, and the cap above is what turned "minutes per
diagram" into "seconds per diagram". A per-page limit is the thing to reach for if that stops
being enough.

### The library is loaded through a `browser`-guarded dynamic import

`$app/environment`'s `browser` is replaced with a literal at build time, so in the SSR build
`browser ? import('mermaid') : …` reads `false ? … : …` and rollup drops the import entirely. A
bare `import()` inside a branch that never executes on the server does **not** achieve that —
the chunk is still emitted, and the production web image ships no `node_modules` for it to
resolve against. This is the same shape, and the same reasoning, as `loadEditor` in
`web/src/routes/[...path]/+page.svelte`, which documents having learnt it the expensive way.

So mermaid is **deliberately absent from `ssr.noExternal`**, where Shiki and KaTeX had to be
added: those two are called during server rendering, and this one can never be. Verified after
`npm run build`: no server chunk contains `import('mermaid')`, and the library sits in its own
client chunk that a page holding no diagram never fetches.

## Consequences

- **Text inside a diagram is not selectable and not searchable by the browser's find**, and it
  is not in `plain_text`, so it will not be in the search index at M7 either. The `alt` carries
  the diagram's source, which is the only description anybody wrote.
- **A drawing many times wider than it is tall keeps its own size.** `max-width: 100%` is
  right for almost every diagram and catastrophic for a very wide one: the 750-relation class
  diagram above laid out to a viewBox of 63 604 × 306, which shrinks into a 700-pixel column
  as roughly 700 × 3 CSS pixels — a grey line where a picture should be. Past eight-to-one
  (`DIAGRAM_ASPECT_LIMIT`) `DiagramView` drops the `max-width` and the drawing is scrolled
  inside the box the wrapper already is: legible by dragging rather than illegible in place.
  Eight, because at that ratio a drawing shrunk into a 390-pixel phone column is still about
  49 pixels tall — three lines of node text.
- **The `<img>` is sized from the drawing's own `viewBox`.** Mermaid emits `width="100%"` with
  a `style="max-width: …px"`, which in a document means "as wide as there is room for, no wider
  than natural size" and inside an image means no intrinsic width at all — the browser stretches
  a three-node diagram across the whole column. Observed against a production build, not
  reasoned about. `diagramSize` reads the `viewBox` and the numbers become `width`/`height`
  attributes, validated as finite, positive and not absurd because they are computed from text
  an author typed and they become layout.
- **A diagram cannot use this wiki's own fonts.** An SVG loaded as an image renders in its own
  isolated context and cannot see the page's stylesheet, so the vendored faces are unavailable
  to it and a generic stack is what resolves. The same stack is used for mermaid's text
  measurement, so labels are laid out against the family they are drawn in.
- **There is no longer a way to show mermaid source *as* source**, which is GitHub's behaviour
  and is defensible, but is a rendering change to stored content that no migration records. The
  escape hatch ships with it: ` ```text ` and ` ```plain ` never draw and never highlight
  (D-18). Verified before the change: no ` ```mermaid ` fence existed in `content-example/` or
  `content-darm/`.
- **Nothing about the diagram is stored.** No block kind, no attribute, no mirror: a diagram is
  `BlockKind::CodeBlock` with `language: "mermaid"`, exactly as a highlighted listing and a
  formula are. The rendered SVG is component state and is re-derived on every page load. A
  cached SVG, a computed size or a parse-error flag written back onto the block would be the
  D-18 trap. `CODE_BLOCK_ATTRS` is now in `export.rs::reduce()` — it was written about here
  before it existed, which was its own small lie — so the symptom of writing one would be a
  silently incomplete backup rather than a loud refusal. That is the better failure of the two
  and it is still a failure; the allow-list is a safety net against what a WRITER can put on a
  block over the collaboration socket, not a licence for what the schema may declare.
- **The advisory stream for Mermaid is active rather than settled.** `npm audit` reports nothing
  against `mermaid@11.17.2` as installed (the two advisories it does report predate this change
  and come from `vite`/`postcss` and `@sveltejs/kit`), but the pattern described in the research
  is a series of escapes from the default `securityLevel: 'strict'` followed by related
  bypasses. The posture here does not depend on the library being correct — that is the whole
  point of the two barriers — but the version should be re-checked against the registry
  whenever this file is next opened.

## What would change this decision

Wanting the diagram's markup in this wiki's DOM — selectable labels, a clickable node, a live
preview inside the editor. The answer to that is the one ADR 0014 already gives: a rasterised
copy, not a sanitiser. A TipTap `NodeView` for editing is not this: it changes rendering only,
declares no attributes, and leaves the schema byte-identical.

Wanting a diagram to be in the first response, without JavaScript. That needs server-side
rendering, which needs a DOM on the server (a headless browser, or a DOM shim mermaid does not
support), and it needs somewhere to put the result — and there is nowhere that is not
`Block::attrs`, which D-18 closed. It is a piece of work, not a setting.
