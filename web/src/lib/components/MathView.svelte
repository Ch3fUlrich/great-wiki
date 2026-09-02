<script lang="ts">
  /**
   * One ` ```math ` fence: the formula KaTeX typeset for it, or the author's own source.
   *
   * **THE ONE PLACE IN THIS READER WHERE A STRING BECOMES MARKUP.** Everywhere else,
   * `BlockView` renders a document by matching on block kind and skips a kind it does not
   * know, which is why nothing in the reader sanitises anything and why
   * `scripts/check-html-sinks.sh` can hold an EMPTY exemption list. That script permits
   * exactly one construction — Svelte's raw-HTML tag applied to `formel.html`, on a line of
   * its own, in this file — and still refuses any other sink here, the same line in any
   * other file, and any other expression on it. Read its `PERMITTED` block for why that is
   * a line rather than a file. (Spelled out rather than written: the check greps this
   * directory, and a comment quoting the tag would be a finding.)
   *
   * # What makes the input safe
   *
   * Three things, in this order:
   *
   * 1. **It is not stored content.** `formel.html` is KaTeX's own output, built by
   *    `$lib/server/maths` from the fence's text. The author's text reaches the page only
   *    through KaTeX's escaping — as maths, and inside the `<annotation>` element a screen
   *    reader reads — which `maths.test.ts` asserts rather than assumes.
   * 2. **KaTeX's `trust` option is never passed**, so `\href`, `\url`, `\includegraphics`
   *    and the whole `\html…` family are refused and render as their own names in red. That
   *    default is the entire safety story of the typesetter, and a test reads the module's
   *    source to check that it never starts being configured.
   * 3. **The type says where it came from.** A `Formula` is produced in one module, which
   *    lives under `$lib/server/` — SvelteKit refuses to let any client-reachable module
   *    import that, so there is no second producer to audit.
   *
   * # Why the typesetting is not done here
   *
   * A component renders on the server and again in the browser while it hydrates, so
   * calling KaTeX here would put about 272 kB of library into every reader's bundle to
   * re-derive markup they were already sent. The page's `load` does it instead
   * (`[...path]/+page.server.ts`), and hands the answer down as `formeln` — the same way
   * `anhaenge` is handed down, and for one of the same reasons: a component that went
   * looking for the answer itself would be a second path to something already decided.
   *
   * `formeln` is `null` wherever nobody typeset the page — the editor renders this same
   * `BlockView` while TipTap mounts. That draws the source and says NOTHING, because being
   * refused and never being asked are different states and must not look alike.
   */
  import { formulaFor, type Formulas } from '$lib/blocks/maths';
  import CodeView from './CodeView.svelte';
  // KaTeX's own layout rules and its twenty self-hosted faces, vendored (see the header of
  // that file). Imported HERE rather than from app.css — the pattern `TableView.css`
  // already follows — so that thirty kilobytes of maths CSS is fetched with the route that
  // can hold a formula and not with every page in the wiki. A face is only fetched when a
  // rule naming it matches something, so a page without maths downloads none of them.
  import '$lib/styles/katex.css';

  interface Props {
    /**
     * The fence's text, exactly as typed — `codeText(block)`, never `plainText`. It is also
     * the key this formula was recorded under, so the two must be the same string.
     */
    source: string;
    /** Every formula the page's `load` typeset, or `null` where nothing typeset this page. */
    formeln?: Formulas | null;
  }

  let { source, formeln = null }: Props = $props();

  const formel = $derived(formulaFor(formeln, source));
</script>

{#if formel?.kind === 'typeset'}
  <!-- `overflow-x` on the wrapper rather than on `.katex-display`: a long equation must
       scroll inside its own box instead of widening the page on a phone, and the box has to
       be OUTSIDE the element KaTeX centres, or the centring is computed against a width the
       reader cannot see. -->
  <div class="formel">
    {@html formel.html}
  </div>
{:else}
  <!-- Not typeset: the fence renders exactly as a fence does, through the same component
       every other code block goes through, so the escaping and the `<pre>` are the ones
       that are already tested. `text` rather than the fence's own `math`, because
       `CodeView` would otherwise answer "Unbekannte Sprache: math" — which would be a
       different and untrue explanation sitting under this one.

       No scroll box of its own here: that `<pre>` is already one (`.prose pre` in app.css),
       and a scroll region inside a scroll region is two things to drag, one of them always
       the wrong one. -->
  <div class="formel-quelle">
    <CodeView text={source} language="text" />
    {#if formel !== null}
      <!-- Which limit stopped it, and why there is a limit at all. Under the block, in the
           same place and the same voice as `CodeView`'s own note: a formula that was not
           set is a state of one block, not a warning about the page. -->
      <p class="formel-hinweis">{formel.note}</p>
    {/if}
  </div>
{/if}

<style>
  /* A long equation scrolls inside its own box rather than widening the page — the failure
     a phone shows first. Nothing sets a margin here: `.prose > * + *` in app.css spaces
     this element like every other block, and this file's rules are UNLAYERED, so anything
     stated here would beat that silently. */
  .formel {
    overflow-x: auto;
    /* `overflow-y` computes to `auto` beside an `auto` on the other axis, so the padding is
       what keeps a tall integral or a matrix from being clipped by its own scroll box. */
    padding-block: var(--space-1);
  }

  /* KaTeX gives `.katex-display` margins of its own; with `.prose > * + *` already spacing
     the wrapper, keeping them would put a line of air above and below every formula. */
  .formel :global(.katex-display) {
    margin-block: 0;
  }

  /* Discreet, and identical to `CodeView`'s note: smallest type in the scale, faintest ink,
     out of the way at the end of the line. It is an aside about the block. */
  .formel-hinweis {
    margin-block: var(--space-1) 0;
    text-align: end;
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }
</style>
