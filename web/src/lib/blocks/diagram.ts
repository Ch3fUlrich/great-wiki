import type { MermaidConfig } from 'mermaid';

/**
 * What both halves of a ` ```mermaid ` fence agree on — and **no mermaid at all**.
 *
 * A diagram is drawn by Mermaid **in the reader's own browser** (`$lib/blocks/mermaid`),
 * into an `<img src="data:image/svg+xml,…">` that `DiagramView.svelte` puts on the page.
 * This module is the only thing the reader and the renderer share, and it is deliberately
 * free of the library: `BlockView` imports [isDiagramFence] from here, `BlockView` renders on
 * the server, and the production web image ships no `node_modules` — so a static
 * `import 'mermaid'` anywhere on that path would be `ERR_MODULE_NOT_FOUND` on the first
 * request in production. The `import type` above is erased at compile time and is not one.
 *
 * See [ADR 0018](../../../../docs/decisions/0018-how-a-diagram-reaches-the-page.md).
 *
 * # Why it is an `<img>` and not markup
 *
 * [ADR 0014](../../../../docs/decisions/0014-what-a-file-has-to-be-to-be-attached.md) settled
 * this for an uploaded SVG and its reasoning is about the MECHANISM rather than about where
 * the bytes came from: an SVG may be shown through `<img>` or a CSS `background-image` —
 * contexts no browser executes script in — and never through `<object>`, `<embed>`,
 * `<iframe>`, or by putting its markup into this wiki's own DOM. Generated SVG is not exempt
 * because we generated it: every byte of it is a function of text somebody with write access
 * to one page typed. `img-src ['self', 'data:']` already admits the address
 * (`web/vite.config.ts`), so no policy directive moves for this feature.
 *
 * # What the caps are for, and why they are looser than the ones on maths
 *
 * A ` ```math ` fence is typeset on the shared server, where `Store::open` holds
 * `max_connections(1)` and a slow page load is a lever on the whole deployment. A diagram is
 * drawn in the reader's own tab, so the same mistake costs one tab. That asymmetry is why
 * [DIAGRAM_CHARACTER_LIMIT] is generous where the formula limits are tight, and why there is
 * no per-page diagram COUNT limit to match `PAGE_FORMULA_LIMIT`: nothing shared is being
 * spent. What remains is the availability class the `<img>` does not close — a parser that
 * loops forever hangs the tab, and mermaid cannot be moved into a Web Worker because it needs
 * the DOM to measure text — so the input is capped before the library is called at all.
 *
 * **The residual, stated so that it is not re-raised as new.** With
 * [DIAGRAM_STATEMENT_LIMIT] in place one diagram costs seconds rather than minutes, but a page
 * carrying twenty diagrams that each sit on the cap is still minutes of intermittently frozen
 * tab, one diagram at a time (`drawDiagram` serialises them and yields between them, so the
 * page is interactive in between and never during). That is knowingly accepted: it costs the
 * reader a tab they can close, and a per-page count would refuse a legitimate page of many
 * small diagrams in order to bound a page nobody writes by accident. A count limit is the
 * thing to reach for if it stops being enough.
 */

/**
 * The longest diagram that is drawn, in characters.
 *
 * Generous on purpose — you would have to try. The diagram in `content-example/` is under 200
 * characters; ten thousand is a flowchart of some hundreds of nodes, written by hand. Past it
 * the fence renders as its own source with a line naming the limit, because an author whose
 * diagram does not appear must be able to tell "too big" from "broken".
 *
 * Also handed to mermaid as `maxTextSize`, so the cap is enforced twice: [diagramRefusal]
 * declines before the library is loaded, and the library declines again if it is ever called
 * with something this module let through.
 */
export const DIAGRAM_CHARACTER_LIMIT = 10_000;

/**
 * How many edges one diagram may have.
 *
 * Mermaid's own default is 500. This is lower because edge count is what the layout
 * algorithms are superlinear in, and a diagram of 200 edges is already past the point where a
 * reader can follow it. Over it, mermaid refuses and the fence shows its source.
 */
export const DIAGRAM_EDGE_LIMIT = 200;

/**
 * How many statements one diagram may hold — **the edge cap for everything that is not a
 * flowchart**.
 *
 * [DIAGRAM_EDGE_LIMIT] is mermaid's own, and mermaid applies it to flowcharts. A
 * `classDiagram`, a `stateDiagram-v2`, a `mindmap`, a `journey` and a stack of nested
 * subgraphs are bounded by nothing but [DIAGRAM_CHARACTER_LIMIT], which is far too generous
 * for them: measured in Chromium under the production policy, 750 `C <|-- D` relations in
 * 9 805 characters — inside every cap this module had — drew in 16.8 seconds, and seven
 * animation frames were served during that window against a 61 fps baseline. That is a
 * frozen tab rather than a slow one, and the same shape costs 9.5 s as a stateDiagram, 7.4 s
 * as a mindmap and 4.5 s as 300 nested subgraphs.
 *
 * So the count is ours and it is syntax-agnostic: every diagram language mermaid understands
 * spells one relation per statement, and a statement is a line or a `;`-separated part of
 * one. It is asked before the library is fetched at all, which also puts the refusal in the
 * first response.
 *
 * The same 200 as the edge cap, because it is the same promise made to a reader
 * ([ADR 0018](../../../../docs/decisions/0018-how-a-diagram-reaches-the-page.md)) and 200
 * relations is already past what anybody can follow in a picture. **What it does not buy is
 * a fast diagram**: 200 class relations still cost seconds in the reader's own tab. That is
 * the asymmetry ADR 0018 accepts — a slow diagram costs one tab and a slow formula costs the
 * deployment — and this cap is what keeps it seconds rather than minutes.
 */
export const DIAGRAM_STATEMENT_LIMIT = 200;

/**
 * How many times wider than tall a drawing may be before it is left at its own size.
 *
 * `max-width: 100%` is right for almost every diagram and catastrophic for a very wide one:
 * the 750-relation class diagram above laid out to a viewBox of 63 604 × 306, which shrinks
 * into a 700-pixel column as roughly 700 × 3 CSS pixels — a grey line where a picture should
 * be. Past this ratio `DiagramView` keeps the drawing's own size and lets it scroll inside
 * the box it is already in, which is legible by dragging rather than illegible in place.
 *
 * Eight because at eight-to-one a drawing shrunk into a 390-pixel phone column is about 49
 * pixels tall — three lines of node text, still readable — and past it stops being a picture.
 */
export const DIAGRAM_ASPECT_LIMIT = 8;

/**
 * The configuration keys a diagram may not set on itself.
 *
 * Mermaid lets a diagram carry its own configuration as a `%%{init: …}%%` directive in the
 * source, and `secure` is the list of keys its `sanitize` deletes from such a directive
 * before applying it. **Passing this list REPLACES mermaid's own**, so the library's defaults
 * are repeated here rather than assumed — leaving one out would silently stop protecting it.
 *
 * The additions, each with what it would otherwise buy the author of a diagram:
 *
 * - `dompurifyConfig` — weakening the sanitiser that is meant to be protecting the page from
 *   this very diagram.
 * - `themeCSS`, `themeVariables`, `fontFamily`, `altFontFamily` — the CSS-injection surface.
 *   It matters here and not only in principle: mermaid inserts a `<style>` element into the
 *   SVG *while the SVG is still inside `document.body`* (see `$lib/blocks/mermaid`), and
 *   under `npm run dev` SvelteKit adds `'unsafe-inline'` to `style-src`, so the barrier that
 *   refuses that element in production is not in force in development.
 * - `htmlLabels` — makes a label markup instead of text.
 * - `theme` — would make both of D-24's two renders come out in the same theme, so the image
 *   shown against a dark background would be the one drawn for a light one.
 */
export const SECURE_CONFIG_KEYS: readonly string[] = [
  // Mermaid's own defaults, verified against the installed package.
  'secure',
  'securityLevel',
  'startOnLoad',
  'maxTextSize',
  'maxEdges',
  'suppressErrorRendering',
  // Ours.
  'dompurifyConfig',
  'themeCSS',
  'themeVariables',
  'htmlLabels',
  'fontFamily',
  'altFontFamily',
  'theme'
];

/**
 * A font stack that resolves inside an `<img>`, which the page's own faces do not.
 *
 * **A stated cost of the `<img>`, not an oversight.** An SVG loaded as an image is rendered in
 * its own isolated context: it cannot see this page's stylesheet, so none of the wiki's
 * vendored faces are available to it and only families the operating system already has will
 * resolve. Generic families are what is left. It is the same stack for both renders and for
 * mermaid's own text measurement, so the labels are laid out against the family they are drawn
 * in.
 */
const DIAGRAM_FONT = 'ui-sans-serif, system-ui, sans-serif';

/** How big the drawing wants to be, in CSS pixels. */
export interface DiagramSize {
  breite: number;
  hoehe: number;
}

/** What the reader should draw for one ` ```mermaid ` fence. */
export type Diagram =
  /**
   * Two data URIs, one per theme (D-24), and the size the drawing asked for.
   *
   * An `<img>` cannot follow `prefers-color-scheme` or the reader's `[data-theme]` choice, and
   * server-side rendering does not know which of the two applies — so both are produced and
   * the stylesheet shows whichever matches, exactly as `tokens.css` does for every other
   * colour in the application.
   *
   * `groesse` is `null` when the drawing did not say — see [diagramSize] for why it has to be
   * read out of the SVG rather than left to the browser.
   */
  | { kind: 'drawn'; hell: string; dunkel: string; groesse: DiagramSize | null }
  /** It was not drawn, and this German line says why. The source is shown above it. */
  | { kind: 'source'; note: string };

/**
 * Is this fence's info string the one that means "draw this"?
 *
 * `mermaid` and nothing else — `mermaid-js`, `graphviz` and `dot` are deliberately not
 * aliases, for [isMathFence]'s reason: each spelling admitted is a spelling to argue about
 * again later, and an author who writes one of them is told this wiki does not know that
 * language rather than being silently given something they did not ask for.
 *
 * `attrs.language` is the info string's first token on the way in
 * (`crates/gw-core/src/markdown.rs`) and an arbitrary JSON value over the collaboration
 * socket — nothing between the editor and `documents.body` validates it — so this takes
 * `unknown` and answers `false` for everything that is not a string.
 */
export function isDiagramFence(language: unknown): boolean {
  return typeof language === 'string' && language.trim().toLowerCase() === 'mermaid';
}

/** German thousands separators, without depending on which locale data Node was built with. */
function zahl(value: number): string {
  return String(value).replace(/\B(?=(\d{3})+(?!\d))/g, '.');
}

/**
 * Why this diagram will not be drawn, or `null` if nothing stands in the way.
 *
 * Asked in the component, so the answer is in the FIRST RESPONSE rather than after hydration:
 * a reader with JavaScript switched off, or one whose browser never gets as far as loading
 * mermaid, still sees the source and the sentence explaining it.
 */
export function diagramRefusal(source: string): string | null {
  if (source.length > DIAGRAM_CHARACTER_LIMIT) {
    return (
      `Nicht gezeichnet: ${zahl(source.length)} Zeichen, höchstens ` +
      `${zahl(DIAGRAM_CHARACTER_LIMIT)} je Diagramm — ein Diagramm wird im Browser gezeichnet, ` +
      'und ein sehr großes hält die Seite an.'
    );
  }
  const anweisungen = diagramStatements(source);
  if (anweisungen > DIAGRAM_STATEMENT_LIMIT) {
    return (
      `Nicht gezeichnet: ${zahl(anweisungen)} Anweisungen, höchstens ` +
      `${zahl(DIAGRAM_STATEMENT_LIMIT)} je Diagramm — ein Diagramm wird im Browser gezeichnet, ` +
      'und ein sehr großes hält die Seite an.'
    );
  }
  return null;
}

/**
 * How many statements a diagram's source holds — see [DIAGRAM_STATEMENT_LIMIT].
 *
 * A line, and a `;`-separated part of a line, because a flowchart may be written either way
 * and the checked-in example is written the second way. Blank lines do not count and `%%`
 * comments do not count, which also excludes a `%%{init: …}%%` directive.
 *
 * **It over-counts a label containing a semicolon**, deliberately: this is a cheap bound on
 * how much work the layout will be asked to do, not a parser, and the only cost of counting
 * one statement too many is that a diagram very near the cap is shown as its own source with
 * a line saying so. Writing a parser for six diagram grammars in order to be exact about a
 * limit nobody should be near is the wrong trade.
 */
export function diagramStatements(source: string): number {
  let count = 0;
  for (const line of source.split('\n')) {
    const trimmed = line.trim();
    if (trimmed === '' || trimmed.startsWith('%%')) continue;
    for (const part of trimmed.split(';')) {
      if (part.trim() !== '') count += 1;
    }
  }
  return count;
}

/**
 * Is this drawing so wide that shrinking it into the column would flatten it? See
 * [DIAGRAM_ASPECT_LIMIT].
 *
 * `null` — a drawing that did not say how big it is — is not overwide: the browser is in
 * charge of that one, and guessing is what this module refuses to do everywhere else.
 */
export function istUeberbreit(groesse: DiagramSize | null): boolean {
  return groesse !== null && groesse.breite > groesse.hoehe * DIAGRAM_ASPECT_LIMIT;
}

/**
 * Mermaid's own edge cap, said in German — or `null` for any other failure.
 *
 * [DIAGRAM_EDGE_LIMIT] is enforced by the library rather than by us, and the library throws an
 * English `Error` about its own configuration (*"Edge limit exceeded. 201 edges found, but the
 * limit is 200. Initialize mermaid with maxEdges set to a higher number…"*) — which is a
 * sentence about how this wiki is built, addressed to whoever built it, shown to somebody who
 * was trying to draw a flowchart. So it is recognised and answered in the reader's own words.
 *
 * **Matching on a dependency's message is brittle, and that is deliberate here**: the cost of
 * it ceasing to match is the general sentence instead of the specific one. It can degrade to
 * less helpful; it cannot degrade to wrong.
 */
export function diagramEdgeRefusal(error: unknown): string | null {
  const text = error instanceof Error ? error.message : '';
  if (!text.startsWith('Edge limit exceeded')) return null;
  return (
    `Nicht gezeichnet: höchstens ${zahl(DIAGRAM_EDGE_LIMIT)} Verbindungen je Diagramm — ` +
    'ein Diagramm wird im Browser gezeichnet, und ein sehr großes hält die Seite an.'
  );
}

/**
 * The options mermaid is initialised with before each of the two renders.
 *
 * A fresh object every call, and every value stated rather than inherited: a default is a
 * thing a dependency may change, and three of these have the page's safety on them.
 */
export function mermaidConfig(theme: 'default' | 'dark'): MermaidConfig {
  return {
    // Never `'loose'` or `'antiscript'` (both admit script into the rendered SVG), and never
    // `'sandbox'` — the workaround every Mermaid advisory recommends — because that emits
    // `<iframe src="data:text/html;base64,…">` and this application's `frame-src` is
    // `['none']`. See ADR 0018 for why that policy is not loosened to suit a library.
    securityLevel: 'strict',
    // Otherwise mermaid goes looking for `.mermaid` elements in the live document and renders
    // whatever it finds, on its own schedule, outside every cap in this module.
    startOnLoad: false,
    // Read out of the installed package rather than assumed: with this false, a diagram whose
    // RENDERER throws takes mermaid's `errorRenderer` branch, which draws an error diagram and
    // rethrows without reaching its own cleanup — leaving that markup parked in
    // `document.body`. With it true, mermaid removes its temporary elements before it throws.
    suppressErrorRendering: true,
    theme,
    maxTextSize: DIAGRAM_CHARACTER_LIMIT,
    maxEdges: DIAGRAM_EDGE_LIMIT,
    // A label is SVG text, never HTML in a `<foreignObject>`. Mermaid serialises the
    // finished SVG through the DOM's HTML serialiser — so an HTML label containing the
    // documented line break `A[Erste Zeile<br>Zweite Zeile]` comes back with a `<br>` that
    // has no closing tag: valid HTML, and not well-formed XML. `data:image/svg+xml` is
    // parsed as strict XML, so that string does not decode and the reader gets a broken
    // image. With this off, mermaid splits the label on `<br>` into `<tspan>`s and the line
    // break works as documented. It is in [SECURE_CONFIG_KEYS] as well, so a diagram's own
    // `%%{init}%%` cannot put it back.
    htmlLabels: false,
    fontFamily: DIAGRAM_FONT,
    // A copy, because mermaid keeps what it is handed and this array is shared.
    secure: [...SECURE_CONFIG_KEYS]
  };
}

/**
 * One rendered SVG as an address an `<img>` can load — or `null` if it is not an SVG.
 *
 * The refusal is not defensive noise. `alt` aside, an `<img>` whose bytes are not an image is
 * a broken-image icon, which reads as a network fault and sends whoever investigates to the
 * wrong place; the fence's own source is the honest answer instead. Nothing observed makes
 * mermaid return something else, which is precisely why this must not be assumed.
 *
 * **And the prefix test is necessary and not sufficient — this was the bug, not a hypothesis.**
 * Whether a `data:image/svg+xml` decodes is a question only an XML parser can answer, and a
 * string starting `<svg` regularly fails it: an `htmlLabels` label containing the documented
 * Mermaid line break `A[Erste Zeile<br>Zweite Zeile]` came back through the DOM's HTML
 * serialiser with a `<br>` that has no closing tag, and every browser refused the picture. So
 * the address is put through the browser's own image decoder before it reaches the page —
 * `zeichenbar` in `$lib/blocks/mermaid`, which needs a DOM and therefore cannot live here,
 * since this module is imported by the server renderer. This function is the cheap half of
 * that pair and must not be read as the guarantee on its own.
 *
 * Percent-encoded rather than base64: it is smaller for text, and it removes `#` (which would
 * truncate the URI at a fragment), `"` and `<` from the value by construction rather than
 * relying on the attribute's own escaping.
 */
/**
 * How big the drawing is, read out of its own `viewBox` — or `null` if it did not say.
 *
 * **Needed because mermaid sizes an SVG for a page and this one goes into an `<img>`.** It
 * emits `width="100%"` with a `style="max-width: …px"`, which in the document means "as wide
 * as there is room for, but no wider than natural size". Inside an image those two say
 * something else entirely: the `max-width` applies to the SVG inside the image's own viewport
 * rather than to the `<img>` box, and `width="100%"` leaves the image with no intrinsic width
 * at all — so the browser stretches it to fill whatever column it is in, and a three-node
 * diagram is blown up to the width of the prose.
 *
 * The `viewBox` is the honest size. Put on the `<img>` as `width` and `height`, it restores
 * exactly what mermaid meant, and `max-width: 100%` in the stylesheet still shrinks it on a
 * phone.
 *
 * Every value is validated: these numbers are computed from text somebody with write access to
 * one page typed, and they become layout. Anything not finite, not positive, or absurd is `null`
 * rather than a guess, which puts the browser back in charge instead of a bad number.
 */
export function diagramSize(svg: string): DiagramSize | null {
  const box = /<svg[^>]*\sviewBox="([^"]*)"/.exec(svg);
  if (box === null) return null;
  const teile = box[1].trim().split(/[\s,]+/);
  if (teile.length !== 4) return null;
  const breite = Number(teile[2]);
  const hoehe = Number(teile[3]);
  const sinnvoll = (wert: number) => Number.isFinite(wert) && wert > 0 && wert <= 100_000;
  if (!sinnvoll(breite) || !sinnvoll(hoehe)) return null;
  return { breite: Math.round(breite), hoehe: Math.round(hoehe) };
}

export function diagramDataUri(svg: string): string | null {
  const trimmed = svg.trimStart();
  const looksLikeSvg = trimmed.startsWith('<svg') || /^<\?xml[^>]*\?>\s*<svg/.test(trimmed);
  if (!looksLikeSvg) return null;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
