import { renderToString, type KatexOptions } from 'katex';
import { codeText, type Block } from '$lib/blocks/render';
import { isMathFence, type Formula, type Formulas } from '$lib/blocks/maths';

/**
 * Typesetting a page's ` ```math ` fences with KaTeX, while the page is being loaded.
 *
 * # Why here and not in the component
 *
 * A Svelte component renders on the server **and again in the browser** while it hydrates,
 * so a component that imported KaTeX would put the whole library — about 272 kB — into
 * every reader's bundle, to re-derive markup that reader was already sent. Doing it in the
 * page's `load` puts KaTeX under `$lib/server/`, which SvelteKit refuses to let any
 * client-reachable module import: a mistake is a build error naming this file rather than a
 * quiet 272 kB. The formula arrives typeset in the first response and works with JavaScript
 * switched off, which was the point of doing it on the server at all.
 *
 * # Why `trust` is the whole of the safety story
 *
 * KaTeX defaults that option to false, and that single default disables `\href`, `\url`,
 * `\includegraphics`, `\htmlClass`, `\htmlId`, `\htmlStyle` and `\htmlData` in one go. There
 * is nothing to configure: the safe setting is the one you get by not typing anything, which
 * is why [katexOptions] does not mention it and why a test reads this file to check that it
 * never starts to.
 *
 * Turning it on would not be a configuration tweak either. ADR 0007 admits
 * `style-src-attr 'unsafe-inline'` into the Content-Security-Policy partly on the sentence
 * *"the renderer does not emit authored CSS into one either way"* (`web/vite.config.ts`), and
 * `\htmlStyle` puts author-written declarations straight into a `style` attribute — so
 * enabling it is an amendment to ADR 0007, argued there, not a line changed here.
 *
 * # Why everything is capped and everything is caught
 *
 * This runs on the shared server rather than in the reader's own tab, and `Store::open` holds
 * `max_connections(1)`, so a slow page load is a lever on the whole deployment rather than on
 * one slow tab. And an uncaught throw in a page's `load` is a 500 for the whole route — a
 * route that is also the only way to edit the page that caused it, so the page could not be
 * repaired afterwards. Hence [FORMULA_CHARACTER_LIMIT], [PAGE_FORMULA_LIMIT],
 * [PAGE_MARKUP_LIMIT], a finite `maxSize` and `maxExpand`, and a `try` around every call.
 *
 * See [ADR 0017](../../../../docs/decisions/0017-what-a-formula-may-do.md).
 */

/**
 * The longest fence that is typeset, in characters.
 *
 * Generous on purpose — you would have to try. A display formula in this corpus is tens of
 * characters long; the largest in `content-example/` is under 60. Five thousand is a page of
 * dense TeX, and past it the fence renders as its own source with a line saying so.
 *
 * What this limit does NOT bound is the size of the answer, which is why
 * [PAGE_MARKUP_LIMIT] exists as well.
 */
export const FORMULA_CHARACTER_LIMIT = 5_000;

/**
 * How many formulas one page is typeset at all.
 *
 * Counted as ATTEMPTS rather than as successes, so that a page of formulas that each fail —
 * over a limit, or throwing — costs the same bounded number of KaTeX calls as a page of
 * formulas that all work. Identical fences are typeset once and looked up thereafter, so
 * this counts distinct ones.
 *
 * A hundred display formulas is a textbook chapter, not a wiki page.
 */
export const PAGE_FORMULA_LIMIT = 100;

/**
 * How much typeset markup one page may carry, in characters.
 *
 * **The cap that actually bounds the response, and it is here because KaTeX amplifies.**
 * Measured, not assumed: `x + ` repeated comes back at roughly 320 characters of markup per
 * source character, so one fence comfortably inside [FORMULA_CHARACTER_LIMIT] can be more
 * than a megabyte on its own. An input cap bounds the parser's work; it bounds the answer's
 * size only by a factor nobody would guess.
 *
 * A typical formula is about 3 kB of markup, so a hundred of them sit at roughly a third of
 * this. A render whose result would cross the line is thrown away rather than trimmed — half
 * a formula is not a formula — and that fence renders as its own source instead.
 *
 * **It is a stop and not a skip**, which is the whole of the difference between bounding the
 * answer and bounding the work. Refusing one formula for crossing the line and then trying
 * the next leaves the total where it was, so the total never rises and every one of
 * [PAGE_FORMULA_LIMIT]'s hundred renders still runs — measured at 7.3 s of server time for a
 * page that kept exactly one formula. So the first refusal closes the page: every later
 * fence gets the same sentence without being rendered.
 */
export const PAGE_MARKUP_LIMIT = 1_000_000;

/**
 * How much of the server's time one page's formulas may cost, in milliseconds.
 *
 * **The cap that bounds the WORK, which neither of the two above does.** Markup and CPU are
 * not proportional: `\begin{array}` fills [PAGE_MARKUP_LIMIT] in one render, while
 * `\text{a a a …}` measures at 20 ms for 15 kB — sixty of those stay inside the markup
 * budget and cost well over a second of the one thread every other reader's page load is
 * queued behind. `Store::open` holds `max_connections(1)`, so that thread is the deployment.
 *
 * Counted as time actually spent inside KaTeX rather than as wall clock, so a page competing
 * with something else on the machine is not punished for it, and checked BETWEEN renders —
 * nothing can interrupt a render in progress, so the bound this buys is "the budget, plus
 * one formula", which is why the per-formula character limit exists as well.
 *
 * A quarter of a second is far above what a real page spends: the whole of
 * `content-example/` typesets in single-digit milliseconds.
 */
export const PAGE_TYPESET_BUDGET_MS = 250;

/** German thousands separators, without depending on which locale data Node was built with. */
function zahl(value: number): string {
  return String(value).replace(/\B(?=(\d{3})+(?!\d))/g, '.');
}

/** Why a refusal is a refusal: the work is paid for by everyone, not by whoever wrote it. */
const WEIL = 'Formeln werden auf dem Server gesetzt, den sich alle Lesenden teilen.';

/**
 * The options every call is made with — **a fresh object every time**.
 *
 * `macros` in particular: KaTeX writes `\gdef` definitions INTO the object it is handed, so
 * one shared object would let a single formula redefine what every later formula on the site
 * means. This module is loaded once per server process, so that would outlive the request and
 * reach pages whose authors cannot edit the page it came from.
 */
export function katexOptions(): KatexOptions {
  return {
    // A fence is display maths by definition: it is a block, on its own lines.
    displayMode: true,
    // Turns a KaTeX ParseError into a rendered error node carrying the author's own source,
    // in KaTeX's error colour, instead of a throw. It is a nicety rather than the guard —
    // KaTeX throws outside that path too (deep nesting overflows the parser's stack and
    // arrives as a RangeError), which is what the `try` in [typeset] is actually for.
    throwOnError: false,
    // Warnings are written to the server's console, and the input is written by whoever may
    // edit a page. `warn` would let one page fill the log at one request per line.
    strict: 'ignore',
    // Defaults to Infinity, which makes `\rule{500em}{500em}` a layout bomb any author could
    // leave on a page and no reader could scroll past.
    maxSize: 50,
    // What bounds macro expansion. This is KaTeX's own default today; it is stated because a
    // default is a thing a dependency may change, and this one has a page's load time on it.
    maxExpand: 1_000,
    macros: {}
  };
}

/**
 * Every ` ```math ` fence on this page, typeset, by the fence text that produced it.
 *
 * The key is `codeText(block)` — exactly the string `BlockView` hands the leaf component —
 * so the two sides cannot key on different strings. Identical fences share one entry and
 * one render.
 */
export function typesetDocument(block: Block | null | undefined): Formulas {
  const formulas: Formulas = new Map();
  if (!block) return formulas;

  let attempts = 0;
  let markup = 0;
  let spent = 0;
  /**
   * The sentence every remaining fence gets, once the page has spent its share.
   *
   * A page-level budget only bounds anything if crossing it CLOSES the page. Refusing one
   * formula and trying the next leaves the totals where they were, so they never rise and
   * every later render still runs — which is how a page could cost 7.3 s of the shared
   * server and keep one formula. Once this is set, nothing else on the page is handed to
   * KaTeX at all.
   */
  let erschoepft: string | null = null;

  const typeset = (text: string): Formula => {
    if (text.length > FORMULA_CHARACTER_LIMIT) {
      return {
        kind: 'source',
        note: `Nicht gesetzt: ${zahl(text.length)} Zeichen, höchstens ${zahl(FORMULA_CHARACTER_LIMIT)} je Formel — ${WEIL}`
      };
    }
    // Before the count, because it is the stronger statement: the page is finished, not
    // merely long.
    if (erschoepft !== null) return { kind: 'source', note: erschoepft };
    if (attempts >= PAGE_FORMULA_LIMIT) {
      return {
        kind: 'source',
        note: `Nicht gesetzt: höchstens ${zahl(PAGE_FORMULA_LIMIT)} Formeln je Seite — ${WEIL}`
      };
    }

    attempts += 1;
    let html: string;
    const begonnen = performance.now();
    try {
      html = renderToString(text, katexOptions());
    } catch {
      // Deliberately says nothing about what went wrong. KaTeX's own message is English, is
      // about TeX rather than about this page, and for the errors that reach here (a stack
      // overflow, most often) says nothing an author could act on. The source is shown above
      // this line, which is the actionable part.
      return { kind: 'source', note: 'Diese Formel konnte nicht gesetzt werden.' };
    } finally {
      // In a `finally`, so a formula that threw is still charged for the time it burnt
      // getting there: a page of formulas that each fail expensively is the same lever as a
      // page of formulas that each succeed expensively.
      spent += performance.now() - begonnen;
    }

    if (markup + html.length > PAGE_MARKUP_LIMIT) {
      erschoepft = `Nicht gesetzt: der Formelsatz dieser Seite ist bei ${zahl(PAGE_MARKUP_LIMIT)} Zeichen angelangt — ${WEIL}`;
      return { kind: 'source', note: erschoepft };
    }
    markup += html.length;
    // Checked after this formula is kept rather than before it is tried, so the budget is
    // spent on formulas and the page that fits inside it is unaffected. Nothing can
    // interrupt a render already running, so the real bound is "the budget plus one
    // formula" — which is what the per-formula character limit is for.
    if (spent > PAGE_TYPESET_BUDGET_MS) {
      erschoepft = `Nicht gesetzt: der Formelsatz dieser Seite hat seine ${zahl(PAGE_TYPESET_BUDGET_MS)} Millisekunden Rechenzeit aufgebraucht — ${WEIL}`;
    }
    return { kind: 'typeset', html };
  };

  const walk = (node: Block): void => {
    if (node.kind === 'codeBlock') {
      // A fence's children are its text leaves, and `codeText` has already read them.
      if (isMathFence(node.attrs?.language)) {
        const text = codeText(node);
        if (!formulas.has(text)) formulas.set(text, typeset(text));
      }
      return;
    }
    for (const child of node.content ?? []) walk(child);
  };

  walk(block);
  return formulas;
}
