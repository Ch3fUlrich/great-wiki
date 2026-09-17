import {
  DIAGRAM_FRAME_MARKE,
  DIAGRAM_FRAME_PATH,
  DIAGRAM_NICHT_GELADEN,
  DIAGRAM_NICHT_GEZEICHNET,
  diagramDataUri,
  diagramFailureNote,
  diagramSize,
  rahmenAntwort,
  type Diagram,
  type DiagramSize
} from './diagram';

/**
 * Drawing a ` ```mermaid ` fence — in the reader's browser, twice, into two `<img>` addresses.
 *
 * # This module loads no library, and that is the change (D-26)
 *
 * Mermaid used to run here, on the page, behind a `browser`-guarded dynamic import. It does
 * not any more. It runs in an `<iframe>` served from [DIAGRAM_FRAME_PATH] — one route of this
 * same application, with a policy scoped to that route (`$lib/csp`'s `diagramFramePolicy`,
 * applied in `hooks.server.ts`) — and this module talks to it over `postMessage`: a
 * diagram's text and a theme go out, an SVG string comes back.
 *
 * **What that bought, and what the defect was.** Mermaid needs a DOM to measure text, and
 * while it measures it inserts a style ELEMENT into the document. `style-src 'self'`
 * refused it — correctly; that is the CSS-injection class — so every diagram logged four
 * *"Refused to apply inline style"* errors, and mermaid laid its labels out against the
 * page's font rather than the one it was about to draw with. The drawing survived (the same
 * `<style>` is serialised into the returned string and is the image's own business inside
 * the `<img>`), so this was noise plus a measurement made against the wrong face — noise in
 * exactly the place a developer looks first. The library has no nonce hook, and
 * `'unsafe-inline'` on `style-src` is the one loosening ADR 0007 refused: it would also make
 * `widenCspNonceToStyles` skip the directive and unstyle the *editor*, in production only.
 *
 * So ADR 0018's "barrier one" — the Content-Security-Policy holding while a dependency
 * worked inside the reader's page — is now a real boundary. Author-written diagram text
 * never enters this document at any point, in any form. The reader's page keeps
 * `style-src 'self'`; the only directive that moved for this feature is `frame-src`, from
 * `'none'` to `'self'`, and `frame-ancestors 'self'` on the frame's own response is what
 * stops anyone else embedding it.
 *
 * **Barrier two is unchanged.** The string that comes back is percent-encoded into a
 * `data:image/svg+xml` URI and set as an `<img src>` — an attribute, not markup. It is never
 * parsed, never inserted, never given to a DOM API that reads HTML. `scripts/check-html-sinks.sh`
 * has nothing to find here and its exemption list stays empty. The address is put through
 * the browser's own image decoder first ([zeichenbar]), because containment is worth nothing
 * if what arrives is a broken-image icon.
 *
 * # Everything crossing the boundary is untrusted data
 *
 * A `message` listener hears every message its window is ever sent. What this module acts on
 * is decided by [rahmenAntwort] in `$lib/blocks/diagram`, which requires the sender to be the
 * one window this page created (`iframe.contentWindow`, a field the sender cannot choose) at
 * this page's own origin, and then validates the payload field by field. The frame applies
 * the mirror-image check to what it is sent. Both are pure functions with tests, because a
 * boundary check that can only be exercised in a browser is one that is exercised once, by
 * hand, and then trusted forever.
 *
 * # Twice, once per theme (D-24)
 *
 * An `<img>` cannot inherit `prefers-color-scheme`, and this wiki has a light/dark control,
 * so one fixed image can only ever match one of them. Both are drawn and
 * `DiagramView.svelte`'s stylesheet shows whichever applies. **The cost, stated rather than
 * hidden: every diagram is drawn twice and carried twice in the markup.** It is paid in the
 * reader's own tab, once per page, for a wiki of tens of pages — and it buys a diagram that
 * is never wrong against its own background.
 *
 * # The caps, the security level and the `secure` list did not move
 *
 * They live in `$lib/blocks/diagram`, which both documents import: `diagramRefusal` is asked
 * by the component before anything is fetched at all, and `mermaidConfig` is applied inside
 * the frame. One definition, two callers — rather than a copy in each document, which is how
 * a `securityLevel` comes to differ between them without anything failing.
 */

/**
 * How long the frame has to load its library and say so.
 *
 * Generous because it pays for a download: the frame fetches the whole of mermaid — by a
 * wide margin the largest thing in this application — before it reports ready, so that
 * "ready" means "will answer promptly" rather than "exists". Under `npm run dev` that is
 * several hundred separate module requests, which is why this is a minute and not ten
 * seconds. Nobody waits it out: the reader is looking at the fence's own source the whole
 * time, exactly as a reader with no JavaScript does.
 */
const BEREIT_MS = 60_000;

/**
 * How long one drawing has, once the library is in hand.
 *
 * Sized against the measurement in `$lib/blocks/diagram`: a diagram sitting on
 * `DIAGRAM_STATEMENT_LIMIT` costs seconds, and the shapes that cost sixteen are the ones the
 * caps already refuse. Thirty is therefore "something is wrong" rather than "this one is
 * big", and what it prevents is a frame that died quietly leaving a page of diagrams
 * permanently mid-render.
 */
const ZEICHNEN_MS = 30_000;

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
 * It stays on THIS side of the frame deliberately. It is a question about the picture the
 * reader's page is about to show, answered by the decoder that page will use, and an
 * `Image` is not a DOM insertion — the containment is unaffected either way.
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

/** The one frame, once it is in the document. `null` until the first diagram asks for it. */
let rahmen: HTMLIFrameElement | null = null;

/** The handshake. Created on first use and then reused — success or failure, it is the answer. */
let angemeldet: Promise<HTMLIFrameElement> | null = null;

/** Who is waiting for which answer, by the number the order carried. */
const offen = new Map<number, (svg: string | null, fehler: string | null) => void>();

/** Order numbers. Monotonic, so a late answer can never be mistaken for the current one. */
let laufend = 0;

/**
 * Every message this window hears, and the one in a thousand that is an answer.
 *
 * [rahmenAntwort] is the whole of the check and it is deliberately not inline: it requires
 * `event.source` to be this page's own `iframe.contentWindow` and `event.origin` to be this
 * page's origin, then validates the payload. Neither of those two fields can be chosen by a
 * sender. Everything else on the wire is treated as data of unknown shape.
 */
function empfangen(ereignis: MessageEvent): void {
  const antwort = rahmenAntwort(ereignis, {
    rahmen: rahmen?.contentWindow ?? null,
    origin: location.origin
  });
  if (antwort === null) return;
  if (antwort.art === 'bereit') {
    melden?.(rahmen as HTMLIFrameElement);
    return;
  }
  if (antwort.art === 'svg') offen.get(antwort.id)?.(antwort.svg, null);
  else offen.get(antwort.id)?.(null, antwort.fehler);
}

/** Resolves the handshake promise. Held here because the `bereit` message is what settles it. */
let melden: ((el: HTMLIFrameElement) => void) | null = null;

/**
 * Put the frame in the document, once, and wait for it to say it is ready.
 *
 * **Lazily, and only from [drawDiagram]** — which is reached from a component effect on a
 * page that holds a diagram. A page with no diagram on it creates no frame, fetches no
 * mermaid and pays for none of this, which is the same promise the dynamic import used to
 * make and is now made one document further out.
 *
 * # The sandbox
 *
 * `allow-scripts allow-same-origin`. **`allow-same-origin` is required and its absence
 * would be silent**: a sandboxed frame without it has an opaque origin, `'self'` in the
 * frame's own policy then matches nothing — including the module chunks it must load, which
 * cannot be nonced because a nonce does not reach a dynamic `import()` — and `event.origin`
 * from it is `"null"`, a value any sandboxed frame anywhere can present. So the frame is
 * same-origin by construction and the sandbox is NOT an origin boundary; it is not described
 * as one. What it still removes is top-level navigation, popups, form submission, downloads,
 * modal dialogs and the pointer/presentation/orientation locks. `web/src/routes/_diagramm/rahmen.ts`
 * carries the long form of this, next to the code that depends on it.
 *
 * # Why it is an address and not an inline document
 *
 * A frame with a local URL — `about:blank`, or a document written inline into the attribute
 * that takes one — INHERITS its embedder's policy, so `style-src 'self'` would follow
 * mermaid into it and nothing would have changed. Only a real response can carry a policy of
 * its own. (That attribute is separately one of the spellings `scripts/check-html-sinks.sh`
 * refuses; it is named rather than written, because the check greps this directory.)
 */
function rahmenOeffnen(): Promise<HTMLIFrameElement> {
  if (angemeldet !== null) return angemeldet;

  angemeldet = new Promise<HTMLIFrameElement>((aufloesen, ablehnen) => {
    melden = aufloesen;
    const el = document.createElement('iframe');
    el.setAttribute('sandbox', 'allow-scripts allow-same-origin');
    // Out of the document's flow, out of the tab order and out of the accessibility tree:
    // there is nothing in it for a reader, and the diagram's own description is the `alt` on
    // the picture it produces. Not `display: none` — a frame is allowed to be hidden that
    // way and browsers deprioritise the script inside one.
    el.setAttribute('aria-hidden', 'true');
    el.setAttribute('tabindex', '-1');
    el.title = 'Diagrammzeichner';
    el.style.position = 'absolute';
    el.style.width = '0';
    el.style.height = '0';
    el.style.border = '0';
    el.style.visibility = 'hidden';
    el.src = DIAGRAM_FRAME_PATH;
    rahmen = el;
    window.addEventListener('message', empfangen);
    document.body.appendChild(el);

    // The frame reports ready only once its library is in hand, so this timeout covers the
    // download as well as the load. A frame that never reports gets DIAGRAM_NICHT_GELADEN —
    // the sentence this application already had for a renderer it could not fetch.
    setTimeout(() => ablehnen(new Error('der Diagrammrahmen hat sich nicht gemeldet')), BEREIT_MS);
  });
  // The promise is stored and awaited by every later diagram, but the rejection is handled at
  // each await rather than here — this keeps a failed handshake from being an unhandled
  // rejection in the window between creation and the first `await`.
  void angemeldet.catch(() => undefined);
  return angemeldet;
}

/**
 * Ask the frame for one drawing and wait for exactly that answer.
 *
 * The order number is what makes the wait safe: an answer to a diagram that timed out two
 * minutes ago arrives at an entry that is no longer in the map and is dropped, rather than
 * being handed to whoever is waiting now.
 */
function frage(
  el: HTMLIFrameElement,
  auftrag: { id: number; quelle: string; thema: 'default' | 'dark' }
): Promise<{ svg: string | null; fehler: string | null }> {
  return new Promise((aufloesen) => {
    const uhr = setTimeout(() => {
      offen.delete(auftrag.id);
      aufloesen({ svg: null, fehler: null });
    }, ZEICHNEN_MS);

    offen.set(auftrag.id, (svg, fehler) => {
      clearTimeout(uhr);
      offen.delete(auftrag.id);
      aufloesen({ svg, fehler });
    });

    // `location.origin` and never `'*'`: the diagram's text is the author's and the frame is
    // this application's own, and a `'*'` target would post it to whatever document happened
    // to answer that address.
    el.contentWindow?.postMessage(
      { gw: DIAGRAM_FRAME_MARKE, id: auftrag.id, quelle: auftrag.quelle, thema: auftrag.thema },
      location.origin
    );
  });
}

/**
 * One diagram at a time, page-wide.
 *
 * The frame serialises its own work as well, so this is not what keeps two renders from
 * reading each other's theme. What it keeps is the page's own promise: each `await` here
 * yields to the event loop, so a page of twenty diagrams paints between them instead of
 * resolving twenty pictures into the DOM at once.
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
  let el: HTMLIFrameElement;
  try {
    el = await rahmenOeffnen();
  } catch {
    return { kind: 'source', note: DIAGRAM_NICHT_GELADEN };
  }

  // Light first: it is the one the majority of readers will be shown, and if the second render
  // fails there is nothing useful to do with a half-themed pair anyway.
  const hell = await once(el, source, 'default');
  if ('note' in hell) return { kind: 'source', note: hell.note };
  const dunkel = await once(el, source, 'dark');
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
 * One render, in one theme, as seen from this side of the frame.
 *
 * Nothing about mermaid appears here any more: the configuration, the temporary elements and
 * the `finally` that sweeps them up are the frame's, where the library is. What is left is
 * the three questions this page has to answer about a string somebody else produced — is it
 * an SVG at all, will this browser decode it, and how big does it say it is.
 */
async function once(
  el: HTMLIFrameElement,
  source: string,
  theme: 'default' | 'dark'
): Promise<Versuch> {
  const { svg, fehler } = await frage(el, { id: (laufend += 1), quelle: source, thema: theme });

  // `diagramFailureNote` tells "nothing came back" from "it would not draw this" and
  // translates mermaid's edge cap on the way past. It lives in `$lib/blocks/diagram` because
  // it is the one part of this that a test without a browser can hold.
  if (svg === null) return { note: diagramFailureNote(fehler) };

  const src = diagramDataUri(svg);
  if (src === null) return { note: DIAGRAM_NICHT_GEZEICHNET };
  // Decoded here rather than trusted, so that a picture the browser cannot read never
  // reaches the page as a broken-image glyph. See [zeichenbar].
  if (!(await zeichenbar(src))) return { note: DIAGRAM_NICHT_GEZEICHNET };
  return { src, groesse: diagramSize(svg) };
}
