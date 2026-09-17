import { browser } from '$app/environment';
import {
  DIAGRAM_FRAME_MARKE,
  mermaidConfig,
  rahmenAuftrag,
  type DiagramAuftrag
} from '$lib/blocks/diagram';

/**
 * Mermaid, in a document of its own — the whole of what runs inside the diagram frame (D-26).
 *
 * # Why there is a frame at all
 *
 * Mermaid needs a browser DOM to measure text, and while it measures it inserts a style
 * ELEMENT into the document it is measuring in. On the wiki's own pages `style-src 'self'`
 * refuses that element — correctly; an injected style element is the CSS-injection class — so
 * every diagram logged four *"Refused to apply inline style"* errors and mermaid laid its
 * labels out against the page's font rather than the one it was about to draw with. The
 * library exposes no nonce hook (verified against `mermaid@11.17.2`), and `'unsafe-inline'`
 * on the page is the one loosening ADR 0007 refused.
 *
 * So the library was moved off the page instead of the policy being moved around the
 * library. This module is served from `/_diagramm` with a policy scoped to that route
 * (`$lib/csp`'s `diagramFramePolicy`), the page posts a diagram's text here, and an SVG
 * STRING goes back. ADR 0018's "barrier one" — the policy holding while mermaid worked
 * inside the page — becomes a real boundary: mermaid never touches the reader's page at all.
 *
 * # The sandbox flags, which are the easiest thing here to get wrong
 *
 * The page creates the frame with `sandbox="allow-scripts allow-same-origin"`
 * (`$lib/blocks/mermaid`). **`allow-same-origin` is not optional and its absence would be
 * silent**, so the reasoning lives here, next to the code that depends on it:
 *
 * - A frame sandboxed WITHOUT `allow-same-origin` has an **opaque** origin. `'self'` in this
 *   route's own policy then matches nothing at all — including the SvelteKit and mermaid
 *   module chunks this document must load, which are `'self'` and cannot be nonced because
 *   a nonce does not reach a dynamic `import()`. The frame would load, render an empty
 *   document, and refuse its own script.
 * - An opaque origin also sends `Origin: null`, which is not this application's origin — so
 *   both halves of the `postMessage` check below would have to be dropped or weakened to
 *   accept `"null"`, which is a value ANY sandboxed frame anywhere can present.
 *
 * **What the sandbox therefore does and does not buy, stated rather than implied.** With
 * both flags this document is same-origin with the page: it can reach `parent.document` and
 * the same `localStorage`, and nothing in the attribute prevents that. It is not an origin
 * boundary and must not be described as one. What it still removes is real and is kept for
 * it: no top-level navigation, no popups, no form submission, no downloads, no modal
 * dialogs, no pointer lock, no presentation or orientation lock. Script that escaped
 * mermaid into this document could not navigate the reader's tab to somewhere else or hand
 * them a file; it could reach back through `parent` to undo that, which is why the sandbox
 * is defence in depth and `script-src` — untouched here, no `'unsafe-inline'`, no
 * `'unsafe-eval'` — is the barrier.
 *
 * **The real isolation is the document, not the sandbox**: a second document with its own
 * policy, its own `document.body` for mermaid to measure in, and its own style context. The
 * reader's page keeps `style-src 'self'` and gains nothing author-controlled in its DOM.
 *
 * # What this module does NOT do
 *
 * It never inserts the finished SVG anywhere. `mermaid.render(id, text)` — the two-argument
 * form — returns a string, the string is posted to the page, and the page makes it a
 * `data:image/svg+xml` address for an `<img>`. The three-argument form would have mermaid
 * perform the DOM write on our behalf, which is the sink `scripts/check-html-sinks.sh`
 * greps for in any file that mentions mermaid — this one included.
 */

/**
 * The library, in the browser only.
 *
 * `$app/environment`'s `browser` is replaced with a literal at build time, so in the SSR
 * build this reads `false ? import(…) : …` and rollup drops the import entirely — no server
 * chunk is emitted and the server bundle names no package. That matters here as much as it
 * ever did on the page: this route is a SvelteKit page like any other, it is server-rendered
 * like any other, and `web/scripts/check-server-bundle.sh` refuses a server bundle that
 * imports a package the production image has no `node_modules` for.
 *
 * A bare `import()` inside a branch that never executes on the server does NOT achieve that
 * — the chunk is still emitted. Same shape, same reasoning, as `loadEditor` in
 * `[...path]/+page.svelte`.
 *
 * The server-side branch returns a promise that never settles rather than rejecting: it is
 * unreachable ([rahmenStarten] is called from an effect, and effects do not run during
 * server rendering), and a rejection would have to be rendered into an SSR response.
 */
const loadMermaid = () =>
  browser ? import('mermaid') : new Promise<typeof import('mermaid')>(() => {});

/**
 * Ids for mermaid's temporary elements. A counter rather than a random value: it is used as
 * a `#id` selector by the library, it must not collide with anything in this document, and a
 * deterministic one is greppable when something does go wrong.
 */
let laufend = 0;

/**
 * Listen, draw, answer — and a teardown for the effect that called this.
 *
 * Returns a function that removes the listener, so a hot reload or an unmount cannot leave
 * two of them answering the same order twice.
 */
export function rahmenStarten(): () => void {
  // Nothing at all when this document is not framed. `window.parent === window` in a tab
  // somebody opened `/_diagramm` in directly, and `rahmenAuftrag` refuses every message in
  // that case — this is the same refusal one step earlier, so that such a tab does not even
  // hold a listener. There is nothing on this page to see: no content, no controls, no data.
  if (window.parent === window) return () => {};

  // Loaded EAGERLY, before the page is told this frame is ready, so that "ready" means "will
  // answer promptly" rather than "exists". The page's two timeouts are sized around that
  // division: a generous one for the handshake, which pays for the library's download, and a
  // shorter one per drawing. See `$lib/blocks/mermaid`.
  const bibliothek = loadMermaid().then((modul) => modul.default);

  // One drawing at a time. Mermaid's configuration is global — `initialize` sets a site
  // config that the next `render` reads — so two renders in flight would be one render
  // reading the other's theme. The page serialises its own requests as well; this is the
  // half that cannot be undone by a second page framing this document.
  let schlange: Promise<unknown> = Promise.resolve();

  const hoeren = (ereignis: MessageEvent) => {
    const auftrag = rahmenAuftrag(ereignis, {
      eltern: window.parent,
      selbst: window,
      origin: location.origin
    });
    if (auftrag === null) return;
    schlange = schlange.then(
      () => beantworten(bibliothek, auftrag),
      () => beantworten(bibliothek, auftrag)
    );
  };

  window.addEventListener('message', hoeren);

  // Told once, unprompted, and only after the library is in hand. A failed import means this
  // never arrives and the page falls back to the fence's own source with the line it already
  // had for a diagram renderer that could not be fetched.
  void bibliothek.then(
    () => antworte({ bereit: true }),
    () => {
      /* The page's handshake timeout is the answer; there is nobody here to tell. */
    }
  );

  return () => window.removeEventListener('message', hoeren);
}

/** Post one answer back to the page that framed this document. */
function antworte(nutzlast: Record<string, unknown>): void {
  // `location.origin` rather than `'*'`: the frame is same-origin by construction, and a
  // `'*'` target would send the drawing to whatever document happened to be the parent —
  // which is a thing an embedder chooses, not this application.
  window.parent.postMessage({ gw: DIAGRAM_FRAME_MARKE, ...nutzlast }, location.origin);
}

/**
 * One order: draw it, answer with the string, and sweep up whatever mermaid left behind.
 *
 * `initialize` before every render rather than once at load — it is what carries the theme,
 * and it rebuilds the site configuration from mermaid's defaults each time, so nothing a
 * previous diagram's `%%{init}%%` directive did can survive into this one. (Mermaid resets
 * its directives per render as well, so this is the second of two.)
 *
 * The `finally` is the part that is not obvious. With `suppressErrorRendering` set, mermaid
 * removes its own temporary elements before throwing — but "the measuring container is ours"
 * is not available to us, because owning the container means the three-argument form and
 * that form performs the DOM write ADR 0014 forbids. So the append is mermaid's and the
 * removal is also ours: the ids are ours, and anything still carrying one when this returns
 * is swept out of this document.
 */
async function beantworten(
  bibliothek: Promise<(typeof import('mermaid'))['default']>,
  auftrag: DiagramAuftrag
): Promise<void> {
  const id = `gw-diagramm-${(laufend += 1)}`;
  try {
    const mermaid = await bibliothek;
    mermaid.initialize(mermaidConfig(auftrag.thema));
    const { svg } = await mermaid.render(id, auftrag.quelle);
    antworte({ id: auftrag.id, svg });
  } catch (fehler) {
    // Mermaid's own message, verbatim and in English. It is not shown to anybody: the page
    // recognises the edge-limit sentence and answers it in German, and says its own general
    // line for everything else. Passing the text rather than a flag is what keeps that
    // recognition in one place instead of in two documents.
    antworte({ id: auftrag.id, fehler: fehler instanceof Error ? fehler.message : String(fehler) });
  } finally {
    // `d…` is the enclosing div mermaid appends to `document.body`, `i…` the iframe it would
    // use under a security level this application never sets; both are named after the id.
    for (const uebrig of [id, `d${id}`, `i${id}`]) document.getElementById(uebrig)?.remove();
  }
}
