# Piece 5 — diagrams, formulas, highlighted code, and one reference that is real

**Design:** [2026-08-07-great-wiki-design.md](../specs/2026-08-07-great-wiki-design.md) §3.2 and
§10. Roadmap entry: **M4 — Block registry**. The owner asked for Mermaid diagrams,
syntax-highlighted code, KaTeX maths and document references, and decided against a block
registry — keep adding variants the way `TaskList` and `Attachment` were added.

**Revision 2**, after three adversarial reviews. Two objections were *blocking* and both were
about the same thing: the first draft said step 6 was the only step that touched the editor's
destructive mirror, and that was false — the document reference in step 5 (now step 6) touches
it, and touches it from both directions at once. Everything below has been re-verified against
the tree; every objection is either folded in or answered by name in
[§ Objections, and where each one landed](#objections-and-where-each-one-landed), so that
nobody re-raises one this document has already considered.

That decision about the registry turns out to be less of a departure than it sounds, and the
reason is the first thing this plan has to say.

## The registry M4 was supposed to register against was never built

The roadmap says of M4: *"The block-type trait and registry are defined in M3; M4 is
registrations against it."* They are not. A grep for a block registry across the whole
workspace returns exactly one hit — `crates/gw-core/src/block.rs:4`, the doc comment saying
the registry is *"planned for M4"*. M3 shipped block kinds by hand, and so has every piece
since: `TaskList` and `TaskItem` in piece 3, `Attachment` in piece 4.

So the owner's decision ratifies what the code already does rather than overruling a design.
**The roadmap sentence at `docs/superpowers/plans/2026-08-07-great-wiki-roadmap.md:55-57` is
wrong and should be corrected in this change**, because M9 (charts) and M10 (citations) both
add blocks, and the next person to read that sentence will go looking for a trait that has
never existed.

## Three of the four features need no new `BlockKind`, and that is the whole size of this piece

This is the finding that matters more than any other here, so it gets stated before the
decisions rather than after them.

**A fenced code block already carries its language end to end.** Not as something to be built —
as something that works today, at every one of the layers that could lose it:

| Layer | Where | What it already does |
|---|---|---|
| Importer | `crates/gw-core/src/markdown.rs:669-681` | stores the info string's first word as `attrs.language` |
| Exporter | `crates/gw-api/src/export.rs:1088-1108` | writes it back onto the fence, widening the fence past any backtick run inside |
| Round trip | `crates/gw-api/src/export.rs:501-516` | no `comparable()` reduction today, so the attribute is compared whole — and survives |
| Editor schema | `@tiptap/extension-code-block` | stock TipTap declares `language` and nothing else; pinned at `web/src/lib/editor/extensions.test.ts:245` |
| CRDT | `crates/gw-collab/src/fixtures.rs:107-109`, and the explicit test at `crates/gw-collab/src/doc.rs:791-799` | a `codeBlock` carrying `language`, newlines and tabs round-trips |

` ```mermaid `, ` ```math ` and ` ```rust ` are therefore the *same stored block*, differing
only in a string the renderer chooses to read. Mermaid, highlighting and display maths are
rendering decisions on an existing kind. They cost **zero mirrors, zero CRDT risk** and — once
§ D-18 below is built — bounded export risk, and they inherit the round-trip guarantee for free.

The doc comment on `BlockKind` enumerates four hand-maintained mirrors plus a fifth. Three of
this piece's four features do not touch a single one of them. That is the difference between
M4 being a large piece and a small one. The fourth — the document reference — touches the
dangerous one, and § D-21 is mostly about that.

## Step zero: two bugs on live content, and they are the same bug seen twice

Before any of it, because everything above renders through the thing that is broken.

### The reader destroys every code block on the site today

`web/src/lib/components/BlockView.svelte:89` renders a code block as
`<pre><code>{plainText(block)}</code></pre>`, and `plainText` ends with
`.replace(/\s+/g, ' ').trim()` (`web/src/lib/blocks/render.ts:156`). So every code block on the
live site is already one line with its indentation gone. A Mermaid source is newline-delimited;
`graph TD;\n  A-->B;` would arrive at any renderer as `graph TD; A-->B;` and parse as nothing.

The data is intact everywhere else — `crates/gw-collab/src/doc.rs:791-799` proves a code block
keeps its newlines, its tabs and its language through the CRDT, and the exporter writes them
out faithfully. Only the reader loses them. Nothing catches it because **no web test renders a
code block at all**: `codeBlock` appears in `web/src` tests only inside `extensions.test.ts`.

**The fix must not touch `plainText`.** Its own doc comment (`render.ts:137-143`) calls it a
byte-for-byte contract with `gw_core::Block::plain_text` (`crates/gw-core/src/block.rs:250-261`),
shared cases living in both test suites so that drift goes red; it feeds every heading anchor
id, the outline, a table's column labels and, at M7, the search index. Widening it to preserve
whitespace would drift the two implementations that two suites exist to keep identical. Read
the code block's text leaves directly instead, and add the missing reader test with a two-line
fixture.

### And the structural diff cannot see the thing this piece makes load-bearing

This is the objection the first draft missed entirely, and it is the more interesting half.

All three revision-diff modes go through `Block::plain_text()`, which ends
`out.split_whitespace().collect::<Vec<_>>().join(" ")` (`crates/gw-core/src/block.rs:257-261`):
`diff_prose` tokenises it (`crates/gw-core/src/diff.rs:120-124`), `diff_structure`
fingerprints a block as kind plus that string (`diff.rs:328-330`), and `diff_design` compares
attributes and marks only. Today that is harmless *because the reader is broken in the same
way* — nobody can see a fence's whitespace, so nobody can miss it. The moment step 1 fixes the
reader and steps 3–5 turn a newline into the difference between a drawn diagram and a broken
one, the diff stops being able to report the change that broke it.

So step 1 fixes both halves or neither. `diff_structure`'s `fingerprint` takes a code block's
own text **verbatim** rather than through `plain_text`; a new `Block::diff_text()` beside
`plain_text`, identical to it except for that one kind, keeps `plain_text` untouched and keeps
the mirror contract intact. `diff_prose` is deliberately left as it is: it tokenises on
whitespace by construction, so a pure re-indentation inside a fence is a **Struktur** change
and not a **Prosa** one. That split should be stated in `diff.rs`'s module docs rather than
discovered.

**One thing the review got wrong here, recorded so it is not re-raised:** the objection said
*"there is no revision-restore endpoint — `restore` in the API is trash-only"*. There is one.
`POST /api/revisions/{id}/restore` is routed at `crates/gw-api/src/routes/revisions.rs:212`,
`GET /api/revisions/{id}/source` at `:211`, and the history page has the button and the
confirmation dialogue (`web/src/routes/[...path]/history/+page.svelte:98-114`, `:133`). So a
whitespace-destroying revision is *recoverable*; what is lost without this fix is the ability
to **find** it — every tab reports "Keine Änderungen …"
(`history/+page.svelte:303`, `:317`, `:332`) for the revision that did it. That is a smaller
failure than the objection claimed and still worth the twenty lines.

## Owner's decisions, 2026-09-02

### D-18: A diagram, a formula and a highlighted listing are all one block kind

` ```mermaid ` draws, ` ```math ` typesets, and everything else highlights. All three are
`BlockKind::CodeBlock` with a `language` attribute, and no new variant is added for any of
them.

Rejected: a `Diagram` kind and a `Math` kind (they buy nothing — the block still has to be
*written* as a fence, so the importer would have to reclassify every ` ```mermaid ` fence in the
corpus by shape, which is exactly the guess [ADR 0015](../../decisions/0015-how-a-placed-file-is-written-in-markdown.md)
refused for a bare `![x](bild.png)` — and they cost all the mirrors including the one that
deletes) and a block registry (see above: it does not exist, and building one to hold three
registrations that need no schema change at all is infrastructure paid for by nobody).

**Consequence:** deciding that ` ```mermaid ` draws means there is no longer a way to *show*
mermaid source as source. That is GitHub's behaviour and defensible, but it is a rendering
change to stored content that no migration records. It costs nothing today — verified: no
` ```mermaid ` and no ` ```math ` fence exists in `content-example/` or `content-darm/` — but
the escape hatch must ship with it: ` ```text ` and ` ```plain ` never draw and never highlight.

**Second consequence:** the importer keeps only the info string's first comma- or
space-separated token (`markdown.rs:669-681`). Per-diagram options written *on* the fence are
destroyed silently at import, and the export round-trip cannot catch it, because the stored
block and the re-imported block agree — the loss happened before either existed. Mermaid's
configuration goes *inside* the fence, as its own `%%{init: …}%%` directive, which is part of
the diagram source and survives as text. What that directive is allowed to set is § D-19's
problem, not this one's.

#### The negative requirement, restated correctly after review

The first draft said: *"no second attribute may ever be added to `codeBlock`"*, and treated
that as sufficient. It is not sufficient, because **it binds the team and not a writer.**

Nothing validates block attributes on the write path. `read_attributes`
(`crates/gw-collab/src/doc.rs:402`) copies whatever the Yjs element carries,
`publish_revision` (`crates/gw-store/src/revisions.rs:345`) serialises the tree as given, and
the fixture `an_attribute_key_may_be_unicode_or_empty` (`doc.rs:857-861`) round-trips a
paragraph carrying `{"": "leer", "größe": 1, "a b": "c"}` — so arbitrary attributes reaching
`documents.body` is demonstrated, not inferred. `reduce()` (`crates/gw-api/src/export.rs:501-516`)
reduces `taskItem` and `Link` marks and nothing else. So anyone with write access on one page
can store `{"language":"rust","x":1}` on a code block through the collab socket and that page
is refused from every export from then on, permanently — a page silently missing from the
owner's backup, which is the `LINK_ATTRS` disaster in a new place.

**So the decision has two halves, and step 2 is the second one:**

1. **A third allow-list.** `CODE_BLOCK_ATTRS: [&str; 1] = ["language"]`, applied in `reduce()`
   exactly as `TASK_ITEM_ATTRS` is. An unrecognised attribute then cannot change the exported
   file *and* cannot delete a page from the backup.
2. **And still no second declared attribute, ever** — the allow-list does not make one safe, it
   changes the failure. A declared attribute is *minted* by ProseMirror's `computeAttrs` onto
   every block the editor touches (this is the `Anchor` doc comment's whole subject,
   `web/src/lib/editor/extensions.ts:104-130`); with the allow-list in place it would then be
   reduced away on both sides, so the comparison passes and the attribute is **silently absent
   from the exported markdown** instead of loudly refusing. A backup that quietly omits a
   theme is better than one that omits a page and worse than one that is correct. Theme, line
   numbers, highlight ranges, a cached SVG and any parse-error flag live in the renderer or in
   component state. Nowhere else.

The allow-list is a safety net for what writers can do, not a licence for what the schema may
declare. `crates/gw-api/tests/export.rs:651` and `:689` are the shape of the tests that pin
both halves, and both of those tests record in their own comments that the reduction they
protect *"was switched off without a test noticing"*.

### D-19: A diagram reaches the page as an image — and the CSP is what holds while it is drawn

Mermaid renders to an SVG string, and that string is set as the `src` of a plain `<img>` as a
`data:image/svg+xml` URI, with the diagram source as `alt`. The **rendered** markup never
enters this wiki's DOM.

The first draft said *"its markup never enters this wiki's DOM"* full stop, and a reviewer was
right that this is contradicted four sections later by *"Mermaid must never reach the server
bundle at all: it measures text by appending to `document.body`"*. Both cannot be true. The
correction matters, because the first draft used the `<img>` claim to justify accepting a
dependency with an active advisory stream, and the `<img>` is not what does that work.

**There are two barriers, in this order.**

**Barrier one, during `render()`: the Content-Security-Policy.** Mermaid needs the DOM to
measure text, so during rendering the diagram — labels included — is inside `document.body`,
and the `<img>` protects nothing yet. What protects the page in that window is the policy:

- `script-src ['self']` with no `'unsafe-inline'` and no `'unsafe-eval'`
  (`web/vite.config.ts:57`) refuses an inline `onerror`/`onload` handler that escaped mermaid's
  own sanitiser.
- `style-src ['self']` (`web/vite.config.ts:60`) refuses an injected `<style>` element, which
  is the CSS-injection class.
- `img-src ['self', 'data:']` (`web/vite.config.ts:91`) refuses a remote beacon.
- `object-src ['none']`, `frame-src ['none']`, `base-uri ['none']` (`:108`, `:104`, `:113`)
  close the rest.

**And that barrier is weaker in development, which the plan must say out loud.**
`web/src/lib/csp.ts:20-22` and `:41-46` record that SvelteKit adds `'unsafe-inline'` to
`style-src` in **development** so it can inject its own component styles. So under
`npm run dev` the CSS-injection class is open, and `themeCSS` — which a diagram can set through
`%%{init}%%` — is the vector. A production-only difference is the worst kind, and this is the
second one in this repo.

**Barrier two, afterwards: the `<img>`.** ADR 0014's constraint is about the *mechanism*, not
about where the bytes came from: an SVG may be shown through `<img>` or a CSS `background-image`
— contexts no browser executes script in — and never through `<object>`, `<embed>`, `<iframe>`,
*or by putting its markup into this wiki's own DOM*. Its switch-back criteria answer this exact
question in advance: *"An SVG needs to render in the page. `<img src>` on the existing address
already works and is safe… What would need this decision changed is wanting the markup in this
wiki's DOM, and the answer to that is a rasterised copy, not a sanitiser."* Generated SVG is
not exempt because we generated it — the bytes are a function of text somebody with write
access to one page typed. `BlockView.test.ts:400-420` already asserts this shape for an
attached SVG and is the model for the diagram's test.

Rejected: `{@html svg}` or `el.innerHTML = svg` (the mechanism ADR 0014 names as executing *in
this origin, with the session cookie in reach*) and `securityLevel: 'sandbox'`, which every
Mermaid advisory recommends as its workaround — it emits `<iframe src="data:text/html;base64,…">`,
and `frame-src` is `['none']`; loosening that to admit `data:` would hand a general
XSS-hosting primitive to a policy whose author wrote `frame-src 'none'` on the grounds that
*"nothing is embedded and nothing embeds this"* (`vite.config.ts:100-104`).

**Because the CSP is barrier one, the library settings are decisions and not advice.** The
first draft filed these under "unverified" as suggestions; they move here:

- **`securityLevel: 'strict'`** and never `'loose'`, `'antiscript'` or `'sandbox'`.
- **`bindFunctions` is never called**, which is what would wire a diagram's `click`
  interactions. Not calling it is the belt to `'strict'`'s braces.
- **An explicit `secure` list**, because the default one is short. It must at minimum cover
  `dompurifyConfig` (which would let diagram text weaken the sanitiser meant to be protecting
  the page from it), `themeCSS`, `themeVariables`, `htmlLabels`, `fontFamily` and
  `altFontFamily`.
- **`mermaid.render(id, text)` — the two-argument form that returns `{ svg }` — and never
  `mermaid.render(id, text, container)`.** The three-argument form performs the DOM write on
  your behalf: the insertion ADR 0014 forbids would happen one stack frame down inside the
  dependency, and it appears nowhere in our source as a sink. This is the objection that
  motivates widening the sink check in step 1; see § "the reader constructs no untrusted HTML".
- **The measuring container is ours, appended and removed in a `finally`**, so a mermaid throw
  does not leave attacker-authored markup parked in the live document.
- **The input is capped before mermaid is called at all** — see § "the availability class".

**Consequence:** `img-src ['self', 'data:']` already permits a data URI, so **no CSP directive
moves**. It also disposes of the `<style>`-element problem for the *rendered* diagram: mermaid
inserts a `<style>` as the SVG's first child and exposes no nonce hook, so in the DOM it would
be refused by `style-src ['self']`; inside an `<img>` that `<style>` is the image's own
business and applies to nothing else.

**Consequence, the cost:** an `<img>` cannot inherit `prefers-color-scheme`, so the diagram must
be re-rendered on a theme change. Text inside it is not selectable and not searchable by the
browser's find. Both are real losses and both are cheaper than the alternative.

**What must never be done to fix a styling symptom:** add `'unsafe-inline'` to `style-src`.
Beyond being the one loosening [ADR 0007](../../decisions/0007-content-security-policy.md)
refused, it would make `widenCspNonceToStyles` skip the directive (`web/src/lib/csp.ts:71-73`)
and silently strip the nonce TipTap depends on — unstyling the *editor*, in production only.

### D-20: A formula is typeset on the server, capped, and inside a try/catch

KaTeX renders through `renderToString` during SSR, so maths is in the first response and works
with JavaScript off. The `trust` option is never passed, never set to a function, and never set
to `true`.

Rejected: client-side rendering (a medical reference should not need a bundle to show a dose
formula) and MathJax (larger, and its own `<style>` injection would land back in D-19's
problem).

**Why `trust` is the whole of the safety story.** It defaults to `false`, and that single
default disables the dangerous command family in one go — `\href`, `\url`, `\includegraphics`,
`\htmlClass`, `\htmlId`, `\htmlStyle`, `\htmlData`. There is nothing to configure; the safe
setting is the one you get by not typing anything. KaTeX also needs **no CSP change at all**:
its output carries inline `style="…"` *attributes*, which `style-src-attr ['unsafe-inline']`
already permits (`vite.config.ts:81`); its stylesheet is a real file; its `woff2` faces are
fingerprinted assets under `font-src ['self']`, vendored into `static/fonts` like every other
font here, because *"nothing is fetched from a CDN and nothing may start being"*
(`vite.config.ts:85-86`).

**Consequence:** `\htmlStyle` must stay unreachable, and not only for its own sake. ADR 0007
justifies `style-src-attr ['unsafe-inline']` partly on the sentence *"the renderer does not emit
authored CSS into one either way"* (`vite.config.ts:80`). Enabling `\htmlStyle` would put
author-written CSS declarations directly into a style attribute and falsify that sentence — so a
future `trust: true` is not a config tweak, it is an ADR 0007 amendment.

#### The server-side renderer gets the stricter budget, not the looser one

Two reviewers were right that the first draft spent its whole denial-of-service budget on the
wrong feature. Mermaid runs in the reader's own tab; KaTeX runs on the shared, single-threaded
SSR path. There is **no `export const ssr = false` anywhere under `web/src`** — verified, the
grep for `ssr`/`csr`/`prerender` returns nothing — so `BlockView` renders on the server for
every reader on every page load, and `[...path]/+page.server.ts` is a dynamic load with no
prerender and no cache.

So, in order of what fails first:

- **A try/catch around `renderToString`, inside the leaf component.** `throwOnError: false`
  converts a KaTeX **ParseError** into a rendered error node; it is not a general catch, and
  KaTeX throws outside that path. An uncaught throw inside a Svelte component during SSR is a
  500 for the whole route — and the route *is* the edit surface
  (`web/src/routes/[...path]/+page.svelte`), so the page could not be repaired through the
  editor either. The German error line is emitted from the catch as well as from
  `throwOnError: false`, so the library option is a nicety and the try/catch is the guard.
- **A per-fence input cap and a per-page fence count cap**, both checked before
  `renderToString` is called. Over either cap, the fence renders as ordinary code with a German
  note. This is the cap the first draft wrote for Mermaid and forgot here.
- **A finite `maxSize`.** The default is `Infinity`, which makes `\rule{500em}{500em}` a layout
  bomb any author can leave on a page.
- **A finite `maxExpand`**, which is what bounds macro expansion.
- **`macros: {}` constructed fresh per call** — KaTeX writes `\gdef` definitions into whatever
  object it is handed, so a shared one lets a formula redefine what every later formula means.

State the asymmetry in the ADR: *the renderer that runs on the server gets the tighter budget
than the one that runs in the reader's tab, because a slow diagram costs one tab and a slow
formula costs the deployment.*

### D-21: A document reference is a link, resolved by identity, written as a scheme — and it touches the mirror that deletes

`[Titel](dok:<id>)` in markdown; `MarkKind::Link` carrying `doc` in the store. Both directions
of the syntax live in `gw_core::markdown`, beside `attachment_destination` and
`attachment_reference` (`crates/gw-core/src/markdown.rs:61-92`).

Rejected: exporting the resolved *path* — which is what D-5 says export does, and which is the
one combination that permanently breaks the backup. The file would hold `[Titel](/pfad)`;
`gw_core::markdown` deliberately imports that as `link_to_url`, never as a doc reference,
because *"this crate has no store, so a markdown link can never be resolved to a document id
here"*; the stored `{doc: id}` and the re-imported `{href: "/pfad"}` differ, `comparable()`
compares link attrs against `LINK_ATTRS = ["href", "doc"]` (`export.rs:432`) and finds one
missing and one extra, and the page is refused. Also rejected: a new `BlockKind` for it — a
reference to a page inside a sentence is inline, and the mark already exists.

**The reasoning is [ADR 0015](../../decisions/0015-how-a-placed-file-is-written-in-markdown.md)'s,
generalised.** A scheme rather than a bare path because nothing predating the feature can
contain one and nothing writes one by accident; both halves in the lower crate because two
copies in two crates drift and the cost of drift is an export that refuses every page holding a
reference.

Everything that follows is what the first draft got wrong.

#### D-21a: This is a mirror edit, and it is the dangerous mirror

**The first draft's ordering rationale said step 6 was "the only step that touches the mirrors —
including the one that deletes". That is false, and it was the most expensive error in the
document.**

`web/src/lib/editor/extensions.ts:132-136` is
`const Anchor = Link.extend({ addAttributes: () => ({ href: {…} }) })` — `href` and nothing
else. `doc` is undeclared. That file's own doc comment (`extensions.ts:104-130`) and
`crates/gw-core/src/block.rs:8-14` both state the rule: an attribute the schema does not
declare is deleted from the Y.Doc by `updateYFragment`'s closing pass and broadcast to every
other connected editor.

Traced end to end in the installed dependency, `web/node_modules/@tiptap/y-tiptap/dist/y-tiptap.js`:

- `attributesToMarks` (`:1500-1510`) builds marks with `schema.mark(name, attrs)`, and
  ProseMirror's `computeAttrs` iterates **declared** attributes only, so `{doc: "01J…"}` becomes
  `{href: null}`.
- `marksToAttributes` (`:1516-1525`) writes `mark.attrs` back **whole**, with no null filtering.
- Node attributes *are* null-filtered — `createTypeFromElementNode` skips a null value
  (`:1039-1044`, `if (val !== null && key !== 'ychange')`) and `updateYFragment` removes one
  (`:1550-1560`). **Marks are not.** That asymmetry is exactly why `TaskItem`'s
  `id: { default: null }` is harmless (`extensions.ts:161-173`) and why the same shape on a
  mark is not.
- `gw-collab` reads it straight back: `attrs_to_marks` (`crates/gw-collab/src/doc.rs:328-341`)
  copies the map verbatim with no null handling, and `Mark`'s serde skips the map only when it
  is **empty** (`crates/gw-core/src/block.rs:180-181`), so `{"href": null}` reaches
  `documents.body`.

**What that costs if step 6 ships without the mirror edit.** An author writes
`[Blutbild](dok:01J8…)` on a page. It stores as `Mark::link_to_doc` (`block.rs:186-190`),
records a `links` row, renders, exports and re-imports cleanly. A second person opens the page
in the editor — `CollabDoc::from_block` (`doc.rs:86`) seeds the room from the stored body — and
types one word in that paragraph. `equalYTextPText` sees `{doc:"01J8…"}` against `{href:null}`
and rewrites the Y.Doc. The document id is gone from the CRDT, broadcast, and filed as a
revision by the next sweep; the reader falls through to plain text, so nothing on the page shows
the reference ever existed. Then the backup: `Renderer::wrap` (`export.rs:819-831`) finds no
`href` string, calls `self.problem("a link has no \`href\` to write …")`, and `render_file`
refuses the page — which is then missing from the export directory (`export.rs:209-218`) while
`.export-fidelity` claims the directory is a faithful copy of the database.

#### D-21b: And the obvious fix re-runs the `LINK_ATTRS` disaster, because the existing reduction is a false friend

The repo's own precedent tells an implementer that the cure for "undeclared is deleted" is to
declare the attribute. On its own, that is the same incident from the other side.

`LINK_ATTRS` is `["href", "doc"]` (`export.rs:432`) and `reduce()` retains every key on that
list **without inspecting its value** (`export.rs:508-511`,
`mark.attrs.retain(|key, _| LINK_ATTRS.contains(&key.as_str()))`). So the allow-list that exists
*specifically* to stop this failure is blind to a minted `doc: null`: it keeps it. Declare
`doc: { default: null }` on `Anchor` and nothing else, and any page holding an ordinary external
link — the DGVS-Leitlinie link on a Darm page, say — is refused the moment somebody edits it:
`computeAttrs` fills the declared default, `marksToAttributes` writes
`{href: "https://…", doc: null}` to the Y.Doc, `attrs_to_marks` stores it, and `comparable()`
then reduces the stored mark to `{"doc":null,"href":"https://…"}` against the re-imported
`{"href":"https://…"}`. That is the `target`/`rel`/`class`/`title` incident with the same blast
radius, arrived at from the other direction.

**So the fix is both edits, in the same commit, and neither alone:**

1. **`Anchor` declares `doc: { default: null, rendered: false }`**, exactly the shape
   `TaskItem`'s `id` uses (`extensions.ts:161-173`), for exactly its reasons: `rendered: false`
   because a document id is database identity and not markup, `default: null` because the
   markdown importer mints none. The pin at `extensions.test.ts:224` moves in the same change,
   and a new case asserts a `doc`-carrying mark survives the round trip through the Y.Doc.
2. **`reduce()` drops a null-valued attribute from the allow-listed set.** One line — retain on
   `LINK_ATTRS.contains(key) && !value.is_null()` — applied symmetrically to marks and to the
   block allow-lists, so a minted null can never differ from an absent key. This is the guard
   the first draft stated forcefully for `codeBlock` and not at all for `Link`, where the
   danger is live.
3. **A test that pins each half**, in the shape of `export.rs:651` and `:689`, plus mutation
   entries in `scripts/mutate.sh`: removing the null rule must go red, and removing `doc` from
   `Anchor` must go red.

#### D-21c: The read sink — the `doc` branch is evaluated before `safeHref`

`web/src/lib/components/BlockView.svelte:202` tests `{#if typeof doc === 'string'}` first;
`safeHref` is consulted only in the `else if` at `:208`. And a `doc` value is **not** a uuid by
construction: `Mark::target_doc` (`crates/gw-core/src/block.rs:204-209`) returns whatever
string is there, `attrs_to_marks` (`doc.rs:328-341`) copies an arbitrary JSON object off the
Yjs attribute, and nothing between the collab socket and `documents.body` validates it.

So the rule, stated because the obvious implementation violates it:

> **The `doc` value never becomes an `href`.** Resolution happens on the server and yields a
> *path*; that path goes through `safeHref` (`web/src/lib/blocks/render.ts:76-87`) like every
> other address, on the same sink. An unresolved, forbidden, trashed or self-referencing target
> renders as **text** — the fallthrough the branch already takes today — and never as an
> anchor. Nothing in the reader ever interpolates `mark.attrs.doc` into an attribute.

`render.ts`'s own doc comment says the CSP is the second line and `safeHref` is the first, *"the
only one of the two that still works wherever the policy is not in force"*. A `javascript:` URL
smuggled in as `{"doc": "javascript:…"}` over the collab socket is the exact case it was written
to catch, on the one branch it does not cover.

#### D-21d: The authorisation rule, which the first draft wrote for the step it was not building

The first draft wrote the disclosure rule in full for transclusion and, for the step that ships,
said only that a restored reference *"degrades to exactly the 'target not available' state the
rendering rule already defines"*. **No such rule exists** — the reviewer checked ADRs 0001–0015
and the code, and the only thing there is `BlockView.svelte:202-207` rendering a non-navigating
`<span data-doc>` with a comment calling resolution "Task 7's job". So step 6 would invent the
rule with no requirement written down, on the one step that touches `gw-store`, and the
precedent it would reach for is in the module it is extending: `links::replace_links` is
*deliberately unfiltered* and says so (`crates/gw-store/src/links.rs:287-293`), with the whole
disclosure property carried by the read side.

**So the rule ships with the step:**

> A reference is resolved through the store, per target, against the caller. A target the
> caller may not read, one that does not exist, one that is in the trash, and the host page
> itself all render **identically**: the reference's own text, unlinked, with no title, no
> path, no excerpt and no count. The verdict is re-asked on every read, so a revoked grant
> empties the reference at the next request with nothing to clean up.

Rejected, and named because it is the natural shape: a batch `ids → (path, title)` query with
no per-target check, on the reasoning that the ids came out of a body the caller may already
read. A `dok:` reference to a page the reader has no access to would then render as a working
link to that page's path — disclosing existence and location that `backlinks_for` and
`graph_for` explicitly refuse to disclose (`links.rs`), and a click yields 403 rather than 404
because `crates/gw-api/src/routes/docs.rs:55-69` splits them deliberately.

**And there is no unfiltered picker.** To be usable by a human this feature wants a
path-or-title → id lookup. An unfiltered one is a whole-corpus existence-and-title oracle for
exactly the attacker in the threat model — somebody with write on one page who wants to know
whether `/darm/befund-mueller` exists. Whatever the picker is, it filters the same way a
reference does, or it is not built in this step.

#### D-21e: Per-reference authorisation is a whole-deployment availability lever

`Store::open` sets `max_connections(1)` (`crates/gw-store/src/lib.rs:84-86`), and that is
deliberate and load-bearing — every query in the application is serialised through one SQLite
connection. One `Store::document_access_id` (`crates/gw-store/src/acl.rs:549-558`) costs
`baseline_for` (`acl.rs:336`) plus `document_path_unchecked` plus `document_access_with_baseline`,
and the hoisted `_with_baseline` variants are `pub(crate)` (`acl.rs:395`, `:529`, `:562`, `:591`)
and unreachable from `gw-api`. So a resolver living in `gw-api` pays a fresh baseline per
reference. The first draft noticed this for transclusion, which it was not building, and not for
D-21, whose per-reference shape is identical and which ships.

Nothing caps mark count — the body is JSON over the collab socket — so a page carrying a few
thousand `dok:` marks would serialise on the order of 10⁴ queries through the connection every
other request in the deployment needs, and repeatedly fetching one attacker-controlled page
stalls the whole wiki for every reader.

**So the resolver lives in `gw-store`**, beside `backlinks_for` and `graph_for`, which is where
the baseline can be hoisted once — exactly the hoist `document_access_id_with_baseline` exists
for — and it **caps how many references it resolves per page**. Over the cap, the remainder
render as text, which is the same state a forbidden target renders in, so the cap discloses
nothing. A resolver written in `gw-api` is the rejected alternative, and the reason it is
rejected is the `pub(crate)` on those four functions.

#### D-21f: The writer half refuses what it cannot round-trip

`Renderer::wrap` interpolates a link destination with **no escaping at all**:
`format!("[{inner}]({href})")` (`export.rs:819-820`). Combined with `doc` being an arbitrary
attacker-controlled string, a `dok:` writer of the same shape emits attacker text straight into
the backup file: a stored `doc` of `x) [siehe](https://angreifer.example/` exports as
`[Titel](dok:x) [siehe](https://angreifer.example/`. The round-trip comparison catches that as
a refusal rather than a corruption, so the cost is another permanently unexportable page — but
only because the guard happens to fire, and "happens to" is not a design.

The model to copy is in the crate this step is already editing: `attachment_destination`
(`markdown.rs:61-92`) returns `None` for anything its own reader would not give back unchanged,
and the exporter turns that into a refusal that names the page. So: **validate that the
destination is a uuid and nothing else.** `documents.rs:127` mints `Uuid::now_v7().to_string()`,
and `documents.rs:504-507` already leans on *"a uuid cannot contain a colon, and that is the
whole reason this is safe"*.

#### D-21g: Check the live database before the scheme exists

The first draft was careful about this for inline maths — insisting on a full export run
against a copy of production before `ENABLE_MATH` — and asserted for `dok:` that *"nothing
predating the feature can contain one"* as a fact. The same one-query check is owed to the
feature that ships and not only to the one that was cut.

The corpora are clean: `grep -rn 'dok:'` over `content-example/` and `content-darm/` returns
nothing, as does `mermaid`, ` ```math ` and `$`. The live wiki is where the risk sits, and this
plan could not inspect it. A stored link mark whose `href` is literally `dok:etwas` — typed as a
URL by anyone with write access at any point in the past — round-trips fine today
(`export.rs:819-820` writes `[T](dok:etwas)`, and `gw_core::markdown` imports it back as
`link_to_url`). The moment the importer learns the scheme, the same file re-imports as
`{doc: "etwas"}`, `comparable()` compares it against the stored `{href: "dok:etwas"}`, and the
page is refused forever — a page nobody edited becoming unexportable, which is the exact failure
mode this plan names as disqualifying for inline maths.

**So step 6 opens with one `SELECT` against a copy of production**, for link marks whose `href`
begins `dok:`. If any exist, they are migrated to `doc` before the importer changes, in the same
change.

#### D-21h: What identity buys and what it costs

**Consequence, stated because it is the asymmetry somebody will trip over:** an export re-seeded
into a fresh database mints new document ids — `SeedMeta` has no `id` key — so every `dok:`
reference in a restored corpus points at nothing. Ordinary links do not have this problem,
because they survive as hrefs and `replace_links` re-resolves the path to an id on publish. So
identity buys move-safety and loses restore-safety. The switch-back criterion: adding `id` to
frontmatter so the seeder preserves identity would make this decision cheaper, and is a separate
one. A restored reference degrades to the unresolved state D-21d defines, so a restored corpus
is *readable*, not broken.

**Consequence:** `FIDELITY_WARNING` (`export.rs:102-120`) enumerates precisely what the format
can and cannot carry, and it currently says *"an internal link resolved to another page rather
than a URL"* is among the things dropped on the way in. A new scheme that is not named there is a
silent change to what the backup means. It moves in the same change.

### D-22: Transclusion is a separate question, and it is not answered yet

"Document references" has two readings and the researchers split on it. The design spec lists
*"embeds, document-reference"* as two block types (§3.2), which reads as transclusion; but
`MarkKind::Link` with a `doc` attribute already exists, is already admitted through the export
comparison by `LINK_ATTRS`, is already drawn by `BlockView` as a non-navigating `<span data-doc>`,
and *nothing in this system writes one* (`crates/gw-store/src/links.rs:8-13` says so outright).

So one reading — "link to a page by identity, so a rename does not break it" — is **finishing
machinery that is already half-built**, and it is D-21. The other reading — "show that page's
content here" — is a new `BlockKind`, the only one in this piece, and the only step that can
destroy content. The two share nothing but a name.

**This plan builds D-21 and stops.** If the owner wants embedded pages, the rule below is what
governs them, and it is written now so that the decision is a yes/no rather than a design
exercise.

## The transclusion permission rule, if it is ever built

Every aggregate view in this system filters the same way: narrow cheaply in SQL, put each
survivor through `Store::access_to` — *"the whole of the authorisation in this crate"* — and
omit anything that comes back `None`, never listing it and never counting it. `backlinks_for`
does it, `graph_for` does it for both ends of every edge, `board_for` does it per card.

**Transclusion is the first surface where omission is not available**, and that is the whole
difficulty. A reference block is not something a view synthesised; it is the host page's own
content, sitting in `documents.body`, which is served *verbatim* to anyone who may read the host
page. Deleting the block from the rendered output hides nothing — the target id is already in
the JSON the reader was handed — while making the page differ from what its author wrote and
from what the editor and exporter hold. For the same reason,
[ADR 0011](../../decisions/0011-what-a-topic-discloses.md)'s stronger shape ("answer as if it
does not exist") is unachievable here.

**And the identifier is not opaque.** Ids are `Uuid::now_v7()` (`crates/gw-store/src/documents.rs:127`),
whose first 48 bits are a millisecond timestamp. The rule below is still right — the id genuinely
is already on the wire, so withholding the block would hide nothing — but the ADR must say what
the identifier itself discloses rather than implying it discloses only "the fact": a reader who
may not read the target nonetheless learns the millisecond at which that page was created, which
is enough to correlate a hidden page with a known event. That belongs in the disclosure
paragraph, next to the note that `docs.rs:55-69` is where the 404/403 split is decided.

So the rule is [ADR 0009](../../decisions/0009-who-may-learn-a-board-card-s-assignee.md)-shaped —
the identifier was already on the wire, so withhold the name and the content, not the fact:

> A reference whose target the caller may not read renders **in place**, as one neutral German
> sentence, and renders **identically** to a reference whose target does not exist, has been
> trashed, or is the host page itself. The target is resolved through
> `Store::document_access_id` — the same accessor a page read ends in — and through nothing
> else. The title, the path, any excerpt and any count are withheld. The block itself stays.
> The verdict is re-asked on every read, so a revoked grant empties the embed at the next
> request with nothing to clean up.

Four rules follow from it, each with a reason:

**Depth is 1.** A transcluded page's own references are not expanded; they render as a link the
reader may follow through the ordinary page route, where the 404/403 split is already decided
and already licensed for a path a human typed. Two reasons, and the second is the load-bearing
one: it closes the cycle question by construction rather than by remembering to carry a visited
set, and it bounds the authorisation cost — the same `max_connections(1)` arithmetic as D-21e,
raised to the power of the depth. Raising the depth makes the visited set mandatory and moves
the expansion into `gw-store` so the baseline can be hoisted once, exactly as `graph_for` and
`board_for` do — which is where D-21e already puts the reference resolver, so the machinery
would be there.

**A cycle is allowed to exist and is stopped at render, never at write.** Refusing A's publish
because B references A would require reading pages the author may not read — and D-3 makes
membership per document, so that is normal rather than exceptional. It cannot be enforced in the
schema either: `links` is a plain edge table with no acyclicity constraint.

**An expansion carries the embedded page's own `Anhänge` list, or it does not render.** Reusing
the host page's list makes every picture inside the embed render the German "not attached"
sentence — a false statement about somebody else's page. Guessing an address instead is worse:
D-16 authorises a download against the page it was reached through, and there is deliberately no
`/blob/{sha}`. The list is fetched server-side with the caller's cookie through
`GET /api/attachments/{path}`, exactly as the host page's own list is.

**A reference records an ordinary `links` row, and contributes nothing to `plain_text`.** The
row, because an embed is at least as strong a connection as a link and `collect()` walks marks
only (`links.rs:75-92`) — a block attribute is invisible to it — and because inheriting
`graph_for`'s both-ends filter is free. Nothing to `plain_text`, because that is computed in
`gw-core`, which has no store and cannot ask a permission question; feeding another page's words
in would put content the reader may not see into this page's anchor ids and, at M7, its search
index. The known cost is the one ADR 0015 already records for placements: `diff_structure`
fingerprints a block as kind plus text (`diff.rs:328-330`), so two references look alike to the
structural diff and swapping one target for another shows up as a *design* change.

## What this shares with everything already built

**Nothing goes red when you add a variant**, and this is worse than `#[non_exhaustive]` implies.
There is no exhaustive `match` on `BlockKind` anywhere in the workspace — the exporter's dispatch
carries a wildcard arm by design, so it compiles fine. The only compile-time completeness checks
are in TypeScript (`satisfies readonly BlockKind[]` and `AssertNever` at
`extensions.ts:326-330`) and they bind `render.ts`'s hand-written union, not the Rust enum. A
Rust-only addition passes `cargo test --workspace`, `npm run check` and `npx vitest run` cleanly
while TipTap's deletion path is live.

`MarkKind` has a guard against this and `BlockKind` does not:
`crates/gw-core/src/markdown.rs:1672-1692` holds an exhaustive `match` inside a `#[test]`,
written so that *"adding a kind stops this test compiling until someone decides where it nests."*
**Write its `BlockKind` twin in step 1**, before this piece adds anything — it costs a dozen
lines, and the next block added anywhere in this system is the one it protects.

**A refusal is a partial backup, not a failed run.** The briefing for this work had it wrong, and
so does most of the tree. `run()` pushes a `Refused` and `continue`s (`export.rs:209-218`),
writing every other page; what fails is the CLI exit code, via `is_complete()` and `bail!`
(`crates/gw-api/src/main.rs:272-285`). The correction makes it worse rather than better: the
directory exists, is missing pages, and looks like a backup. `ExportReport` says so on every run
and `FIDELITY_WARNING` leaves the sentence in the directory, which is the only reason this is
survivable.

The first draft proposed correcting one comment. The claim is written in **sixteen** places, and
they were counted:

`crates/gw-core/src/block.rs:20` · `crates/gw-core/src/frontmatter.rs:105` ·
`crates/gw-api/src/export.rs:370`, `:453`, `:487`, `:1017` ·
`crates/gw-api/tests/export.rs:604` · `crates/gw-api/tests/export_markdown.rs:400`, `:676` ·
`crates/gw-store/src/topics.rs:918` · `web/src/lib/editor/extensions.ts:119`, `:225` ·
`web/src/lib/editor/extensions.test.ts:222` ·
`docs/superpowers/specs/2026-08-15-links-topics-tasks-design.md:291` ·
`docs/operations/status-2026-08-19.md:69` ·
`docs/decisions/0015-how-a-placed-file-is-written-in-markdown.md:13-14`.

The last one is an **Accepted ADR**, and it is the one that matters most, because this plan
proposes a new ADR *"exactly parallel to ADR 0015"* — written from 0015's opening paragraph, the
new ADR would re-publish the error in a fresh decision record. An Accepted ADR is a record of
what was decided and when, so it is not silently edited: it gets a dated **Correction** note
under its consequences saying what a refusal actually costs. The other fifteen are comments and
are simply fixed, in the step that first touches each file, with the `block.rs` one fixed in
step 1 because step 1 is already in that file.

**The reader constructs no untrusted HTML, and this piece is the first thing that would change
that.** `{@html}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `setHTMLUnsafe`,
`createContextualFragment`, `document.write` and `srcdoc` appear nowhere under `web/src` — the
grep returns one hit and it is `BlockView.test.ts:418`, an assertion that those strings are
*absent* from rendered output. `BlockView.svelte:40-41` states that as the reason there is no
sanitisation step. D-19 keeps it true for Mermaid by construction: an `<img src>` is an
attribute, not markup. KaTeX is the one place a rendered string reaches the DOM.

So, and this is wider than the first draft's version of the check:

- Confine KaTeX to a single leaf component with a doc comment saying what makes its input safe,
  and amend `BlockView.svelte`'s comment in the same commit rather than leaving it stale.
- **The check covers the whole sink class, not two spellings**: `{@html`, `.innerHTML`,
  `.outerHTML`, `insertAdjacentHTML`, `setHTMLUnsafe`, `createContextualFragment`,
  `document.write`, `srcdoc`, `new Function` and `eval(`.
- **And it covers the sink that is not in our source at all**: `mermaid.render` with three
  arguments. Handing mermaid a container makes it perform the write on your behalf, so the
  check greps for the three-argument call and fails on it, with a comment saying why a
  hand-written grep is checking a dependency's call signature.
- **The exemption list is a named file list, and it starts empty.** Step 1 ships the check with
  zero exemptions, because zero is the true state today. Step 4 adds exactly one entry — the
  KaTeX leaf — in the same commit as the file. The first draft scheduled the check before the
  file it exempts and never said the exemption was coming, which reads to the next implementer
  as a prohibition and pushes them toward `insertAdjacentHTML` or `setHTMLUnsafe` — spellings
  the narrow check would not have caught — and away from the SSR rendering D-20 chose
  specifically so maths works with JavaScript off.

**Svelte's `style:` directive is the CSP-legal way to colour a token**, and it is already used
by `BlockView.svelte:99` and `:103` for table alignment. It is exactly what a token-returning
highlighter needs, and it is already paid for by `style-src-attr ['unsafe-inline']`. **But the
value bound to it comes from the theme and never from the fence's text or from `attrs.language`**
— that is the same argument D-20 makes about `\htmlStyle`, and a violation would falsify the
same sentence in `vite.config.ts:80` (*"the renderer does not emit authored CSS into one either
way"*) that ADR 0007 leans on to justify `style-src-attr`.

**And `attrs.language` is an unvalidated, attacker-reachable string.** The importer keeps the
info string's first token whatever it is (`markdown.rs:669-681`), and over the collab socket it
is arbitrary. So it is a **key looked up in an allow-list map** and nothing else: never
interpolated into a dynamic `import()` specifier, never into a class name, never into a `style:`
value.

**The production web image ships no `node_modules`**, and `docker/gw-web.Dockerfile:68-84`
refuses a server bundle that imports any bare specifier — with the error
`REFUSING: the server bundle imports packages this image will not contain`. KaTeX rendering on
the server therefore has to go into `ssr.noExternal` — **and so does the highlighter**, which is
equally server-rendered, because `BlockView` is SSR'd and `style:` needs the token colours in the
first response. `web/vite.config.ts:131` currently reads `noExternal: ['@ark-ui/svelte']`; both
libraries join it. Mermaid must never reach the server bundle at all: it needs the DOM to measure
text, so it is loaded through the same `browser`-guarded dynamic-import shape as `loadEditor`, for
the reason `web/src/routes/[...path]/+page.svelte:122-142` already documents — a bare `import()`
inside an `{#if}` still emits the chunk.

**The gate does not run that assertion, and the first draft claimed it did.** `web/package.json:8`
is `"build": "vite build"`; `justfile:45-46` is `build: npm run build`; `justfile:270-279`
`agent-ci` runs `just lint`, `just test`, `just build`. The `REFUSING:` check lives **only** in
the Dockerfile and runs **only** during `docker build`. So a missing `ssr.noExternal` entry passes
every gate command green and fails at image build, after review. Steps 3 and 4 therefore end on
the Dockerfile's own pipeline run against `web/build` — the same
`find build -name '*.js' -not -path 'build/client/*' | grep -E "^(import|export)[^']*from '[^']+'"`
— added to the gate for those steps rather than trusted to `npm run build`.

## Order of work

| | Step | Touches | Depends on |
|---|---|---|---|
| 1 | Code blocks keep their newlines in the reader **and in the structural diff**; the `BlockKind` guard test; the widened HTML-sink check | `web`, `gw-core` | — |
| 2 | `CODE_BLOCK_ATTRS` in `reduce()`, so a writer cannot delete a page from the backup | `gw-api` | — |
| 3 | Syntax highlighting, from tokens rather than markup; `ssr.noExternal`; bundle assertion | `web` | 1, 2 |
| 4 | Display maths as a ` ```math ` fence, typeset during SSR, capped and caught | `web` | 1, 2, 3 |
| 5 | Mermaid, rendered to an `<img>` | `web` | 1, 2 |
| 6 | Document references: the production check, the editor mirror, the `reduce()` null rule, both syntax directions, uuid validation, the filtered batch resolver, the reader sink | `gw-core`, `gw-api`, `gw-store`, `web` | — |
| 7 | Transclusion — only if the owner asks for it | all five crates and `web` | 6 |

**Why this order, and it is not a preference.**

Step 1 is a bug on live content: every code block on the site is already broken, and steps 3–5
all render through the thing it fixes. It is also the cheapest step in the piece and the one that
leaves behind the guard test and the sink check that protect steps 4, 5 and 7.

Step 2 is new in this revision and is two lines plus two tests. It precedes anything that makes
code blocks worth writing, because until it lands any writer can permanently remove a page from
the owner's backup by putting a second attribute on a fence — and the feature this piece ships is
precisely the one that gives people a reason to want to.

Steps 3, 4 and 5 are independent of each other and land in ascending order of what can go wrong.
None of them stores anything, broadcasts anything or changes what markdown means. **But the first
draft's claim that "the worst outcome of getting one wrong is a page that looks wrong until the
next deploy" is false for step 4** and a reviewer was right to say so: D-20 puts KaTeX on the SSR
path deliberately, there is no `export const ssr = false` anywhere under `web/src`, and an
uncaught throw inside a Svelte component during SSR is a 500 for the whole route — including the
route that is the edit surface. Step 4 therefore sits behind step 3 and in front of step 5 in
ascending order of blast radius, not descending order of difficulty, and its try/catch is a
requirement rather than a nicety.

Step 5 is last of the three because it is the one that needs a security posture, a CSP
verification in a real browser, and a dependency with an active advisory stream.

Step 6 is independent of 1–5 and could be done in parallel, but it is placed after them because it
is the first step that changes what markdown *means*, the first that can make a page permanently
unexportable, and — **contrary to the first draft** — the first that touches the editor mirror
that deletes. It is a five-surface change, not a four-crate one.

Step 7 is last because it is the only step that adds a kind to that mirror and the only one that
can destroy another page's content.

## The properties to write mutation tests against

Five, and none of them is currently pinned by anything.

**A `codeBlock` carrying a second attribute must be reduced, not refused, and its `language` must
still be compared.** Both directions, in the shape of `tests/export.rs:651` and `:689`: emptying
`CODE_BLOCK_ATTRS` must go red (which is the mutation that proved `TASK_ITEM_ATTRS` needed its
own test), and a fence whose stored `language` differs from what its own markdown re-imports as
must still be refused.

**A `Link` mark carrying a null-valued allow-listed attribute must compare equal to one carrying
no such key.** Removing the `!value.is_null()` clause from `reduce()` must go red. This is the
guard against D-21b, and it is the one the existing allow-list is blind to.

**A `doc` attribute must survive a round trip through the Y.Doc.** In `extensions.test.ts`,
beside the existing `keeps every attribute the server writes` case at `:245`: removing `doc` from
`Anchor` must go red. This is the guard against D-21a, and it is a TypeScript test rather than a
Rust one because the deletion happens in the editor.

**A reference's scheme must round-trip through `render_file`, never through `render` alone.** The
refusal for a doc-resolved link happens in the *comparison*, not in the renderer, so a unit test
of the renderer passes while `render_file` fails. Every test for step 6 goes through
`export::render_file`. And the mutation entries go into `scripts/mutate.sh` the way the placement
syntax did — a mutation that breaks one half of the syntax at a time is what proves the two
directions are one rule.

**A reference to a page the caller may not read must render identically to one whose target does
not exist.** The mutation: replace the resolver's per-target `document_access_id_with_baseline`
with `document_by_path_unchecked`, exactly as `scripts/mutate.sh:1093-1100` does for attachments.
That mutation must go red, and it is the only thing that proves D-21d is a code path rather than
a paragraph.

## What should be an ADR

- **How a diagram reaches the page** (D-19). The first time this repo renders generated markup,
  and ADR 0014's switch-back criteria explicitly anticipate the question. It must record the
  two-barrier structure honestly — that the CSP is what holds while mermaid measures text in
  `document.body`, and the `<img>` is what holds afterwards — because an ADR that claims the
  markup never enters the DOM would enshrine a guarantee the code does not provide. It must
  record that `style-src` carries `'unsafe-inline'` in development (`csp.ts:41-46`), so the
  posture is weaker under `npm run dev` than in production. And it must record that
  `securityLevel: 'sandbox'` — the workaround every Mermaid advisory recommends — is unavailable
  here and why, so the next person does not lose an afternoon to it.
- **What a formula may do** (D-20). Because `trust: true` would falsify a sentence ADR 0007
  relies on, and the reasoning has to be written where an amendment would find it. It states the
  budget asymmetry: the server-side renderer is capped harder than the client-side one.
- **How a document reference is written in markdown** (D-21). Exactly parallel to ADR 0015, for
  exactly ADR 0015's reasons, including the restore-safety cost and its switch-back criterion —
  **but not copying 0015's opening paragraph**, which contains the refusal error. It must also
  carry D-21a's mirror finding, because that is the reusable lesson: a mark's attributes are not
  null-filtered by y-tiptap and a node's are, so the `TaskItem::id` pattern does not transfer to
  a mark without the `reduce()` change.
- **What a reference discloses** (D-21d). Small, and it ships with step 6 rather than waiting for
  transclusion — it is the rule the first draft assumed already existed.
- **What a transclusion discloses** — only if step 7 happens. Parallel to ADRs 0009 and 0011, and
  it names what a v7 uuid discloses on its own.
- **Adding block kinds by hand rather than by registry.** Arguably system-wide rather than local
  to this piece, since M9 and M10 both add blocks. At minimum the roadmap's claim that the
  registry exists has to be corrected.
- **A dated correction note on ADR 0015**, for the refusal sentence. Not an edit to the decision.

No ADR 0007 amendment is expected, because no CSP directive moves — **with one caveat that is
now a constraint rather than a hope**: see the highlighter note below. If a directive turns out
to be needed, that is the signal to reconsider the feature rather than the policy.

## Out of scope, and why

**Freeform canvas.** The roadmap lists it under M4, but it is a drawing surface with its own
persistence model and no markdown spelling at all — it cannot be a code block, and it is a piece
of its own rather than a block in this one.

**Inline maths (`$x$` mid-sentence).** This is the one feature here that genuinely cannot be had
cheaply, and the reason is worth writing down so nobody tries it as a follow-up. pulldown-cmark
0.12.2 does ship `Options::ENABLE_MATH`, but turning it on is a **global re-parse of every
stored paragraph**: it makes `$` a special byte for the whole corpus. And `escape_inline` does
not escape `$` (`export.rs:1355-1387` — it escapes `\` `` ` `` `*` `[` `]` `_` `<` `&` `~` and
nothing else). So a stored paragraph containing a matched `$…$` pair — a German medical page
reading "$120 bis $150" — exports verbatim today, would re-import as a different tree with the
delimiters gone, and would be refused. A page nobody edited becoming unexportable is the
`LINK_ATTRS` disaster in a new place. It also needs a new `MarkKind`, an entry in `MARK_ORDER`, a
verbatim rendering path that bypasses `escape_inline` entirely, and a named refusal for a formula
whose content contains a literal `$` (which has no spelling at all).

The mitigation exists and is backward-compatible in both directions — `\$` suppresses maths
under `ENABLE_MATH` and already re-imports as a plain `$` today — but it must ship in the *same*
change as the option, and it must be preceded by a full export run against a copy of the
production database. `content-example/` holds no `$` at all, so the test suite cannot fail on
this; the live database is where the risk sits and this plan could not inspect it. Display maths
as a ` ```math ` fence needs none of it.

**External embeds** (YouTube, iframes of other sites). `frame-src ['none']`, and the ADR 0007
reasoning behind it, makes this a policy change rather than a feature.

**Citations and bibliography** (M10, which depends on M4), **charts** (M9), **dataset views**
(M8).

**Mermaid `click` interactions.** `bindFunctions` is what wires them, and not calling it is the
belt to `securityLevel: 'strict'`'s braces. This moved from "out of scope" to a stated D-19
decision, but it stays listed here so nobody adds it as a small convenience.

**Diagram editing inside the editor** — a live preview, a node view, a diagram picker. If one is
ever wanted, it is a TipTap `NodeView`, which changes rendering only and declares no attributes,
so the schema stays byte-identical. Anything that writes back into the block — a computed size, a
cached SVG, an error flag — is the D-18 trap, and with `CODE_BLOCK_ATTRS` in place the symptom
changes from a refused page to a silently incomplete backup, which is harder to notice.

**Server-side diagram rendering and caching.** Both would be improvements; both need somewhere to
put the result, and there is nowhere that is not `Block::attrs`.

## Where the research disagreed, and what is still unverified

**How many mirrors there are.** `block.rs`'s doc comment says four plus a softer fifth. One
researcher counted ten; another counted eleven distinct edits and named a sixth mirror
(`ATTRIBUTE_LABEL` at `web/src/lib/history.ts:167`, which the attachment commit edited in the
same hunk as `BLOCK_LABEL` at `:139` and which the doc comment does not mention) and a seventh
(the editor surface's own `:global(.gw-ed-surface …)` CSS, which has to mirror `BlockView`'s
scoped styles). They agree the doc comment understates it; they disagree on the count. The honest
answer is that the number depends on whether the kind has attributes and whether a human can
create it — `Attachment` needed a toolbar control, a prop threaded through three call sites and a
helper module; `TaskList` needed none of that because TipTap's `[ ] ` input rule creates it. This
matters for step 7. **The doc comment should be corrected in whichever step first adds a kind**,
and its `Anchor`-shaped mark hazard should be written into it in step 6 regardless, because step
6 is the proof that a *mark* attribute has its own version of the same rule.

**Whether an export refusal fails the run.** The briefing said it does; two researchers
independently found it does not, and it was verified here. Documented above, with all sixteen
sites, because the correction makes the consequence worse rather than better.

**What "document references" means.** Genuinely unresolved, and it is D-22. One researcher read
it as finishing the `doc` mark; another read it as transclusion and mapped the permission rule
for it. This plan builds the first and specifies the second. **The owner has to say which they
meant**, and the answer changes the size of this piece by a large factor.

**Nothing about Mermaid's or KaTeX's runtime behaviour under this CSP has been measured.**
Neither library is installed here — `web/package.json` was checked — so every claim about what
the policy admits is inference from the policy, not observation. `web/vite.config.ts:99-104`
already records that this class of breakage is invisible to the server, and `csp.ts`'s own doc
comment records that both CSP facts in this repo were found by loading the site rather than by
reading the spec. **Verify against a production build and a real browser, not `npm run dev`** —
SvelteKit loosens `style-src` in development, so a production-only failure is exactly the shape
to expect. The same uncertainty applies to the claim that `mermaid.render()` appends to
`document.body` to measure text: that is a reviewer's report of the library's documented
behaviour and it has not been observed here. D-19 is written to be correct either way — if it
turns out mermaid never touches the live document, the CSP argument is simply unnecessary rather
than wrong.

**The advisory record for Mermaid is one researcher's report and it has not been verified.** They
cited a 2026 run of CVEs — diagram text escaping the SVG under the default `securityLevel:
'strict'`, CSS injection via `%%{init}%%` reaching page scope and then sibling scope, prototype
pollution, and several parser infinite loops — with specific identifiers and fixed-in versions.
There is no way to check those identifiers from here and it was not attempted. **Re-check them
against the registry at implementation time**, and expect the version to pin to have moved: the
pattern described is an active series rather than a settled one, where each fix was followed by a
related bypass. The same caution applies to the KaTeX version figure, which that researcher
themselves flagged as coming from a search result rather than the registry.

**The availability class is the one D-19 does not close**, and it is the reason the caps are
decisions. A parser that loops forever hangs the reader's tab, and mermaid cannot be moved into a
Web Worker because it needs the DOM. So: refuse to render a fence over a few kilobytes and show
it as ordinary code instead, set `maxTextSize` and `maxEdges` low, and render lazily so a page
with twenty diagrams does not block first paint.

**The highlighter's engine is a CSP constraint, not a library preference.** A tokens-returning
highlighter is what step 3 asks for, and Shiki is the obvious choice — but Shiki's default regex
engine is WebAssembly, and instantiating WASM requires `'wasm-unsafe-eval'` in `script-src`,
which this plan's own gate says would be *"the signal to reconsider the feature rather than the
policy"*. The failure shape is the production-only one `vite.config.ts:99-104` already warns
about: green in tests and in `npm run dev`, every code block unstyled in production with a
console violation and nothing server-side to notice. **So: a pure-JavaScript tokeniser, or
Shiki's JavaScript regex engine explicitly configured**, with the language set bundled and
enumerated rather than dynamically imported. This was a deferred choice in the first draft and it
is a constraint here.

## Objections, and where each one landed

Nothing raised in review is dropped. Fourteen were folded in; three sub-claims were wrong and are
answered by name so they are not re-raised.

| Objection | Disposition |
|---|---|
| D-19's "markup never enters the DOM" contradicts mermaid needing `document.body` | **Folded.** D-19 rewritten as two barriers; the CSP named as the one that holds during `render()`; the dev-mode `style-src` gap stated; `securityLevel`, `bindFunctions`, the `secure` list and the owned container promoted from advice to decisions. |
| BlockView's `doc` branch is evaluated before `safeHref` | **Folded.** D-21c: the `doc` value never becomes an `href`; resolution yields a path and the path goes through `safeHref`; unresolved renders as text. |
| D-21 ships with no authorisation rule | **Folded.** D-21d writes it, and names the unfiltered batch query and the unfiltered picker as the rejected shapes. |
| Per-reference authorisation fan-out is an availability lever | **Folded.** D-21e: the resolver lives in `gw-store` so the baseline hoists once, and caps per page. `max_connections(1)` and the four `pub(crate)` hoists cited. |
| KaTeX's DoS budget is spent on the wrong feature | **Folded.** D-20: per-fence input cap, per-page fence count cap, finite `maxSize` and `maxExpand`, and the stated asymmetry that the server renderer is capped harder. |
| D-18's negative requirement binds the team, not a writer | **Folded.** D-18 gains `CODE_BLOCK_ATTRS` as step 2, plus the reason the allow-list still does not license a second *declared* attribute. |
| The `dok:` writer must refuse what it cannot round-trip | **Folded.** D-21f: validate the destination is a uuid, modelled on `attachment_destination`; `wrap`'s unescaped `format!` cited. |
| The HTML-sink check is too narrow and misses `mermaid.render(id, text, el)` | **Folded.** Sink list widened to ten spellings plus the three-argument mermaid call; the `style:` value constrained to theme-derived colours. |
| "No CSP directive moves" is likely false for Shiki's WASM engine | **Folded.** Now a constraint: pure-JS tokeniser or Shiki's JS regex engine, languages enumerated, `attrs.language` a map key only. |
| A v7 uuid discloses a creation timestamp | **Folded.** D-22's ADR requirement now says the identifier's own disclosure must be named. |
| D-21 destroys its own references in the CRDT and broadcasts it | **Folded.** D-21a, traced through `y-tiptap.js:1500-1525` and `:1039-1044`; step 6's row now names `web` and the editor mirror. |
| The obvious fix re-runs the `LINK_ATTRS` disaster | **Folded.** D-21b: declare `doc` **and** make `reduce()` drop null-valued allow-listed attrs; both pinned by tests and mutations. |
| Whitespace becomes load-bearing and every diff mode is whitespace-blind | **Folded, with one correction.** Step 1 now fixes `diff_structure`'s fingerprint via a new `Block::diff_text()`. The objection's claim that *"there is no revision-restore endpoint"* is **wrong**: `POST /api/revisions/{id}/restore` (`revisions.rs:212`), `GET /api/revisions/{id}/source` (`:211`) and the history UI (`history/+page.svelte:98-114`) all exist. The loss is the ability to *find* the bad revision, not to undo it. |
| "Check production first" applied to `$` and not to `dok:` | **Folded.** D-21g: step 6 opens with one `SELECT` for existing `href` values beginning `dok:`, with the migration in the same change. |
| Step 5 touches the destructive mirror; the plan said step 6 was the only one | **Folded.** Same as D-21a; the ordering rationale is rewritten and the false sentence is gone. |
| Step 3's "worst outcome is a page that looks wrong" is false for KaTeX | **Folded.** D-20 gains the try/catch; the ordering rationale now says why it is false and reorders on blast radius. |
| The gate does not run the Docker bundle assertion; the highlighter needs `noExternal` too | **Folded.** Verified (`package.json:8`, `justfile:45-46`, `:270-279`, `Dockerfile:68-84`); steps 3 and 4 end on the Dockerfile's own pipeline, and both libraries join `ssr.noExternal`. |
| The refusal correction is scoped to one comment when the error is in many | **Folded, and the count corrected.** Sixteen sites, all listed; ADR 0015 gets a dated correction note rather than an edit, and the new ADR does not copy its opening paragraph. |
| The sink check precedes the file it exempts, and is too narrow | **Folded.** The exemption list is named, starts empty in step 1, and gains exactly one entry in step 4 in the same commit as the file. |

## The gate

```
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd web && npm run check && npx vitest run && npm run build
cargo run -p gw-api -- seed --content content-example
```

In a worktree, `just agent-ci`, not `just ci`. `npm run build` is not optional here: it is the
only gate step that exercises the production CSP, and it is where three of this piece's seven
steps can fail without anything else noticing.

**And for steps 3 and 4, one more line**, because `just agent-ci` does *not* run the Docker
bundle assertion — the check lives only in `docker/gw-web.Dockerfile:68-84` and only during
`docker build`:

```
cd web && npm run build && \
  find build -name '*.js' -not -path 'build/client/*' \
    -exec grep -hE "^(import|export)[^']*from '[^']+'" {} + \
  | grep -oE "from '[^']+'" | cut -d\' -f2 | grep -vE '^(node:|\.)' | sort -u
```

Empty output is the pass condition. Anything printed is a package the runtime image will not
contain.

**And for step 5, a browser.** The CSP claims in D-19 are inference. Load a page holding a
diagram from a production build — not `npm run dev` — and read the console.

A changelog fragment under `changelog.d/` ships in the commit that earns it — no exceptions and
no waiver — and the reasoning goes to Omnigraph as well, per
[CLAUDE.md](../../../CLAUDE.md). Note that `omnigraph`, `graphify` and `playwright` all failed to
connect in the sessions this plan was written and revised in, so that write is outstanding and
the browser verification above cannot be automated from here yet.

`content-example/README.md:31` currently reads *"Diagramme und Formeln. Noch nicht gebaut."*
Steps 3 through 5 are what make that sentence false, and the seed corpus should gain a diagram, a
formula and a highlighted listing in the same change — every `.md` file under `content-example/`
is walked by an export round-trip test, so a seeded example is also a round-trip proof. Step 6
should seed a `dok:` reference for the same reason, *after* the production check in D-21g.
## Owner's answers to this plan's open questions, 2026-09-02

The plan shipped with six open questions. Four are now answered; the reasoning is recorded
because each was a real choice with a rejected alternative.

### D-23: Shiki, its JavaScript regex engine, and a curated grammar set

Editor-grade accuracy, and the languages are a deliberate list — shell, YAML, JSON, SQL,
Rust, TypeScript, Python, Markdown — rather than everything Shiki ships.

Rejected: highlight.js (smaller and needs no curation, but rougher on exactly the shell and
YAML that this corpus is mostly made of) and Shiki's full grammar set (best on anything ever
pasted, and by far the largest download for a wiki whose pages are overwhelmingly prose).

**The engine is not a preference.** Shiki defaults to Oniguruma compiled to WebAssembly, and
this application's Content-Security-Policy has no `'wasm-unsafe-eval'`. The JavaScript regex
engine must be configured explicitly, and a test must assert it — a silent fall back to the
WASM default would work in development, pass every test that does not render in a browser,
and fail only behind the CSP in production.

**Adding a language is a deliberate act**, and the set should live in one named place with
that sentence beside it, so the ninth grammar is added on purpose rather than by reflex.

### D-24: A diagram is rendered twice, once for each theme

Mermaid runs during server rendering and its output is a fixed image; the site has a
light/dark control, and one image can only match one of them.

So both are produced and the stylesheet shows whichever matches. Rejected: one neutral look
(simplest, and it would read as deliberately plain rather than wrong — but "acceptable on
both grounds" is a compromise nobody asked for on a page they are trying to read), and
rendering in the browser (diagrams would follow the theme live, and Mermaid would then run on
every reader's machine over page text, which is precisely the attack surface D-19 exists to
close).

The cost is stated rather than hidden: every diagram is rendered twice at publish time and
carried twice in the markup. That is paid once per page render, by the server, for a wiki of
tens of pages — and it buys a diagram that is never wrong against its own background.

### D-25: An unknown language is shown plain, and named quietly

A fence whose language the highlighter does not know renders exactly as it does now —
correct, monospaced, uncoloured — with the unrecognised language shown discreetly on the
block.

Rejected: silence. An author who writes ```` ```kotlin ```` and sees no colour has no way to
tell whether the wiki does not know Kotlin, whether they misspelled it, or whether
highlighting is broken. Naming it answers all three at once, and costs a label only on
fences that already have something to explain.

A fence with **no** language is not an unknown language and gets no label — the author said
nothing and the page should not argue with them.

### Still open

Two of the six remain, and both need measurement rather than a decision:

- The exact cap numbers (D-22's "generous" is settled as a direction; the figures should come
  from what the corpus actually contains, with headroom).
- Whether the live database holds any link mark whose `href` begins `dok:`. The corpora are
  clean, but production could not be inspected from the session that wrote this.
