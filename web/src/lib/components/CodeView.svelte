<script lang="ts">
  /**
   * One fenced code block: the text exactly as it was typed, coloured if the language is
   * one this wiki knows.
   *
   * **Tokens, never markup.** `$lib/server/highlight` hands back runs of TEXT with two colours
   * each; every run is interpolated into a `<span>` and the colours are bound through
   * Svelte's `style:` directive, which server-renders as a `style="…"` ATTRIBUTE. So the
   * reader still constructs no HTML from stored content — the sentence
   * `BlockView.svelte` states and `scripts/check-html-sinks.sh` enforces with an empty
   * exemption list — and no CSP directive moves: `style-src-attr 'unsafe-inline'` already
   * pays for the attribute (ADR 0007), and `style-src 'self'` still refuses an injected
   * STYLE ELEMENT, which is what a highlighter shipping a stylesheet of its own would need.
   * (Spelled out rather than written as a tag: `svelte-check`'s parser splits the file on
   * the literal string, so one inside this comment leaves the script block "unclosed" —
   * two errors, neither of them naming a comment.)
   *
   * **Both themes ride along.** A colour is bound as a custom property rather than as
   * `color:` directly, because an inline `color` would beat every stylesheet rule and the
   * reader's theme is not known here: they may have chosen one (`[data-theme]`) or be
   * following their system (`prefers-color-scheme`). The two rules at the bottom of this
   * file are the same pair `tokens.css` uses for every other colour in the application,
   * and they are the only place that decides which of the two applies.
   */
  import { fenceFor, type Fence, type Fences } from '$lib/blocks/code';

  interface Props {
    /**
     * The fence's text, exactly as typed — `codeText(block)`, never `plainText`. Every
     * newline and every space of indentation is content here.
     */
    text: string;
    /**
     * The info string's first word, as the importer stored it — or anything at all, since
     * nothing between the collab socket and `documents.body` validates a block attribute.
     * It is half of the key this fence is looked up under and a string printed as text, and
     * nothing else: never a class name, never an `import()` specifier, never a `style:`
     * value.
     */
    language?: unknown;
    /**
     * Every fence the page's `load` tokenised, or `null` where nothing tokenised this page.
     *
     * **Handed down rather than computed here, and that is what keeps Shiki out of the
     * reader's browser.** This component renders on the server and AGAIN while it hydrates,
     * so calling the highlighter from it put the library and its eight grammars — 609 kB
     * raw — into every page's bundle and re-ran the tokeniser in the reader's own tab to
     * re-derive what they had already been sent. `$lib/server/highlight` cannot be imported
     * from anywhere a browser can reach, so the guarantee is a build error rather than a
     * promise, and the page's caps apply to the page rather than to one block.
     *
     * `null` draws the fence exactly as it was typed and says nothing: the editor renders
     * this component while TipTap mounts, and being refused and never having been asked are
     * different states that must not look alike. `MathView` and `DiagramView` show a
     * formula's or a diagram's own source through here for the same reason.
     */
    fences?: Fences | null;
  }

  let { text, language, fences = null }: Props = $props();

  /** A page nobody tokenised, and a fence nobody walked: printed as typed, said nothing about. */
  const UNGEFRAGT: Fence = { kind: 'plain', note: null };

  const fence = $derived(fenceFor(fences, text, language) ?? UNGEFRAGT);
</script>

<div class="quellcode">
  {#if fence.kind === 'highlighted'}
    <!-- One `{#each}` over a flat run of tokens, with the line breaks already in it as
         tokens of their own. The tags are broken across lines at the `>` rather than
         around it: `<pre>` preserves whitespace, and Svelte preserves the template's
         whitespace inside one, so a newline after `<code>` would be a newline in the
         listing. -->
    <pre><code
        >{#each fence.tokens as token, index (index)}<span
            style:--token-hell={token.light}
            style:--token-dunkel={token.dark}>{token.text}</span
          >{/each}</code
      ></pre>
  {:else}
    <pre><code>{text}</code></pre>
  {/if}
  {#if fence.kind === 'plain' && fence.note !== null}
    <!-- D-25: a fence whose language this wiki does not know renders plain and SAYS so.
         An author who writes ```kotlin and sees no colour cannot otherwise tell whether
         the wiki does not know Kotlin, whether they misspelled it, or whether
         highlighting is broken — and a fence that states no language at all gets nothing,
         because the author said nothing and the page should not argue with them.

         Under the block rather than over it or inside it: `.prose pre` is a horizontal
         scroll region, so a note inside would scroll away from a wide listing, and an
         absolutely positioned one would sit on top of the first line of code on a phone. -->
    <p class="quellcode-hinweis">{fence.note}</p>
  {/if}
</div>

<style>
  /* The wrapper carries the block's own spacing so that the note belongs to the listing
     rather than reading as the next paragraph. `.prose > * + *` in app.css applies to
     this element now instead of to the `<pre>`, so the `<pre>`'s own default margins are
     dropped — otherwise the fence would gain a line of air above and below it. */
  .quellcode pre {
    margin: 0;
  }

  /* The light theme's colour is the default because `:root` is the light theme, and an
     unset custom property leaves `color` to inherit — which is the right answer for a
     token that has no colour of its own, such as a line break. */
  .quellcode code span {
    color: var(--token-hell);
  }

  /* Dark by system preference, and dark by explicit choice: two rules, because a media
     query and an attribute selector cannot be combined into one that means "either".
     tokens.css carries the same duplication for the same reason. */
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .quellcode code span {
      color: var(--token-dunkel);
    }
  }

  :global(:root[data-theme='dark']) .quellcode code span {
    color: var(--token-dunkel);
  }

  /* Discreet: the smallest type in the scale, the faintest ink, and out of the way at the
     end of the line. It is an aside about the block, not a warning about the page. */
  .quellcode-hinweis {
    margin-block: var(--space-1) 0;
    text-align: end;
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }
</style>
