<!--
  The graph: the pages of this wiki and the links between them (D-4).

  This is the screen the owner asked for by name — the one Joplin could not give them. Nodes
  are pages, edges are links somebody deliberately wrote, and topics are not in it.

  **Everything here is already filtered.** `Store::graph_for` emits an edge only when the
  caller may read BOTH of its ends, and a node only when a surviving edge touches it, so
  there is nothing left for this component to hide and nothing it could disclose by hiding it
  wrongly. See `crates/gw-store/src/links.rs`.

  **Plain SVG and a hand-written force layout, no graph library** — the reasoning is in
  `$lib/graph/layout.ts` and it is about size: this corpus is tens of pages, the layout is
  sixty lines, and the smallest library that would replace it costs more bundle than the
  whole route.

  The diagram is laid out on the SERVER, so it arrives complete in the first response and
  works with JavaScript switched off. Nothing on this page needs a script at all: the nodes
  are ordinary links and the subtree filter is an ordinary GET form.
-->
<script lang="ts">
  import { edgeKey, edgeLine, layout, NODE_RADIUS } from '$lib/graph/layout';

  let { data } = $props();

  const placed = $derived(layout(data.graph.nodes, data.graph.edges));
  const at = $derived(new Map(placed.nodes.map((node) => [node.path, node])));
  const titles = $derived(new Map(data.graph.nodes.map((node) => [node.path, node.title])));

  /** Every edge that has a line left to draw once both ends are trimmed. */
  const lines = $derived(
    data.graph.edges
      .map((edge) => {
        const from = at.get(edge.from);
        const to = at.get(edge.to);
        return from && to ? { edge, line: edgeLine(from, to) } : null;
      })
      .filter((drawn) => drawn?.line != null)
  );
</script>

<svelte:head>
  <title>Graph — great-wiki</title>
</svelte:head>

<main id="content">
  <h1>Graph</h1>
  <p class="lede">
    Seiten und die Verweise zwischen ihnen. Es erscheint nur, was Sie auch lesen dürfen.
  </p>

  <!-- A GET form, so the subtree ends up in the address bar and needs no script. -->
  <form class="filter" method="get">
    <label for="wurzel">Auf einen Teilbaum einschränken</label>
    <input id="wurzel" name="wurzel" type="text" value={data.root ?? ''} placeholder="/darm" />
    <button type="submit">Anzeigen</button>
  </form>

  {#if data.error}
    <p class="notice">{data.error}</p>
  {:else if placed.nodes.length === 0}
    <!-- One message for "there are no links", for "none of them are yours" and — only when
         a root is given — for "they all leave this subtree", because the store deliberately
         answers the first two the same way: distinguishing them here would say that
         something is being withheld, which is the whole of what it was hiding. The third is
         different: `within_root` (crates/gw-store/src/links.rs) drops an edge whose far end
         sits outside the requested subtree, so a subtree whose pages link only outward
         renders this same empty graph even though it has real, readable links. "Noch keine
         Verweise unterhalb von X" would say those links do not exist, which is false — so
         the root case is worded to claim only that none STAY inside the subtree, which is
         true in all three cases at once. Without a root there is no "leaves the subtree" to
         be honest about, so that case keeps the plain, short wording. -->
    <p class="empty">
      {#if data.root}
        Noch keine Verweise, die innerhalb von {data.root} bleiben. Verweise nach außerhalb
        des Teilbaums werden hier nicht angezeigt.
      {:else}
        Noch keine Verweise. Sobald eine Seite auf eine andere verweist, erscheint die
        Verbindung hier.
      {/if}
    </p>
  {:else}
    <figure>
      <svg
        viewBox="0 0 {placed.width} {placed.height}"
        aria-label="Verweisgraph mit {placed.nodes.length} Seiten und {data.graph.edges.length} Verbindungen"
      >
        <defs>
          <!-- `context-stroke` so the head takes the line's colour, which is a token and
               therefore themeable — a hard-coded fill here would be the one mark on the page
               a plugin could not repaint. -->
          <marker
            id="gw-graph-pfeil"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" />
          </marker>
        </defs>

        <g class="edges">
          {#each lines as drawn (edgeKey(drawn!.edge))}
            <line
              x1={drawn!.line!.x1}
              y1={drawn!.line!.y1}
              x2={drawn!.line!.x2}
              y2={drawn!.line!.y2}
              marker-end="url(#gw-graph-pfeil)"
            />
          {/each}
        </g>

        <g class="nodes">
          {#each placed.nodes as node (node.path)}
            <a href={node.path}>
              <!-- The WHOLE title, always: `label.text` is shortened when the title is too
                   wide to draw (see `$lib/graph/labels.ts`), and this is both the accessible
                   name of the link and the tooltip a pointer gets, so nothing here may be the
                   short form. The twin list below keeps the whole title too. -->
              <title>{node.title}</title>
              <circle cx={node.x} cy={node.y} r={NODE_RADIUS} />
              <!-- Placed rather than assumed: the label used to be centred below the node
                   unconditionally, which at thirty-five pages drew forty-four pairs of labels
                   on top of each other. -->
              <text x={node.label.x} y={node.label.y} text-anchor={node.label.anchor}>
                {node.label.text}
              </text>
            </a>
          {/each}
        </g>
      </svg>
      <figcaption>
        {placed.nodes.length} Seiten, {data.graph.edges.length} Verweise. Ein Pfeil zeigt von der
        verweisenden auf die verwiesene Seite.
      </figcaption>
    </figure>

    <!--
      The twin ADR 0005 requires: the diagram conveys "these pages are connected" only to
      somebody who can see it. Plain text rather than links, deliberately — every page here
      is already a link inside the SVG, and repeating them would double the tab stops for
      the keyboard users this is meant to help.
    -->
    <ul class="twin">
      {#each data.graph.edges as edge (edgeKey(edge))}
        <li>{titles.get(edge.from) ?? edge.from} verweist auf {titles.get(edge.to) ?? edge.to}</li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  /* `@layer components`, the plugin contract (ADR 0005): a plugin's unlayered rules beat
     every rule here regardless of specificity, so a theme can repaint the graph without an
     `!important` anywhere.

     No `:not([hidden])` guards in this file, and that is on purpose rather than an omission.
     The guard in Dialog.svelte exists because Ark UI closes a component by setting the
     `hidden` attribute and relying on a user-agent rule any author `display` would beat.
     Nothing on this page is an Ark component, so adding the guard here would only raise
     specificity — which is exactly the trap that comment warns about. */
  @layer components {
    main {
      padding: var(--space-6);
    }

    h1 {
      font-size: var(--text-3xl);
      line-height: var(--leading-tight);
      margin-block-end: var(--space-2);
    }

    .lede {
      color: var(--ink-muted);
      margin-block-end: var(--space-6);
    }

    .filter {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
      margin-block-end: var(--space-6);
    }

    .filter label {
      font-size: var(--text-sm);
      color: var(--ink-muted);
    }

    .filter input {
      font: inherit;
      font-size: var(--text-sm);
      padding: var(--space-1) var(--space-2);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
      color: var(--ink);
    }

    .filter button {
      font: inherit;
      font-size: var(--text-sm);
      padding: var(--space-1) var(--space-3);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg-sunken);
      color: var(--ink);
      cursor: pointer;
    }

    .notice,
    .empty {
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--bg-sunken);
      padding: var(--space-4);
      color: var(--ink-muted);
      max-width: var(--measure);
    }

    /* THE ONE VIEW THAT IS NOT WIDENED, and it is deliberate. `$lib/graph/layout` fixes
       the viewBox at 1100 units precisely so that one unit is about one CSS pixel in a
       72rem column — that is what makes the 13px labels 13px on screen. Letting the
       drawing stretch to a 3440px monitor would scale every label with it and make the
       picture proportionally taller, which is not "more graph", it is the same graph
       further away. The heading, the lede and the filter above fill the view like every
       other one; the drawing keeps its scale and centres in the room. */
    figure {
      margin: 0;
      max-inline-size: 72rem;
      margin-inline: auto;
    }

    svg {
      inline-size: 100%;
      block-size: auto;
      background: var(--bg-sunken);
      border: 1px solid var(--border);
      border-radius: var(--radius);
    }

    .edges line {
      stroke: var(--border-strong);
      stroke-width: 1.5;
    }

    .nodes circle {
      fill: var(--accent);
      stroke: var(--bg-raised);
      stroke-width: 2;
    }

    .nodes text {
      fill: var(--ink);
      font-size: 13px;
      /* Not the reading face: labels sit in a diagram, not in prose, and at this size the
         serif loses more to the background than it gains. */
      font-family: var(--font-sans);
    }

    .nodes a:hover circle,
    .nodes a:focus-visible circle {
      fill: var(--ink);
    }

    .nodes a:hover text,
    .nodes a:focus-visible text {
      text-decoration: underline;
    }

    figcaption {
      margin-block-start: var(--space-3);
      color: var(--ink-faint);
      font-size: var(--text-sm);
    }

    /* Visually hidden, still announced. The standard clip rectangle rather than
       `display: none`, which would take it out of the accessibility tree as well. */
    .twin {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
      margin: 0;
      padding: 0;
      list-style: none;
    }
  }
</style>
