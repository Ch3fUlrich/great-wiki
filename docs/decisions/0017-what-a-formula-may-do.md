# 0017 — What a formula may do

**Status:** Accepted (2026-09-02)

## Context

D-20 of [the rich-blocks plan](../superpowers/plans/2026-09-02-rich-blocks.md) makes a
` ```math ` fence a **typeset formula**, drawn by KaTeX. Like the syntax highlighting in
[ADR 0016](0016-the-syntax-highlighter-s-regex-engine.md), it adds no `BlockKind`, no block
attribute and no mirror: a formula, a diagram and a highlighted listing are all
`BlockKind::CodeBlock` carrying a `language`, and which one you get is a rendering decision.

Three things make this different from highlighting, and each of them is a decision below.

**KaTeX emits HTML.** `BlockView.svelte` renders a document by matching on block kind and
skips a kind it does not know, which is why nothing in this reader sanitises anything —
`scripts/check-html-sinks.sh` exists to keep that true and its exemption list is empty.
A typesetter that hands back markup is the first legitimate reason to put a string into the
page as markup, and this repository has been careful enough about that sentence that
weakening it needs an argument rather than an exemption.

**A formula runs on the shared server.** `Store::open` holds `max_connections(1)`, so work
done while a page loads is work every other reader is queued behind. And an uncaught throw
in a page's `load` is a 500 for the whole route — a route that is also the only way to edit
the page that caused it, so a page could put itself beyond repair.

**KaTeX has an option that would undo all of it.** `trust` enables `\href`, `\url`,
`\includegraphics`, `\htmlClass`, `\htmlId`, `\htmlStyle` and `\htmlData`.

## Decision

### KaTeX 0.18.5, pinned exactly, called from the page's `load`

`web/src/lib/server/maths.ts` walks the body, finds every ` ```math ` fence, and calls
`renderToString` once per distinct fence. `[...path]/+page.server.ts` hands the result down
to `BlockView` as `formeln`, exactly as it hands down `anhaenge`, and `MathView.svelte`
draws it.

**Not in the component, and that is the point.** A Svelte component renders on the server
**and again in the browser while it hydrates**, so a component that imported KaTeX would put
the whole library — 272 kB minified — into every reader's bundle in order to re-derive
markup that reader had already been sent. Under `$lib/server/` it cannot: SvelteKit refuses
to let any client-reachable module import that directory, so "no maths library reaches the
reader" is a build error naming the file rather than a promise in a comment. Measured on
this build: the client carries KaTeX's stylesheet and none of its JavaScript.

The plan's D-20 says the try/catch belongs *"inside the leaf component"*. The reasoning it
gives for that — an uncaught throw during server rendering is a 500 for the route, and the
route is the edit surface — transfers to a `load` unchanged, and is what the `try` in
`typesetDocument` is for. What does not transfer is putting KaTeX in the reader's bundle,
which is the thing D-20 chose server rendering to avoid.

Pinned to an exact version, with no caret, for the reason the highlighter is: KaTeX's
stylesheet, its class names and its font metrics are one artefact, and the vendored copies
of two of the three are checked against the installed package on every test run.

### `trust` is never passed — and that is the whole of the safety story

It defaults to `false`, and that single default disables all seven commands at once. There
is nothing to configure: the safe setting is the one you get by not typing anything.
`katexOptions()` therefore does not mention it, `maths.test.ts` reads the module's own source
to check that it never starts to, and each of the seven is asserted against the *attribute*
it would have written rather than against its name appearing anywhere — KaTeX echoes the
author's TeX inside `<annotation encoding="application/x-tex">` on purpose, so `\href{https://…}`
does appear in the answer, as text, and must not appear as an `href`.

**Turning it on would be an amendment to [ADR 0007](0007-content-security-policy.md), not a
configuration change.** That decision admits `style-src-attr 'unsafe-inline'` partly on the
sentence *"the renderer does not emit authored CSS into one either way"*
(`web/vite.config.ts`), and `\htmlStyle` puts author-written declarations straight into a
`style` attribute. The reasoning is written in `maths.ts` as well as here, because that is
where somebody about to make the change would be looking.

Everything else KaTeX's output needs, the policy already gives it: its inline `style="…"`
**attributes** are what `style-src-attr` pays for, its stylesheet is a real file under
`style-src 'self'`, and its faces are served from this origin under `font-src 'self'`. **No
CSP directive moves.**

### The one HTML sink is permitted as a LINE, not as a file

`MathView.svelte` puts `formel.html` into the page with Svelte's raw-HTML tag. The plan
expected `scripts/check-html-sinks.sh` to gain its first `EXEMPT` entry here. It did not, and
the list is still empty.

Exempting a *file* switches the check off for that file: every sink spelling, every line, for
as long as the file exists — and it does so precisely where the risk is highest, since the
exempt file is by definition the one already handling generated markup, where a second sink
would be least surprising to write. The argument that was actually made is about **one
value**, so what is permitted is one value:

```
^web/src/lib/components/MathView\.svelte:[0-9]+:[[:space:]]*\{@html formel\.html\}$
```

The same expression in another file is still a finding; another expression in that file is
still a finding; another sink in that file is still a finding. The self-test plants all four
cases so that narrowing any of them back fails by name. The cost is stated in the script: a
rename or a reformat of that line turns the check red, and the fix is to edit the list — which
is the intended failure, because a check that keeps passing while the thing it describes moves
has stopped describing anything.

What makes the input safe is written on the component: it is KaTeX's own output rather than
stored content, `trust` is never passed, and a `Formula` has exactly one producer, which lives
under `$lib/server/` and therefore cannot acquire a second one in client code.

### The faces are vendored, because a CDN font fails silently

`font-src 'self'`. `fonts.css` records that this is not a latency decision: a font fetched
from a third party on every page load hands that party the reader's IP address and the page
they were reading. Under this policy a CDN font does not fail loudly — it fails to load, and
the maths renders in whatever serif the machine has, at metrics KaTeX did not compute. That is
wrong in a way nobody reports.

So `web/scripts/vendor-katex.mjs` copies the twenty woff2 faces into
`static/fonts/katex/` (304 kB, the woff and ttf cuts left behind) and rewrites KaTeX's own
stylesheet into `src/lib/styles/katex.css` with exactly two substitutions — the `src:` list
reduced to woff2, and each address made absolute under `/fonts/katex/`. `styles/katex.test.ts`
asserts that what is committed is byte-for-byte what that script would write from the
installed package, so a version bump cannot leave a stylesheet computed against metrics that
have moved.

The faces are **MIT**, KaTeX's own, and are the one family here that is not OFL: the carve-out
at the top of the root `LICENSE` names them, KaTeX's licence text sits beside the binaries,
and `styles/fonts.test.ts` now asks each family for the licence it is actually under. That
test also stops asking the maths faces for a capital sharp s — `KaTeX_Math` is a typesetting
alphabet that never sets a German word, and demanding ẞ of it would be demanding a glyph
nothing would ask it to draw.

### Four caps, and each one bounds something the others do not

Generous — you would have to try. A display formula in this corpus is tens of characters long.

- **5 000 characters per fence.** Over it, the fence renders as its own source.
- **100 formulas per page**, counted as *attempts* rather than successes, so that a page whose
  formulas all fail costs the same bounded number of KaTeX calls as a page whose formulas all
  work. Identical fences are typeset once and looked up thereafter.
- **1 000 000 characters of typeset markup per page.** **This is the cap that actually bounds
  the response, and it is here because KaTeX amplifies.** Measured, not assumed: `x + `
  repeated comes back at roughly 320 characters of markup per source character, so a fence
  comfortably inside the character limit can be more than a megabyte on its own. An input cap
  bounds the parser's work; it bounds the answer's size only by a factor nobody would guess.
  A render whose result would cross the line is discarded rather than trimmed — half a formula
  is not a formula.

  **It is a stop and not a skip, and it was a skip until 2026-09-02.** Refusing one formula
  for crossing the line and then trying the next leaves the total where it was, so the total
  never rises and every one of the hundred allowed renders still runs. Measured through
  `typesetDocument` itself: a hundred `\begin{array}{c×1650}` fences of 4 975 characters
  each — all inside the per-formula limit — cost **7 339 ms** of SSR time on one page load and
  kept exactly **one** formula, with ninety-nine rendered and thrown away, repeatable on every
  request. So the first refusal now closes the page: every later fence gets the same sentence
  without being handed to KaTeX at all, which is two renders instead of a hundred.
- **250 milliseconds of typesetting per page.** **The cap that bounds the WORK**, which none of
  the three above does on its own. Markup and CPU are not proportional: `\begin{array}` fills
  the markup budget in a single render, while `\text{a a a …}` measures at 20 ms for 15 kB —
  sixty of those stay inside the markup budget and cost well over a second of the one thread
  every other reader's page load is queued behind. Counted as time actually spent inside
  KaTeX, so a page competing with something else on the machine is not punished for it, and
  checked *between* renders: nothing can interrupt a render already running, so the bound this
  buys is "the budget, plus one formula" — which is what the per-formula character limit is
  for. Also a stop.

Plus a finite `maxSize` (the default is `Infinity`, which makes `\rule{500em}{500em}` a layout
bomb any author could leave behind), a finite `maxExpand`, `strict: 'ignore'` so that a page
cannot fill the server's log one warning per line, and a **fresh `macros` object per call** —
KaTeX writes `\gdef` definitions into the object it is handed, and this module is loaded once
per server process, so a shared one would let one formula redefine what every later formula on
the site means, on pages its author cannot edit.

A refusal names the limit, the number and the reason: *"gesetzt wird auf dem Server, den sich
alle Lesenden teilen"*. The fence's own source is printed above it.

### `throwOnError: false` is the nicety; the `try` is the guard

`throwOnError: false` converts a KaTeX **ParseError** into a rendered error node carrying the
author's source in red, and nothing else. KaTeX throws outside that path: two thousand nested
groups overflow the parser's own stack and arrive as a **RangeError**, which sails straight
through the option. That is the case `maths.test.ts` uses, because a malformed-but-parseable
formula would prove only that the option works.

## Consequences

- **No maths JavaScript reaches the reader.** The client bundle carries KaTeX's stylesheet —
  27 kB, 4.3 kB gzipped, fetched with the route that can hold a formula rather than with every
  page — and none of its code. A face is fetched only when a rule naming it matches something,
  so a page without maths downloads none of the 304 kB.
- **The server bundle grows to about 514 kB**, and `katex` joins `ssr.noExternal` for it. The
  production image ships no `node_modules`, so a bare specifier there is
  `ERR_MODULE_NOT_FOUND` on the first request; `web/scripts/check-server-bundle.sh` is what
  catches it, and `just build` runs it.
- **A formula's markup travels twice** — once in the server-rendered HTML, once in the page
  payload that hydration reads. That is the price of computing it where the library can be
  kept out of the browser, and for a formula of a few kilobytes against a library of 272 kB it
  is the better trade for every page that holds fewer than about eighty of them.
- **The editor shows a formula as its source.** `Editor.svelte` renders the same `BlockView`
  while TipTap mounts and is handed the same `formeln`, so the pre-mount view is typeset; the
  editing surface itself shows the fence, which is what editing a formula means. TipTap
  declares no new attribute and no new node — the schema is byte-identical (D-18).
- **A fence with no page load behind it says nothing.** `formeln` is `null` there, and the
  fence renders as source with no note: being refused and never being asked are different
  states and must not be reported as the same one.
- **` ```math ` is the only spelling.** `latex`, `tex` and `katex` are not aliases — each
  spelling admitted is one that has to be argued about again later, and an author who writes
  one is told the wiki does not know that language rather than silently given something else.
  ` ```text ` and ` ```plain ` remain D-18's escape hatch.
- **The formula map is a `Map`, and that is a bug fix rather than a style.** It is keyed by the
  fence's own text, which anyone with write access types, and it crosses the wire as page
  data: SvelteKit serialises that with `devalue`, whose parser **throws** on an object carrying
  a `__proto__` property (`node_modules/devalue/src/parse.js`). A page holding a ` ```math `
  fence whose whole content is `__proto__` would have failed to load at all.

## Switch-back criteria

Revisit if any of these becomes true:

- **A page carries enough maths for the payload duplication to hurt.** The escape is to strip
  the typeset markup out of the payload — SvelteKit has no supported way to send data to SSR
  only, so this would mean re-rendering in the browser, which is the client bundle again. The
  honest alternative at that point is a cache keyed by fence text, and there is nowhere to put
  one that is not `Block::attrs`, which D-18 forbids.
- **Inline maths is wanted.** It is not this decision extended; it is a global re-parse of every
  stored paragraph, a new `MarkKind`, and a full export run against a copy of production before
  anything ships. The plan's "Out of scope" section has the reasoning and it has not changed.
- **`trust` is wanted for one command.** It is all seven or none — the option is a boolean or a
  predicate over every one of them — and turning it on falsifies a sentence ADR 0007 leans on.
  The amendment is argued there, not here.
- **KaTeX's output stops being the only string that becomes markup.** The permission in
  `check-html-sinks.sh` is one line for one value. A second producer of HTML is a second
  argument, made from scratch, not an extension of this one.
