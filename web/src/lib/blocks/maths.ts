/**
 * What both halves of display maths agree on.
 *
 * A ` ```math ` fence is typeset by KaTeX **while the page is loaded on the server**
 * (`$lib/server/maths`), and drawn by `MathView.svelte`. This module is the only thing the
 * two share, and it is deliberately tiny: it holds no KaTeX, so it costs the reader's
 * browser nothing, and it holds the two questions that would be a silent bug if the walker
 * and the renderer ever answered them differently —
 *
 *  - **which fence is a formula** ([isMathFence]). A fence the walker typeset and the reader
 *    drew as code is unrendered maths that nothing reports; the reverse is a formula told it
 *    exceeded a limit it never reached.
 *  - **how a fence is looked up** ([formulaFor]). The key is the fence's own text, exactly as
 *    `codeText` reconstructs it, so the two sides cannot key on different strings.
 *
 * Why the typesetting is not simply done here, in the component, where it would be shorter:
 * a component renders on the server AND again in the browser during hydration, so importing
 * KaTeX into one would put the whole library in the reader's bundle — about 272 kB to
 * re-derive markup the reader was already sent. Doing it in the page's `load` puts KaTeX in
 * `$lib/server/`, which SvelteKit refuses to let any client-reachable module import: the
 * guarantee is a build error rather than a promise. See
 * [ADR 0017](../../../../docs/decisions/0017-what-a-formula-may-do.md).
 */

/** What the server made of one ` ```math ` fence. */
export type Formula =
  /** KaTeX's markup for it. The one string this reader ever puts into the page as markup. */
  | { kind: 'typeset'; html: string }
  /**
   * It was not typeset, and this German line says which limit stopped it and why.
   *
   * The reader shows the fence's own source above the line. That is the same thing a reader
   * with no page load behind them sees (the editor's preview), except that they are told
   * nothing — being refused and never being asked are different states and must not be
   * reported as the same one.
   */
  | { kind: 'source'; note: string };

/**
 * Every formula on one page, by the fence text that produced it.
 *
 * **A `Map`, not a plain object, and that is a bug fix rather than a style.** The key is text
 * that anyone with write access to the page typed, and this crosses the wire as page data —
 * SvelteKit serialises that with `devalue`, whose parser THROWS on an object carrying a
 * `__proto__` property (`node_modules/devalue/src/parse.js`). A page holding a ` ```math `
 * fence whose whole content is `__proto__` would have failed to load at all, on a route that
 * is also the only way to edit the page that caused it. A `Map` has no such key, devalue
 * carries it natively, and `Map.get` cannot answer out of `Object.prototype` either.
 */
export type Formulas = Map<string, Formula>;

/**
 * Is this fence's info string the one that means "typeset this"?
 *
 * `math` and nothing else. `latex`, `tex` and `katex` are deliberately not aliases: each
 * spelling admitted is a spelling that has to be argued about again later, and an author who
 * writes one of them is told the wiki does not know that language rather than being silently
 * given something they did not ask for.
 *
 * `attrs.language` is the info string's first token on the way in
 * (`crates/gw-core/src/markdown.rs`) and an arbitrary JSON value over the collaboration
 * socket — nothing between the editor and `documents.body` validates it — so this takes
 * `unknown` and answers `false` for everything that is not a string.
 */
export function isMathFence(language: unknown): boolean {
  return typeof language === 'string' && language.trim().toLowerCase() === 'math';
}

/**
 * What the server made of this fence, or `null` if it made nothing of it.
 *
 * `null` covers two different situations on purpose, and the caller must not tell them
 * apart: nobody typeset this page (the editor renders the same `BlockView` while TipTap
 * mounts, with no page load behind it), or this exact fence was not among the ones walked.
 * Both draw the fence's source and say nothing about limits, because in neither case was a
 * limit reached.
 */
export function formulaFor(
  formulas: Formulas | null | undefined,
  source: string
): Formula | null {
  return formulas?.get(source) ?? null;
}
