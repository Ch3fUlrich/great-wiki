import { browser } from '$app/environment';
import {
  diagramDataUri,
  diagramEdgeRefusal,
  diagramSize,
  mermaidConfig,
  type Diagram,
  type DiagramSize
} from './diagram';

/**
 * Drawing a ` ```mermaid ` fence — in the reader's browser, twice, into two `<img>` addresses.
 *
 * # Why the library is behind a `browser` guard and not simply imported
 *
 * The guard is not defensive: it is what keeps mermaid out of the **server** bundle.
 * `$app/environment`'s `browser` is replaced with a literal at build time, so in the SSR build
 * [loadMermaid] reads `false ? import(…) : …` and rollup drops the import entirely — no
 * server chunk is emitted and the server bundle names no package. A bare `import()` inside a
 * branch that never executes on the server does **not** achieve that: the chunk is still
 * emitted, and the production web image ships no `node_modules` for it to resolve against
 * (`docker/gw-web.Dockerfile`, `web/scripts/check-server-bundle.sh`). This is the same shape,
 * and the same reasoning, as `loadEditor` in `web/src/routes/[...path]/+page.svelte`, which
 * documents having learnt it the expensive way.
 *
 * It is also why a page with no diagram on it downloads none of this. Mermaid is by a wide
 * margin the largest thing in this application; the `import()` is reached from a component's
 * effect, so it is fetched when a diagram is actually on screen and never before.
 *
 * # What is in `document.body` while a diagram is drawn, honestly
 *
 * The two-argument `mermaid.render(id, text)` returns the SVG as a **string** and never writes
 * it into the page — but it needs the DOM to measure text, so while it works, the diagram
 * (its labels included) is inside `document.body`. Verified in the installed package rather
 * than taken from the documentation: `render` does `let root = select(document.body)`, appends
 * a temporary `<div id="d…"><svg id="…">`, and — this is the part that matters — inserts a
 * `<style>` element built from the theme into that SVG while it is still there.
 *
 * So there are two barriers and they act in this order.
 *
 * 1. **While it renders, the Content-Security-Policy.** `script-src ['self']` with no
 *    `'unsafe-inline'` and no `'unsafe-eval'` refuses an inline handler that escaped mermaid's
 *    own sanitiser; `style-src ['self']` refuses an injected `<style>` ELEMENT, which is the
 *    CSS-injection class; `img-src ['self', 'data:']` refuses a remote beacon; `object-src`,
 *    `frame-src` and `base-uri` are `['none']`. **That barrier is weaker under `npm run dev`**
 *    — SvelteKit adds `'unsafe-inline'` to `style-src` there so it can inject component styles
 *    (`$lib/csp`) — which is why a diagram is verified against a production build.
 * 2. **Afterwards, the `<img>`.** The returned string is percent-encoded into a
 *    `data:image/svg+xml` URI and set as an `src`. An `<img>` executes no script and reaches
 *    no DOM, which is the containment ADR 0014 already requires for an uploaded SVG. The
 *    address is put through the browser's own image decoder before it is put on the page
 *    ([zeichenbar]), because that containment is worth nothing if what arrives is a
 *    broken-image icon — see that function for the case that made this necessary.
 *
 * **Barrier one visibly fires in production, and that is the barrier working rather than a
 * fault.** Verified against a production build in a real browser: every diagram logs
 * *"Refused to apply inline style … style-src 'self' 'nonce-…'"*, because the `<style>` mermaid
 * inserts while measuring is exactly the injected style element `style-src` exists to refuse.
 * The drawing is unaffected — that `<style>` is serialised into the returned string and is the
 * image's own business once it is inside the `<img>`, where the page's policy does not reach.
 * What the refusal costs is that mermaid measures text with the page's own font rather than the
 * one it is about to draw with; both are proportional sans faces at the same size, the node
 * padding absorbs the difference, and the checked-in example renders correctly. **The fix for
 * that console line is not `'unsafe-inline'`** — beyond being the one loosening ADR 0007
 * refused, it would make `widenCspNonceToStyles` skip the directive and silently strip the
 * nonce TipTap depends on, unstyling the editor in production only.
 *
 * The library's own settings are the third thing, and they live in `$lib/blocks/diagram` with
 * a test on each: `securityLevel` strict, an explicit `secure` list, and `bindFunctions` never
 * called — not calling it is the belt to strict's braces, and there is nothing in an `<img>`
 * for a diagram's `click` handler to be bound to anyway.
 *
 * # Twice, once per theme (D-24)
 *
 * An `<img>` cannot inherit `prefers-color-scheme`, and this wiki has a light/dark control, so
 * one fixed image can only ever match one of them. Both are drawn and `DiagramView.svelte`'s
 * stylesheet shows whichever applies. **The cost, stated rather than hidden: every diagram is
 * drawn twice and carried twice in the markup.** It is paid in the reader's own tab, once per
 * page, for a wiki of tens of pages — and it buys a diagram that is never wrong against its own
 * background.
 */

/**
 * The library, in the browser only. See this module's header for why the ternary is load
 * bearing and an `{#if}` around a bare `import()` is not.
 *
 * The server-side branch returns a promise that never settles, exactly as `loadEditor` does.
 * It is unreachable — nothing calls [drawDiagram] outside a component effect, and effects do
 * not run during server rendering — and a rejection would be worse than a hang, because it
 * would have to be rendered into the SSR response for a reader who never asked for a diagram.
 */
const loadMermaid = () =>
  browser ? import('mermaid') : new Promise<typeof import('mermaid')>(() => {});

/**
 * Is this address one the browser can actually turn into a picture?
 *
 * **The guarantee `diagramDataUri` cannot make**, and the reason "malformed source is never
 * a broken image" was false until it existed. That function is pure and sees a string;
 * whether a `data:image/svg+xml` decodes is a question only an XML parser can answer, and
 * the answer is regularly no: mermaid serialises the finished SVG through the DOM's own HTML
 * serialiser, so any HTML-only spelling in it — a `<br>` with no closing tag, a
 * `&nbsp;` for a non-breaking space somebody pasted — is well-formed HTML and not well-formed
 * XML, while an `<img src="data:image/svg+xml,…">` is parsed as strict XML. Setting
 * `htmlLabels: false` (see `$lib/blocks/diagram`) removes the case that actually occurs;
 * this is what makes the promise true of the ones nobody has thought of.
 *
 * The browser's own decoder is the oracle rather than a parser of ours: same bytes, same
 * code path, same answer as the `<img>` on the page is about to give. A rejection means the
 * fence shows its own source with a German line, which is what the feature promises for
 * every other failure.
 *
 * A `DOMParser` would be the obvious alternative and is deliberately not used: its
 * parse-a-string method is one of the spellings `scripts/check-html-sinks.sh` refuses — on the
 * grounds that an inert document's nodes become live the moment they are adopted into the real
 * one — and that check's exemption list is empty and stays empty. (Named rather than written,
 * because the check greps this directory and would find the word in this very sentence.)
 */
async function zeichenbar(src: string): Promise<boolean> {
  const bild = new Image();
  bild.src = src;
  // `decode` is what makes this an answer rather than a guess. Where it does not exist the
  // picture is shown as before — a broken image is bad, and refusing every diagram in a
  // browser too old to have had `decode()` since 2018 would be worse.
  if (typeof bild.decode !== 'function') return true;
  try {
    await bild.decode();
    return true;
  } catch {
    return false;
  }
}

/** Said when the library itself could not be fetched — an offline reader, or a failed deploy. */
const NICHT_GELADEN = 'Der Diagrammzeichner konnte nicht geladen werden.';

/**
 * Said when mermaid was asked and would not draw it.
 *
 * Deliberately says nothing about what went wrong. Mermaid's own parse errors are English,
 * are about its grammar rather than about this page, and arrive as an object whose shape it
 * does not document; the source is shown above this line, which is the actionable part.
 */
const NICHT_GEZEICHNET = 'Dieses Diagramm konnte nicht gezeichnet werden.';

/**
 * Ids for mermaid's temporary elements. A counter rather than a random value: it is used as a
 * `#id` selector by the library, it must not collide with anything on the page, and a
 * deterministic one is greppable when something does go wrong.
 */
let laufend = 0;

/**
 * One diagram at a time, page-wide.
 *
 * Mermaid's configuration is global — `initialize` sets a site config the next `render` reads —
 * so two renders in flight would be one render reading the other's theme. Serialising is also
 * what keeps a page of twenty diagrams from being one long block of script: each `await` here
 * yields to the event loop, so the browser paints between diagrams instead of after all of
 * them.
 */
let schlange: Promise<unknown> = Promise.resolve();

/**
 * Draw one diagram, or say why not. Never throws, and never leaves a rejected promise behind.
 *
 * The caller is a component effect, so the answer arrives after hydration; until it does, the
 * reader is looking at the fence's own source, which is what a reader with no JavaScript keeps
 * looking at.
 */
export function drawDiagram(source: string): Promise<Diagram> {
  const lauf = schlange.then(
    () => draw(source),
    () => draw(source)
  );
  // The queue must survive one diagram failing, and `draw` resolves rather than rejects — this
  // is belt and braces so that a bug in it cannot stop every later diagram on the page.
  schlange = lauf.catch(() => undefined);
  return lauf;
}

async function draw(source: string): Promise<Diagram> {
  let mermaid;
  try {
    mermaid = (await loadMermaid()).default;
  } catch {
    return { kind: 'source', note: NICHT_GELADEN };
  }

  // Light first: it is the one the majority of readers will be shown, and if the second render
  // fails there is nothing useful to do with a half-themed pair anyway.
  const hell = await once(mermaid, source, 'default');
  if ('note' in hell) return { kind: 'source', note: hell.note };
  const dunkel = await once(mermaid, source, 'dark');
  if ('note' in dunkel) return { kind: 'source', note: dunkel.note };
  // One size for both. The two renders differ in colour and not in layout, and two `<img>`
  // boxes of different sizes would move the page as the theme changed.
  return { kind: 'drawn', hell: hell.src, dunkel: dunkel.src, groesse: hell.groesse };
}

/** One theme's drawing: the address to load it from, and how big it says it is. */
interface Zeichnung {
  src: string;
  groesse: DiagramSize | null;
}

/** …or the German line saying why there is no drawing. */
type Versuch = Zeichnung | { note: string };

/**
 * One render, in one theme, cleaned up whatever happens.
 *
 * `initialize` before every call rather than once at load: it is what carries the theme, and it
 * rebuilds the site configuration from mermaid's defaults each time, so nothing a previous
 * diagram's `%%{init}%%` directive did can survive into this one. (Mermaid resets its
 * directives per render as well — verified — so this is the second of two.)
 *
 * The `finally` is the part that is not obvious. With `suppressErrorRendering` set, mermaid
 * removes its own temporary elements before throwing — but "the measuring container is ours,
 * appended and removed in a `finally`" is not available to us, because owning the container
 * means the three-argument form and that form performs the DOM write ADR 0014 forbids. So the
 * append is mermaid's and the removal is also ours: the ids are ours, and anything still
 * carrying one when this returns is swept out of the live document.
 */
async function once(
  mermaid: (typeof import('mermaid'))['default'],
  source: string,
  theme: 'default' | 'dark'
): Promise<Versuch> {
  laufend += 1;
  const id = `gw-diagramm-${laufend}`;
  try {
    mermaid.initialize(mermaidConfig(theme));
    const { svg } = await mermaid.render(id, source);
    const src = diagramDataUri(svg);
    if (src === null) return { note: NICHT_GEZEICHNET };
    // Decoded here rather than trusted, so that a picture the browser cannot read never
    // reaches the page as a broken-image glyph. See [zeichenbar].
    if (!(await zeichenbar(src))) return { note: NICHT_GEZEICHNET };
    return { src, groesse: diagramSize(svg) };
  } catch (fehler) {
    // The edge cap is mermaid's to enforce and its message is about mermaid's configuration,
    // so it is the one failure worth translating; everything else gets the general sentence,
    // because the library's own text is English, is about TeX-like grammar rather than about
    // this page, and says nothing an author could act on.
    return { note: diagramEdgeRefusal(fehler) ?? NICHT_GEZEICHNET };
  } finally {
    // `d…` is the enclosing div mermaid appends to `document.body`, `i…` the iframe it would
    // use under a security level this application never sets; both are named after the id.
    for (const uebrig of [id, `d${id}`, `i${id}`]) document.getElementById(uebrig)?.remove();
  }
}
