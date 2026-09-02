<script lang="ts">
  /**
   * One ` ```mermaid ` fence: the diagram, drawn as a picture — or the author's own source.
   *
   * **The rendered diagram never becomes markup in this page.** It arrives as a string,
   * percent-encoded into a `data:image/svg+xml` URI and set as an `<img src>` — an attribute,
   * not markup, so `scripts/check-html-sinks.sh` has nothing to find here and this reader still
   * constructs no HTML from stored content. That is the containment
   * [ADR 0014](../../../../docs/decisions/0014-what-a-file-has-to-be-to-be-attached.md) already
   * requires for an uploaded SVG, and its reasoning is about the mechanism rather than about
   * where the bytes came from: generated SVG is not exempt because we generated it, since every
   * byte of it is a function of text somebody with write access to one page typed.
   * `$lib/blocks/mermaid` documents what the library does to `document.body` while it draws, and
   * which barrier holds during that window. See
   * [ADR 0018](../../../../docs/decisions/0018-how-a-diagram-reaches-the-page.md).
   *
   * # Three states, and only one of them is a picture
   *
   * 1. **Not drawn yet** — the whole of the first response, and permanently for a reader with
   *    JavaScript switched off. The fence's own source, through `CodeView`, saying nothing:
   *    being refused and never having been asked are different states and must not look alike.
   *    (`MathView` makes the same distinction for a page nobody typeset.)
   * 2. **Refused** — the source, plus one German line naming the limit or the failure. A
   *    diagram that is too big is refused before the library is fetched at all, so that line is
   *    in the first response too.
   * 3. **Drawn** — two `<img>`, one per theme.
   *
   * Malformed source is state 2 and never a broken image: `drawDiagram` catches everything and
   * `diagramDataUri` refuses to make an address out of anything that is not an SVG.
   *
   * # Two images, because one picture cannot follow a theme (D-24)
   *
   * This wiki has a light/dark control, and an `<img>` inherits neither it nor
   * `prefers-color-scheme`. So the diagram is drawn twice, both are put in the markup, and the
   * rules at the bottom of this file show whichever matches — the same pair `tokens.css` uses
   * for every other colour in the application, and the same one `CodeView` uses for a token.
   *
   * **The cost, stated where somebody changing this will find it: every diagram is drawn twice
   * and carried twice.** Two renders in the reader's tab (serialised, so they do not compete),
   * and both data URIs in the DOM, of which one is never painted. It is paid once per page load
   * on a wiki of tens of pages, and it buys a diagram that is never wrong against the
   * background it is read on. Rejected: one neutral look, which would read as deliberately
   * plain rather than wrong, and re-drawing on every theme change, which trades a fixed cost
   * for a flash of the old picture exactly when somebody is looking at it.
   */
  import { diagramRefusal, istUeberbreit, type Diagram } from '$lib/blocks/diagram';
  import { drawDiagram } from '$lib/blocks/mermaid';
  import CodeView from './CodeView.svelte';

  interface Props {
    /**
     * The fence's text, exactly as typed — `codeText(block)`, never `plainText`. The newlines
     * are not formatting here, they are the whole of the syntax: `graph TD; A-->B;` on one line
     * parses as nothing.
     */
    source: string;
  }

  let { source }: Props = $props();

  /**
   * Too big to draw, decided from the text alone — so the answer is server-rendered and a
   * reader with no JavaScript is told as much as one with it.
   */
  const zuGross = $derived(diagramRefusal(source));

  /** What the browser made of it, or `null` while nothing has been asked yet. */
  let gezeichnet = $state<Diagram | null>(null);

  /**
   * Draw it, in the browser, after the page is already readable.
   *
   * An effect rather than a `load`: mermaid needs the DOM to measure text, so it cannot run on
   * the server at all, and an effect does not run there. That is also what makes the library a
   * fetch this page pays for only if it holds a diagram — see `$lib/blocks/mermaid` for why the
   * `browser` guard around the import is what keeps it out of the server bundle.
   */
  $effect(() => {
    const text = source;
    // Reset first: this component is reused across a client-side navigation, and showing the
    // previous page's picture under this page's source is worse than showing nothing.
    gezeichnet = null;
    if (diagramRefusal(text) !== null) return;

    let abgebrochen = false;
    void drawDiagram(text).then((diagramm) => {
      if (!abgebrochen) gezeichnet = diagramm;
    });
    return () => {
      abgebrochen = true;
    };
  });

  /** The size refusal outranks anything drawn, because nothing was drawn. */
  const diagramm = $derived<Diagram | null>(
    zuGross === null ? gezeichnet : { kind: 'source', note: zuGross }
  );

  /**
   * A drawing so wide that `max-width: 100%` would flatten it into a line.
   *
   * Measured: 750 class relations lay out to a viewBox of 63 604 × 306, which shrinks into a
   * 700-pixel column as about 700 × 3 CSS pixels — grey, and unreadable. Past
   * `DIAGRAM_ASPECT_LIMIT` the picture keeps its own size and is scrolled inside the box
   * this wrapper already is, which is legible by dragging.
   */
  const ueberbreit = $derived(
    diagramm?.kind === 'drawn' && istUeberbreit(diagramm.groesse)
  );
</script>

{#if diagramm?.kind === 'drawn'}
  <!-- `alt` is the diagram's own source on both. It is the only description anybody wrote, it
       is what a screen reader is given, and it is what the browser's find would otherwise have
       nothing to match — text inside an `<img>` is neither selectable nor searchable, which is
       the second stated cost of drawing a picture instead of markup.

       The hidden one is `display: none`, so a screen reader passes over it rather than reading
       the same diagram twice.

       `width` and `height` come from the drawing's own `viewBox` (see `diagramSize`), because
       mermaid writes `width="100%"` for a document context and inside an image that means no
       intrinsic width at all — the browser would stretch a three-node diagram across the whole
       column. They are omitted when the drawing did not say, which is the browser's default
       behaviour and not a guess. -->
  <div class="diagramm" class:ueberbreit>
    <img
      class="hell"
      src={diagramm.hell}
      alt={source}
      width={diagramm.groesse?.breite}
      height={diagramm.groesse?.hoehe}
    />
    <img
      class="dunkel"
      src={diagramm.dunkel}
      alt={source}
      width={diagramm.groesse?.breite}
      height={diagramm.groesse?.hoehe}
    />
  </div>
{:else}
  <!-- Not drawn: the fence renders exactly as a fence does, through the same component every
       other code block goes through, so the escaping and the `<pre>` are the ones that are
       already tested. `text` rather than the fence's own `mermaid`, because `CodeView` would
       otherwise answer "Unbekannte Sprache: mermaid" — a different and untrue explanation
       sitting under this one.

       No scroll box of its own: that `<pre>` is already one (`.prose pre` in app.css), and a
       scroll region inside a scroll region is two things to drag, one of them always wrong. -->
  <div class="diagramm-quelle">
    <CodeView text={source} language="text" />
    {#if diagramm !== null}
      <!-- Which limit stopped it, in the same place and the same voice as `CodeView`'s own
           note: a diagram that was not drawn is a state of one block, not a warning about the
           page. -->
      <p class="diagramm-hinweis">{diagramm.note}</p>
    {/if}
  </div>
{/if}

<style>
  /* A wide diagram scrolls inside its own box rather than widening the page — the failure a
     phone shows first, and the one the behaviour harness checks at 390px. Nothing sets a
     margin here: `.prose > * + *` in app.css spaces this element like every other block. */
  .diagramm {
    overflow-x: auto;
  }

  /* An SVG carries its own intrinsic size, which on a phone is regularly wider than the
     column. `height: auto` keeps the aspect ratio while `max-width` takes the width down. */
  .diagramm img {
    max-width: 100%;
    height: auto;
  }

  /* …except where taking the width down would take the height with it. A drawing many times
     wider than it is tall becomes a line at column width, so it keeps its own size and is
     dragged instead — the wrapper is already a scroll region. See `istUeberbreit`. */
  .diagramm.ueberbreit img {
    max-width: none;
  }

  /* Light is the default because `:root` is the light theme. Dark is asked for twice — by
     system preference and by explicit choice — because a media query and an attribute
     selector cannot be combined into one rule that means "either". `tokens.css` carries the
     same duplication for the same reason. */
  .diagramm .dunkel {
    display: none;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .diagramm .hell {
      display: none;
    }

    :global(:root:not([data-theme='light'])) .diagramm .dunkel {
      display: inline;
    }
  }

  :global(:root[data-theme='dark']) .diagramm .hell {
    display: none;
  }

  :global(:root[data-theme='dark']) .diagramm .dunkel {
    display: inline;
  }

  /* Discreet, and identical to `CodeView`'s and `MathView`'s notes: smallest type in the
     scale, faintest ink, out of the way at the end of the line. */
  .diagramm-hinweis {
    margin-block: var(--space-1) 0;
    text-align: end;
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }
</style>
