/**
 * What both halves of a fenced code block agree on.
 *
 * A fence is tokenised by Shiki **while the page is loaded on the server**
 * (`$lib/server/highlight`), and drawn by `CodeView.svelte`. This module is the only thing
 * the two share, and it is deliberately tiny: it holds no highlighter, so it costs the
 * reader's browser nothing, and it holds the one question that would be a silent bug if the
 * walker and the renderer ever answered it differently — **how a fence is looked up**
 * ([fenceKey]). The key is the language and the fence's own text, exactly as `codeText`
 * reconstructs it, so the two sides cannot key on different strings.
 *
 * Why the tokenising is not simply done in the component, where it would be shorter: a
 * component renders on the server AND again in the browser during hydration, so importing
 * Shiki into one puts the library and all eight grammars in the reader's bundle — 609 kB
 * raw on every page of a wiki that is overwhelmingly prose — and re-runs the tokeniser
 * during hydration to re-derive what the reader was already sent. That second run is on the
 * reader's own main thread: a fence that costs the server a second costs the reader's tab
 * the same second, frozen. Doing it in the page's `load` puts Shiki under `$lib/server/`,
 * which SvelteKit refuses to let any client-reachable module import, so the guarantee is a
 * build error rather than a promise — the same shape `$lib/blocks/maths` uses for KaTeX.
 * See [ADR 0016](../../../../docs/decisions/0016-the-syntax-highlighter-s-regex-engine.md).
 */

/** One run of text that shares a colour. */
export interface CodeToken {
  /** The characters, exactly as the author typed them. */
  text: string;
  /**
   * The light theme's colour as a hex literal, or `null` for a run that has none —
   * which is what a line break is.
   *
   * Validated as a hex literal on the way out of the theme rather than trusted. It is
   * bound through Svelte's `style:` directive, so it becomes a `style="…"` attribute, and
   * ADR 0007 admits those on the stated grounds that *"the renderer does not emit authored
   * CSS into one either way"*. A theme is ours and a fence's text is not, but the check
   * costs one regex and makes that sentence true of the code rather than of the intent.
   */
  light: string | null;
  /**
   * The dark theme's colour, carried alongside rather than chosen between.
   *
   * The reader may have picked a theme (`[data-theme]`) or be following the system
   * (`prefers-color-scheme`), and server rendering knows neither. Both colours ride along
   * as custom properties and `CodeView.svelte`'s stylesheet picks — the same shape
   * `tokens.css` already uses for every other colour in the application.
   */
  dark: string | null;
}

/**
 * What the reader should draw for one fence.
 *
 * `note` is the discreet line under the block (D-25). It names an unrecognised language,
 * or says why a block that could have been highlighted was not. `null` means the block
 * has nothing to explain — either it highlighted, or the author asked for no language at
 * all and the page has no business commenting on that.
 */
export type Fence =
  | { kind: 'plain'; note: string | null }
  | { kind: 'highlighted'; tokens: CodeToken[] };

/**
 * Every fence on one page, by the language and text that produced it.
 *
 * **A `Map`, not a plain object, and that is a bug fix rather than a style** — the same one
 * `Formulas` records. The key is built from text anyone with write access to the page
 * typed, and this crosses the wire as page data, which SvelteKit serialises with `devalue`,
 * whose parser THROWS on an object carrying a `__proto__` property. A `Map` has no such key,
 * devalue carries it natively, and `Map.get` cannot answer out of `Object.prototype` either.
 */
export type Fences = Map<string, Fence>;

/**
 * The one string both sides look a fence up by.
 *
 * The language belongs in it as well as the text: the same listing written as ` ```sql ` and
 * as ` ```text ` is two different answers, and one page can hold both.
 *
 * **Length-prefixed rather than joined by a separator**, because there is no character a
 * language cannot contain. Nothing between the collaboration socket and `documents.body`
 * validates a block attribute, so `{language: "rust "}` is storable, and under any separator
 * that fence keys identically to `{language: "rust"}` holding a text one character longer —
 * two different blocks sharing one entry, which would show one fence's tokens in place of
 * the other fence's text. A count cannot collide with the text that follows it.
 *
 * `language` is `unknown` for the same reason: anything that is not a string keys the same
 * as no language at all, which is also how the highlighter treats it.
 */
export function fenceKey(text: string, language: unknown): string {
  const named = typeof language === 'string' ? language : '';
  return `${named.length}:${named}${text}`;
}

/**
 * What the server made of this fence, or `null` if it made nothing of it.
 *
 * `null` covers two different situations on purpose, and the caller must not tell them
 * apart: nobody tokenised this page (the editor renders the same `BlockView` while TipTap
 * mounts, with no page load behind it), or this exact fence was not among the ones walked —
 * which is the case for the source of a formula or a diagram that could not be rendered,
 * since those are shown through `CodeView` as deliberately plain text. Both draw the fence
 * exactly as it was typed and say nothing about limits, because in neither case was a limit
 * reached.
 */
export function fenceFor(
  fences: Fences | null | undefined,
  text: string,
  language: unknown
): Fence | null {
  return fences?.get(fenceKey(text, language)) ?? null;
}
