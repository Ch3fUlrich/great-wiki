import { createHighlighterCoreSync } from 'shiki/core';
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript';
import json from '@shikijs/langs/json';
import markdown from '@shikijs/langs/markdown';
import python from '@shikijs/langs/python';
import rust from '@shikijs/langs/rust';
import shellscript from '@shikijs/langs/shellscript';
import sql from '@shikijs/langs/sql';
import typescript from '@shikijs/langs/typescript';
import yaml from '@shikijs/langs/yaml';
import githubDark from '@shikijs/themes/github-dark';
import githubLight from '@shikijs/themes/github-light';
import { codeText, type Block } from '$lib/blocks/render';
import { fenceKey, type CodeToken, type Fence, type Fences } from '$lib/blocks/code';
import { isMathFence } from '$lib/blocks/maths';
import { isDiagramFence } from '$lib/blocks/diagram';

/**
 * Turning a fenced code block into coloured runs of text.
 *
 * # The engine is a policy constraint, not a preference
 *
 * Shiki tokenises with Oniguruma **compiled to WebAssembly** unless it is told otherwise,
 * and instantiating WebAssembly needs `'wasm-unsafe-eval'` in `script-src`. This
 * application's policy is `script-src 'self'` and nothing else (`web/vite.config.ts`,
 * [ADR 0007](../../../../docs/decisions/0007-content-security-policy.md)), so the default
 * engine cannot run in the browser this wiki is served to. It would run everywhere the
 * policy is not in force — in Node, in `vitest`, and, because SvelteKit is lenient there,
 * under `npm run dev` — which makes the failure a production-only one: every code block
 * unstyled, a console violation nobody server-side can see, and a green gate.
 *
 * So `createJavaScriptRegexEngine` is passed explicitly, and `highlight.test.ts` makes
 * `WebAssembly` unreachable and asserts that a fence still comes back coloured. See
 * [ADR 0016](../../../../docs/decisions/0016-the-syntax-highlighter-s-regex-engine.md).
 *
 * # Tokens, never markup
 *
 * What comes back from here is text and two colours per run. It is `{#each}`ed into spans
 * by `CodeView.svelte` and coloured through Svelte's `style:` directive, so nothing this
 * module produces is ever parsed as HTML — `scripts/check-html-sinks.sh` keeps that true,
 * and its exemption list is empty. Shiki's own `codeToHtml` is not imported for exactly
 * that reason; the tokeniser underneath it is.
 *
 * # It runs in the page's `load`, and everything it costs is capped there
 *
 * **This module is under `$lib/server/`, and that is the guarantee rather than a
 * convention**: SvelteKit refuses to let any client-reachable module import it, so the
 * highlighter and its eight grammars cannot reach a reader's browser. `[...path]`'s
 * `+page.server.ts` calls [highlightDocument] once per page load and hands the tokens down
 * as data, exactly as it does for `formeln`.
 *
 * It was first written as a call inside `CodeView.svelte`, and both halves of that were
 * wrong in ways only a measurement shows:
 *
 *  - **The reader downloaded it.** A component renders on the server and again while it
 *    hydrates, so Shiki was in the client bundle — 609 kB raw on every page of a wiki that
 *    is overwhelmingly prose — and ran a second time in the reader's own tab to re-derive
 *    tokens that reader had already been sent. A fence costing the server a second cost the
 *    reader's tab the same second, frozen.
 *  - **Only ONE fence was bounded.** [FENCE_CHARACTER_LIMIT] caps one fence and nothing
 *    capped a page, so twenty of them cost twenty times as much, on the single-threaded SSR
 *    process every other reader's page load queues behind (`Store::open` holds
 *    `max_connections(1)`). Measured before this change: a page of five 20 000-character
 *    markdown fences answered in 51.98 s, and an unrelated page requested two seconds into
 *    that render waited 48.85 s for it.
 *
 * So the caps are per page as well as per fence, and they are four:
 * [LINE_CHARACTER_LIMIT] (the one that matters — the cost is superlinear in LINE length,
 * not in size), [PAGE_FENCE_LIMIT], [PAGE_TOKEN_LIMIT] and [PAGE_BUDGET_MS]. Every call is
 * also inside a `try`: an uncaught throw in a page's `load` is a 500 for the whole route,
 * and that route is also the edit surface, so the page could not be repaired through the
 * editor either.
 */

/**
 * The languages this wiki highlights — and **adding a language is a deliberate act**.
 *
 * This array is the one named place. No reader downloads any of them — the highlighter
 * runs in the page's `load` and only its output crosses the wire — but every grammar
 * listed here is loaded into the server process at start-up and is another parser reading
 * text somebody with write access typed, so the set is the eight the corpus is actually
 * written in rather than the several hundred Shiki ships. A ninth is a decision about what
 * this wiki's server does with a page's contents, not a one-line import.
 *
 * Aliases are NOT repeated here. Each grammar carries its own (`sh`, `bash`, `zsh` for
 * shell; `ts` for TypeScript; `yml`, `rs`, `md`, `py`), and reading them off the grammar
 * is what stops a hand-kept second list drifting from the first.
 */
const GRAMMARS = [json, markdown, python, rust, shellscript, sql, typescript, yaml];

/** The two themes, both carried on every token. See [CodeToken.dark]. */
const THEMES = { light: 'github-light', dark: 'github-dark' } as const;

/**
 * The info strings that mean "print this exactly as it is".
 *
 * D-18 reserves ` ```text ` and ` ```plain ` as the escape hatch from every rendering
 * decision this piece adds — they never highlight and, once diagrams land, never draw. So
 * they are *known* languages that produce no colour, and they get no label: an author who
 * wrote `text` said what they meant and the page must not answer them with "unknown".
 */
const PLAIN_LANGUAGES = new Set(['text', 'plain', 'plaintext', 'txt']);

/**
 * The longest fence that is tokenised, in characters.
 *
 * Chosen for the shape of the corpus with room to spare: the longest fence in
 * `content-example/` is under 300 characters, and 20 000 is a 400-line listing. Past it
 * the block renders as ordinary code, because the cost of tokenising is paid on the SSR
 * path — `Store::open` holds `max_connections(1)`, so a page carrying a megabyte fence
 * would be a lever on the whole deployment rather than on one slow page.
 *
 * **It is not the cap that matters, and believing it was is what made this feature a
 * denial-of-service lever.** Tokenising is superlinear in the length of a LINE and very
 * nearly free in the number of them: measured on this machine, 20 000 characters of
 * TypeScript arrives in 350 ms when the lines are 200 characters and in 1 003 ms when the
 * whole fence is ONE line, and 20 000 characters of Markdown in 6 ms against 11 286 ms for
 * the same two shapes. A pasted minified blob does the second by accident.
 * [LINE_CHARACTER_LIMIT] is the cap that follows from that.
 *
 * Exported so the test asserts against the same number the renderer uses.
 */
export const FENCE_CHARACTER_LIMIT = 20_000;

/**
 * The longest LINE that is tokenised, in characters.
 *
 * The cap the measurements actually justify. Real code wraps: 120 characters is a common
 * house limit and 400 is already four times that, so a line past it is a minified bundle,
 * a base64 blob or a pasted one-liner — none of which highlighting helps anyone read. A
 * fence holding one renders as ordinary code with a note naming the line.
 *
 * Measured, with 20 000 characters in every case and the worst of the eight grammars:
 * lines of 120 cost 396 ms, 200 cost 350 ms, 400 cost 616 ms, 1 000 cost 1 150 ms and
 * 2 000 cost 2 644 ms. So this is what bounds ONE fence at roughly six-tenths of a second,
 * and [PAGE_BUDGET_MS] is what bounds the rest of the page.
 */
export const LINE_CHARACTER_LIMIT = 400;

/**
 * How many fences on one page are tokenised at all.
 *
 * Counted as ATTEMPTS rather than as successes, so a page of fences that each fail costs
 * the same bounded number of calls as a page of fences that all work. Identical fences are
 * tokenised once and looked up thereafter, so this counts distinct ones. A hundred listings
 * is a reference manual, not a wiki page — the same figure, for the same reason, as
 * `PAGE_FORMULA_LIMIT` in `$lib/server/maths`.
 */
export const PAGE_FENCE_LIMIT = 100;

/**
 * How many coloured runs one page may carry.
 *
 * **The cap that bounds the RESPONSE**, and it is here because the tokens travel twice: once
 * as the rendered spans and once as the page data SvelteKit serialises for hydration. A
 * dense 20 000-character listing is about 12 000 tokens, so this is one and a half of them
 * and roughly 600 kB of data — far above the whole of `content-example/`, and far below the
 * several megabytes an unbounded page would otherwise post to every reader.
 *
 * A stop rather than a skip, for the reason `PAGE_MARKUP_LIMIT` is: refusing one fence and
 * trying the next leaves the total where it was, so the total never rises and every
 * remaining fence is tokenised anyway.
 */
export const PAGE_TOKEN_LIMIT = 20_000;

/**
 * How much of the server's time one page's fences may cost, in milliseconds.
 *
 * The bound on the WORK, which none of the three above gives on its own: a page may hold a
 * hundred fences that are each inside every character limit and still cost the one thread
 * every other reader is queued behind more than it can spare. Counted as time actually
 * spent inside the tokeniser rather than as wall clock, so a page competing with something
 * else on the machine is not punished for it, and checked BETWEEN fences — nothing can
 * interrupt a tokenisation already running, so the bound this buys is "the budget, plus one
 * fence", which is what [LINE_CHARACTER_LIMIT] keeps small.
 *
 * A quarter of a second, matching `PAGE_TYPESET_BUDGET_MS` in `$lib/server/maths` because
 * it is the same thread and the same argument. The whole of `content-example/` tokenises in
 * single-digit milliseconds.
 *
 * **What this leaves, stated rather than left to be rediscovered:** a page written to sit
 * exactly on the budget costs about 0.9 s of server time and there is no cache, so
 * requesting it in a loop is still a heavier request than an ordinary page's 0.03 s. That
 * is a factor of thirty against the factor of seventeen hundred it replaces, it is the same
 * shape as any other expensive page, and a content-keyed cache was declined rather than
 * overlooked: it would keep arbitrary page text alive in the server process for the benefit
 * of exactly the page that abuses it.
 */
export const PAGE_BUDGET_MS = 250;

/** The longest language name printed back to the author. */
const LABEL_CHARACTER_LIMIT = 24;

/** The grammar names, for the test that pins the set. Aliases are not names. */
export const CODE_LANGUAGES: readonly string[] = GRAMMARS.flatMap((grammar) =>
  grammar.map((lang) => lang.name)
).sort();

/**
 * The highlighter, built once at module load.
 *
 * `createHighlighterCoreSync` rather than the async `createHighlighterCore`, because this
 * is called from a component's render and Svelte's server renderer is synchronous. The
 * sync constructor also refuses an engine that needs loading, which is the second reason
 * it is the right one here.
 */
const highlighter = createHighlighterCoreSync({
  themes: [githubLight, githubDark],
  langs: GRAMMARS,
  engine: createJavaScriptRegexEngine()
});

/** Every name and alias the highlighter answers to, lower-cased. */
const KNOWN = new Set(highlighter.getLoadedLanguages().map((name) => name.toLowerCase()));

/** A theme colour, or `null` for anything that is not plainly one. */
function colour(value: unknown): string | null {
  return typeof value === 'string' && /^#[0-9a-fA-F]{3,8}$/.test(value) ? value : null;
}

/**
 * The language an author wrote, reduced to one short line — or `null` if they wrote none.
 *
 * `attrs.language` is the info string's first token on the way in
 * (`crates/gw-core/src/markdown.rs`) and an arbitrary JSON value over the collab socket;
 * nothing between the editor and `documents.body` validates it. It is only ever a key
 * looked up in [KNOWN] and a string printed as text — never a class name, never an
 * `import()` specifier, never a `style:` value.
 */
function stated(language: unknown): string | null {
  if (typeof language !== 'string') return null;
  const flat = language.replace(/\s+/g, ' ').trim();
  return flat === '' ? null : flat.slice(0, LABEL_CHARACTER_LIMIT);
}

export function highlightFence(text: string, language: unknown): Fence {
  const name = stated(language);
  if (name === null) return { kind: 'plain', note: null };

  const lower = name.toLowerCase();
  if (PLAIN_LANGUAGES.has(lower)) return { kind: 'plain', note: null };
  if (!KNOWN.has(lower)) return { kind: 'plain', note: `Unbekannte Sprache: ${name}` };
  if (text.length > FENCE_CHARACTER_LIMIT) {
    return { kind: 'plain', note: 'Zu lang für die Hervorhebung' };
  }
  // The cap that does the work. See [LINE_CHARACTER_LIMIT]: a single long line is what
  // costs seconds, and the note names the line rather than the fence so that an author
  // looking at a 40-line listing knows which one to break.
  const laengste = longestLine(text);
  if (laengste > LINE_CHARACTER_LIMIT) {
    return {
      kind: 'plain',
      note: `Zeile mit ${laengste} Zeichen — zu lang für die Hervorhebung`
    };
  }

  let tokens: CodeToken[];
  try {
    tokens = tokenise(text, lower);
  } catch {
    // A grammar that throws must cost this block its colour and nothing else. This runs
    // inside the page's `load`, and an uncaught throw there is a 500 for the whole route —
    // which is also the only way to edit the page that caused it.
    return { kind: 'plain', note: 'Hervorhebung nicht möglich' };
  }

  // The tokeniser splits on `\n` and hands back lines, so a `\r\n` fence comes back one
  // character shorter. Step 1 of this piece existed because a fence's whitespace IS its
  // content; a highlighter that quietly rewrites it is that same bug arriving through a
  // dependency. Anything that does not reconstruct exactly renders plain instead.
  if (tokens.map((token) => token.text).join('') !== text) return { kind: 'plain', note: null };
  return { kind: 'highlighted', tokens };
}

/** Shiki's lines, flattened into one run of tokens with the line breaks put back. */
function tokenise(text: string, lang: string): CodeToken[] {
  const lines = highlighter.codeToTokens(text, { lang, themes: THEMES, defaultColor: false });
  const out: CodeToken[] = [];
  lines.tokens.forEach((line, index) => {
    // The break BEFORE this line, so an empty line (which shiki reports as no tokens at
    // all) still costs exactly one newline.
    if (index > 0) out.push({ text: '\n', light: null, dark: null });
    for (const token of line) {
      out.push({
        text: token.content,
        light: colour(token.htmlStyle?.['--shiki-light']),
        dark: colour(token.htmlStyle?.['--shiki-dark'])
      });
    }
  });
  return out;
}

/** The longest line in a fence, in characters — the number the cost actually follows. */
function longestLine(text: string): number {
  let longest = 0;
  let start = 0;
  for (;;) {
    const brk = text.indexOf('\n', start);
    const end = brk === -1 ? text.length : brk;
    if (end - start > longest) longest = end - start;
    if (brk === -1) return longest;
    start = brk + 1;
  }
}

/**
 * Every fence on this page, tokenised, by the key `CodeView` will look it up under.
 *
 * The same shape as `typesetDocument` in `$lib/server/maths`, deliberately: one walk in the
 * page's `load`, one entry per distinct fence, and every limit applied in one place where
 * the page — rather than the block — is what is being bounded.
 *
 * ` ```math ` and ` ```mermaid ` fences are skipped rather than tokenised. They are drawn by
 * `MathView` and `DiagramView`, which show the fence's own source through `CodeView` with
 * `language="text"` when they cannot draw it, so a listing this walked would be work nobody
 * ever reads.
 */
export function highlightDocument(block: Block | null | undefined): Fences {
  const fences: Fences = new Map();
  if (!block) return fences;

  let attempts = 0;
  let tokens = 0;
  let spent = 0;
  /**
   * The sentence every remaining fence gets, once the page has spent its share.
   *
   * A page-level budget only bounds anything if crossing it CLOSES the page: refusing one
   * fence and trying the next leaves the totals where they were, so they never rise and
   * every later fence is tokenised anyway.
   */
  let erschoepft: string | null = null;

  const highlight = (text: string, language: unknown): Fence => {
    if (erschoepft !== null) {
      // Only a fence that WOULD have been coloured is told why it was not. One that states
      // no language, or `text`, or a language this wiki does not know, has its own answer
      // already and the page's budget is none of its business.
      const fence = highlightFence(text, language);
      return fence.kind === 'plain' ? fence : { kind: 'plain', note: erschoepft };
    }
    if (attempts >= PAGE_FENCE_LIMIT) {
      return { kind: 'plain', note: `Höchstens ${PAGE_FENCE_LIMIT} Blöcke je Seite` };
    }

    attempts += 1;
    const begonnen = performance.now();
    const fence = highlightFence(text, language);
    spent += performance.now() - begonnen;
    if (fence.kind !== 'highlighted') return fence;

    if (tokens + fence.tokens.length > PAGE_TOKEN_LIMIT) {
      erschoepft = 'Der hervorgehobene Code dieser Seite ist an seiner Grenze angelangt';
      return { kind: 'plain', note: erschoepft };
    }
    tokens += fence.tokens.length;
    if (spent > PAGE_BUDGET_MS) {
      erschoepft = `Die Hervorhebung dieser Seite hat ihre ${PAGE_BUDGET_MS} Millisekunden Rechenzeit aufgebraucht`;
    }
    return fence;
  };

  const walk = (node: Block): void => {
    if (node.kind === 'codeBlock') {
      const language = node.attrs?.language;
      if (!isMathFence(language) && !isDiagramFence(language)) {
        const text = codeText(node);
        const key = fenceKey(text, language);
        if (!fences.has(key)) fences.set(key, highlight(text, language));
      }
      return;
    }
    for (const child of node.content ?? []) walk(child);
  };

  walk(block);
  return fences;
}
