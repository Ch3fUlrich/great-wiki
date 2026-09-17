# 0018 — How a diagram reaches the page

**Status:** Accepted (2026-09-02), amended (2026-09-17)

> **Amendment, 2026-09-17 — Mermaid runs in a frame of its own (D-26).** The section
> *"There are two barriers, and the CSP is the one that holds while the diagram is drawn"*
> described a state this record no longer describes. Barrier one is now a **boundary** rather
> than a policy holding while a dependency worked inside the reader's page: Mermaid runs in an
> `<iframe>` served from `/_diagramm`, one route of this same application, with a policy scoped
> to that route. The diagram's text crosses to it over `postMessage` and an SVG string crosses
> back; the `<img>` is built exactly as before. The original text is kept below with the
> amendment beside it rather than rewritten away, because the measurement that forced it — the
> four refusals per diagram, visible only in production — is the reason this decision exists at
> all.

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
- `object-src`, `frame-src`, `base-uri` are `['none']` — close the rest. (`frame-src` is
  `['self']` since the amendment below; the other two are still `['none']`.)

Mermaid neither calls `eval` nor instantiates WebAssembly (checked across every `.mjs` it
ships), so `script-src 'self'` costs it nothing.

**Barrier two, afterwards: the `<img>`.** No browser executes script in one, and it reaches no
DOM. This is ADR 0014's containment, applied to bytes we produced rather than bytes we stored.

**Barrier one visibly fired, and that is what D-26 came from.** Verified in a real browser
against a production build (`node build/index.js`, the real nonce-based policy on the response):
every diagram logged

```
Refused to apply inline style because it violates the following Content Security Policy
directive: "style-src 'self' 'nonce-…'"
```

— once per render, so twice per diagram, four lines for the pair. That was the `<style>` element
mermaid inserts while it measures, refused exactly as an injected style element should be. The
drawing was unaffected: the same `<style>` is serialised into the returned string, and inside the
`<img>` it is the image's own business, where the page's policy does not reach. What the refusal
actually cost was that mermaid measured text against the page's own font rather than the one it
was about to draw with — both proportional sans faces at 16 px, with mermaid's node padding
absorbing the difference.

**Neither half of that was acceptable to leave documented.** Console noise sits in the one place
a developer looks first when something else is wrong, and a measurement made against the wrong
face is a defect even where padding hides it. So the amendment below moved the library instead of
the policy.

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

**That sentence used to end in "by hand", and that is what let the four refusals survive a
fortnight of green harness runs.** Since D-26 it does not: `just behaviour` builds and starts an
adapter-node server on a second port, and **Group M** is the group that asks the real policy the
real question. See the amendment below for what it asserts.

**And the `<style>`-in-the-live-document problem above is gone rather than contained**: mermaid's
live document is now the frame's, not the reader's.

### AMENDED (D-26): Mermaid runs in a frame of its own, and that is where barrier one is

The library needs a DOM and injects a `<style>` into it. The three ways out of that were: leave
it documented; loosen `style-src`; or give the library a document that is not the reader's page.
The owner chose the third.

**The mechanism.** `web/src/lib/blocks/mermaid.ts` — still the page's renderer, and now holding
no library at all — creates one hidden `<iframe>` whose `src` is `DIAGRAM_FRAME_PATH`
(`/_diagramm`, named once in `$lib/blocks/diagram` so the policy and the element cannot
disagree). `web/src/routes/_diagramm/` is that document: it loads mermaid, listens for a
diagram's text and a theme, renders with the **two-argument** `mermaid.render(id, text)` exactly
as before, and posts the SVG **string** back. The page percent-encodes it into a
`data:image/svg+xml` URI and sets it as an `<img src>`. Barrier two is untouched; the sink check
still finds nothing and its exemption list is still empty.

**The policy, and the one directive that moves.** SvelteKit's `kit.csp` is one configuration for
the whole application, so the frame's response gets its own by replacement in
`web/src/hooks.server.ts`: `$lib/csp`'s `diagramFramePolicy` sets `style-src 'self'
'unsafe-inline'` and `frame-src 'none'` and leaves every other directive byte-for-byte —
`script-src 'self' 'nonce-…'` in particular, because the frame is same-origin and therefore holds
the session cookie. On the **page**, `frame-src` opens from `'none'` to `'self'` and nothing
wider; `frame-ancestors 'self'` was already there and is what stops anyone else embedding the
renderer. That is the whole of the policy change.

**Why a nonce is not used on the frame's `style-src`, and why the two repairs are alternatives.**
A source list containing a nonce makes browsers ignore `'unsafe-inline'` beside it — the rule
`widenCspNonceToStyles` exists to respect. So the frame's response goes through
`diagramFramePolicy` **instead of** the nonce widening, never after it. Getting that order wrong
would leave a frame whose loosening is silently inert, in production only.

**The sandbox flags, which are the easiest thing here to get wrong.** The element carries
`sandbox="allow-scripts allow-same-origin"`. `allow-same-origin` is **required** and its absence
would be silent: a frame sandboxed without it has an *opaque* origin, so `'self'` in its own
policy matches nothing — including the module chunks it must load, which cannot be nonced because
a nonce does not reach a dynamic `import()` — and `event.origin` from it is the string `"null"`,
which any sandboxed frame anywhere can present. So the frame is same-origin by construction and
**the sandbox is not an origin boundary**: this document can reach `parent.document` and the same
`localStorage`, and nothing in the attribute prevents it. What the attribute still removes is
top-level navigation, popups, form submission, downloads, modal dialogs and the
pointer/presentation/orientation locks — defence in depth, not the barrier. The barrier is
`script-src`, which did not move. **The real isolation is the document**: its own policy, its own
`document.body` for mermaid to measure in, its own style context, and nothing author-controlled
in the reader's DOM at any point.

**Both directions of `postMessage` are checked, and both checks are pure functions with tests.**
A `message` listener hears everything its window is sent, and the payload is whatever a sender
chose; the two fields a sender cannot choose are `event.source` and `event.origin`, and both are
compared at each end. `rahmenAuftrag` (the frame's side) requires the sender to be
`window.parent`, requires `window.parent !== window` so that a directly-opened `/_diagramm`
accepts nothing, requires `event.origin === location.origin`, and then validates the payload field
by field. `rahmenAntwort` (the page's side) requires the sender to be the `iframe.contentWindow`
this page created, requires the same origin, and validates likewise. They live in
`$lib/blocks/diagram` so that they can be tested without a browser — a boundary check that can
only be exercised by hand is one that is exercised once and then trusted forever.

**A frame that never answers is a stated state.** The frame reports ready only once its library
is in hand (60 s budget, which pays for the download), and each drawing has 30 s. Past either, the
fence shows its own source with a German line, exactly as a cap or a malformed diagram does. A
reader with JavaScript switched off is unaffected: the source and the `alt` are what they always
were.

**What did not move.** The caps, `securityLevel: 'strict'`, `htmlLabels: false`, the `secure`
list, `suppressErrorRendering`, the two renders per theme (D-24), the `viewBox` sizing, the
aspect-ratio rule and the decode check are all where they were — in `$lib/blocks/diagram`, which
both documents import, rather than copied into each. Mermaid is still absent from
`ssr.noExternal` and still reached through a `browser`-guarded dynamic import, now inside the
frame's module; `web/scripts/check-server-bundle.sh` is what holds that.

**And it is now checked rather than remembered.** `just behaviour` starts an adapter-node
production build on a second port, and **Group M** asserts against the real policy: `frame-src
'self'` on the page and nothing looser, the frame's own four directives, zero policy violations
anywhere on a page holding a diagram (both documents, since `page.on('console')` hears every
frame), no mermaid element or injected style left in the reader's document, the sandbox
attribute's exact value, and — from inside the frame — that it is same-origin and that
`window.open` still answers `null`. Group L's named exclusion for the four refusals is gone,
because there is nothing left to exclude.

**Cost.** A second document to keep in step with the first, one more page of this application
that exists for machinery rather than for a reader, and `frame-src` open by one origin. The root
layout carries a three-line opt-out for it, because SvelteKit has no way for a route to escape
the root layout and the frame must not carry the workspace — not merely for the waste, but
because the tab strip's effect writes to a `localStorage` the frame *shares* with the page that
created it.

**Rejected:** leaving it documented (the noise masks real errors in exactly the place a developer
looks first), `'unsafe-inline'` on the page's `style-src` (the one loosening ADR 0007 refused,
and it would make `widenCspNonceToStyles` skip the directive and unstyle the editor in production
only), a nonce or a hash for mermaid's `<style>` (the library has no nonce hook — verified
against `mermaid@11.17.2` — and a hash is a copy of a dependency's private constant), and
pre-rendering at publish time with a headless browser on the API host (the cleanest result, and a
large new dependency on the one host that must stay small).

### `securityLevel: 'sandbox'` is unavailable here, and that is not reconsiderable

Every Mermaid advisory recommends `securityLevel: 'sandbox'` as its workaround. It emits
`<iframe src="data:text/html;base64,…">`, and this application's `frame-src` was `['none']` —
written on the grounds that *"nothing is embedded and nothing embeds this"*. Loosening it to
admit `data:` would hand a general XSS-hosting primitive to the policy in exchange for a library
setting. The plan's own gate applies: if a directive turns out to be needed, that is the signal
to reconsider the feature rather than the policy.

**D-26 did not weaken this, and the distinction is the point.** `frame-src` is now `['self']` on
the page — one origin, this one, serving one document written here — and `['none']` again on that
document's own response. `data:` is still refused, so `securityLevel: 'sandbox'` is still
unavailable and `'strict'` is still what is set, *inside the frame*. A frame this application
serves and a frame whose contents arrive base64-encoded in a URL are not the same object.

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
