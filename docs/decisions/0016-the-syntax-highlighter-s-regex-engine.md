# 0016 — The syntax highlighter's regex engine

**Status:** Accepted (2026-09-02)

## Context

D-23 of [the rich-blocks plan](../superpowers/plans/2026-09-02-rich-blocks.md) chose **Shiki**
to colour fenced code blocks, for editor-grade accuracy on the shell and YAML this corpus is
mostly made of, with a curated grammar set rather than everything Shiki ships.

Shiki tokenises TextMate grammars with **Oniguruma compiled to WebAssembly**, and that is its
default — you get it by not typing anything. Instantiating WebAssembly requires
`'wasm-unsafe-eval'` in `script-src`. This application's policy is `script-src 'self'` and
nothing else (`web/vite.config.ts`, [ADR 0007](0007-content-security-policy.md)), and the
plan's own rule for this piece is that if a directive turns out to be needed, *that is the
signal to reconsider the feature rather than the policy*.

What makes this worth a decision record rather than a line of configuration is the **shape of
the failure**. WebAssembly is available in Node, in `vitest`, and in the browser under
`npm run dev` — SvelteKit issues no policy there. So a highlighter on the default engine would
work on the developer's machine, pass `cargo test`, `npx vitest run`, `npm run check` and
`npm run build`, and then leave **every code block on the site unstyled in production only**,
with a console violation nothing server-side can see. `web/vite.config.ts:99-104` already
records that this class of breakage is invisible to the server, and `web/src/lib/csp.ts`
records that both CSP facts in this repository were found by loading the site rather than by
reading the spec.

## Decision

### The JavaScript regex engine is passed explicitly, and a test fails if it ever is not

`web/src/lib/server/highlight.ts` builds the highlighter with
`createHighlighterCoreSync({ …, engine: createJavaScriptRegexEngine() })`, importing that
engine from `shiki/engine/javascript`. Shiki's root bundle — whose `codeToHtml` and friends
default to the WASM engine — is never imported.

Two tests hold it, and they fail for different reasons:

- `highlight.test.ts` makes `globalThis.WebAssembly` **throw on any access**, re-imports the
  module underneath that, and asserts a fence still comes back with colour on it. That is the
  production constraint reproduced in a test rather than described in a comment.
- A second test reads the module's own import specifiers and refuses any that match
  `wasm|oniguruma`, and refuses the bare `shiki` entry point. The engine is an *option*, so
  losing it is a one-line regression; this is what names it.

### The grammar set lives in one place, and adding a language is a deliberate act

`GRAMMARS` in `highlight.ts` is that place, with the sentence beside it. Eight grammars —
JSON, Markdown, Python, Rust, shell, SQL, TypeScript, YAML — and their own declared aliases,
read off each grammar rather than typed out a second time.

Rejected: Shiki's full grammar set (best on anything ever pasted, and by far the largest
download for a wiki whose pages are overwhelmingly prose) and highlight.js (smaller, needs no
curation, and rougher on exactly the shell and YAML this corpus is made of).

### Colour reaches the page as tokens, never as markup

`highlightFence` returns runs of **text** with two colours each. `CodeView.svelte` puts each
run in a `<span>` and binds the colours through Svelte's `style:` directive, which
server-renders as a `style="…"` attribute — already paid for by `style-src-attr
'unsafe-inline'`, and already the mechanism `BlockView.svelte` uses for table alignment. So
**no CSP directive moves**, `scripts/check-html-sinks.sh` keeps its empty exemption list, and
the sentence that there is no sanitisation step in the reader stays true.

Two rules follow, and the second is the one an implementer would break first:

- **A colour is validated as a hex literal on the way out of the theme.** ADR 0007 justifies
  `style-src-attr 'unsafe-inline'` partly on *"the renderer does not emit authored CSS into
  one either way"*. A theme is ours and a fence's text is not; the check costs one regular
  expression and makes that sentence true of the code rather than of the intent.
- **`attrs.language` is a key looked up in an allow-list and a string printed as text, and
  nothing else** — never an `import()` specifier, never a class name, never a `style:` value.
  The importer keeps the info string's first token whatever it is
  (`crates/gw-core/src/markdown.rs`), and over the collab socket a block attribute is
  arbitrary; nothing between there and `documents.body` validates it.

### Both themes ride along on every token

A colour is bound as a custom property (`--token-hell`, `--token-dunkel`) rather than as
`color:`, because an inline `color` would beat every stylesheet rule and server rendering does
not know which theme the reader is in — they may have chosen one (`[data-theme]`) or be
following their system (`prefers-color-scheme`). `CodeView.svelte`'s two rules are the only
place that decides, which is the same shape `tokens.css` uses for every other colour here.

This is D-24's answer to the same question for diagrams, arrived at more cheaply: a diagram is
an image and has to be rendered twice, whereas a token is text and needs only a second colour.

### Highlighting runs in the page's `load`, and the caps belong to the page

This was first written as a call inside `CodeView.svelte`, with one cap — 20 000 characters
per fence — and the reasoning that `BlockView` server-renders for every reader, so a fence is
on the shared SSR path and must be bounded. Both halves of that were wrong, and only a
measurement shows it.

**A per-fence cap bounds no page.** Nothing counted fences, nothing de-duplicated them, and
nothing bounded the time a page could spend. Measured against a production build
(`node build`, adapter-node, one process): a page holding five 19 994-character ` ```markdown `
fences answered in **51.98 s**, and an unrelated page — normally 0.03 s — requested two
seconds into that render waited **48.85 s** behind it. Two consecutive requests each cost the
full time, because nothing is cached. Any writer, on one page, could hold the whole front end
down by requesting it in a loop.

**And the size of a fence is nearly the wrong thing to measure.** The cost is superlinear in
the length of a LINE: 20 000 characters of TypeScript tokenise in 350 ms as 200-character
lines and in 1 003 ms as a single line; 20 000 characters of Markdown in 6 ms against
11 286 ms. A pasted minified bundle does the second by accident.

So the highlighter moved to `$lib/server/highlight.ts`, `[...path]/+page.server.ts` calls
`highlightDocument(body)` once per load, and the tokens travel in the page data exactly as
`formeln` does — which is the escape this ADR's own switch-back criteria already named, taken
for the availability reason rather than the download one. The caps are now five:

- **`LINE_CHARACTER_LIMIT`, 400** — the one the measurements justify. A fence holding a longer
  line renders as ordinary code with a note naming the line's length.
- **`FENCE_CHARACTER_LIMIT`, 20 000** — unchanged, and no longer doing the work alone.
- **`PAGE_FENCE_LIMIT`, 100** — attempts, not successes, and distinct fences only.
- **`PAGE_TOKEN_LIMIT`, 20 000 runs** — what bounds the *response*: the tokens travel twice,
  once as rendered spans and once as hydration data.
- **`PAGE_BUDGET_MS`, 250** — what bounds the *work*, counted as time actually spent inside
  the tokeniser and checked between fences. The same figure as `PAGE_TYPESET_BUDGET_MS` in
  `$lib/server/maths`, because it is the same thread and the same argument.

The last two are **stops rather than skips**: crossing either closes the page, and every
remaining fence renders plain with a German line. A budget that refuses one fence and tries
the next leaves the total where it was, so the total never rises and everything is tokenised
anyway — the same defect this change fixed in `PAGE_MARKUP_LIMIT`
([ADR 0017](0017-what-a-formula-may-do.md)).

**Every call is still inside a `try`.** An uncaught throw in a page's `load` is a 500 for the
whole route, and that route is also the edit surface — so the page could not be repaired
through the editor either.

**What this leaves, stated rather than left to be rediscovered:** a page written to sit
exactly on the budget costs about 0.9 s of server time, and there is still no cache, so
requesting it in a loop is a heavier request than an ordinary page's 0.03 s. That is a factor
of thirty where it was a factor of seventeen hundred, and it is the same shape as any other
expensive page. A content-keyed cache was declined rather than overlooked: it would keep
arbitrary page text alive in the server process for the benefit of exactly the page abusing
it.
- **A fence whose characters do not survive tokenising renders plain.** Shiki splits on `\n`
  and returns lines, so a CRLF fence comes back one character shorter; step 1 of this piece
  existed because a fence's whitespace *is* its content, and a dependency quietly rewriting it
  is that same bug from a new direction. The reconstruction is checked at run time.

### An unknown language is named; a fence with no language is not

D-25. ` ```kotlin ` renders plain with **Unbekannte Sprache: kotlin** under the block, which
answers at once whether the wiki does not know Kotlin, whether the author misspelled it, or
whether highlighting is broken. A fence that states no language gets nothing — the author said
nothing and the page should not argue with them. ` ```text `, ` ```plain `, ` ```plaintext `
and ` ```txt ` are D-18's escape hatch: known, never coloured, never labelled.

## Consequences

- **`shiki` joins `ssr.noExternal`.** The production web image ships no `node_modules` and
  `docker/gw-web.Dockerfile` refuses a server bundle importing any bare specifier. That check
  ran only during `docker build`, so a missing entry passed every gate command green and
  failed after review; it now lives in `web/scripts/check-server-bundle.sh`, which both the
  Dockerfile and `just build` run.
- **The reader's JavaScript does not grow at all.** The first version of this put Shiki and
  all eight grammars in the client bundle — 609 kB raw, about 94 kB brotli, on every page of a
  wiki that is overwhelmingly prose — because a component renders on the server and again
  while it hydrates, so the tokeniser ran a second time in the reader's own tab to re-derive
  what that reader had already been sent. A fence costing the server a second cost the tab the
  same second, frozen. Tokenising in `load` removes both: `grep -r shiki build/client` finds
  nothing.
- **The page data grows instead, on pages that hold code.** Tokens travel as well as being
  rendered, so a listing is carried roughly twice; `PAGE_TOKEN_LIMIT` is what bounds that at
  about 600 kB in the worst case, and the data is highly repetitive and compresses well. For
  a wiki of prose pages this is the better trade by a wide margin — the download it replaces
  was paid by every page whether it held code or not.
- **The curated set of eight is still curated, for a different reason.** No reader downloads a
  grammar now; the server loads all eight at start-up, and each one is another parser reading
  text somebody with write access typed.
- **Markdown fences lose their bold headings.** Only `color` is carried; a theme's
  `font-weight` and `font-style` are dropped, because every additional declaration is another
  thing entering a `style` attribute and the colour is what carries the meaning.
- **The `<pre>` gains a wrapper element.** The note has to sit outside the horizontal scroll
  region, or it scrolls away from a wide listing. `.prose > * + *` now spaces that wrapper and
  `CodeView.svelte` zeroes the `<pre>`'s own margins, so the rhythm is unchanged.

## Switch-back criteria

Revisit if any of these becomes true:

- **The client download stopped being worth it, and this one is already spent.** The escape
  was to tokenise in the page's server `load` and carry the tokens in the payload; it was
  taken before this record was first committed, and for the availability reason rather than
  the download one (see the decision above). `Editor.svelte` was the stated obstacle and was
  not one: it already receives `formeln` from the page data and now receives `fences` the same
  way, so its reading fallback renders exactly what the reader sees. What is left to watch is
  the other direction — the payload for a code-heavy page, which `PAGE_TOKEN_LIMIT` bounds and
  which nothing else measures.
- **Shiki's JavaScript engine stops handling one of the eight grammars.** It is a
  transformation of Oniguruma patterns rather than an implementation of them, and the
  compatibility is per grammar. The symptom would be a throw at first use of one language,
  which the `try` turns into an uncoloured block with a note rather than a 500 — so it is
  survivable, and `forgiving: true` is the option to reach for before the WASM engine.
- **The policy gains `'wasm-unsafe-eval'` for some other reason.** It should not, and this
  decision would not be the reason: the JavaScript engine is not a workaround being tolerated,
  it is a smaller attack surface than a WebAssembly module in the same origin as the session
  cookie.
