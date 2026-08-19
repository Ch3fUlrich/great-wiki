import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { edgeKey, layout } from '$lib/graph/layout';
import { ANONYMOUS, type Graph } from '$lib/api';

/**
 * The graph screen, rendered exactly as the server renders it.
 *
 * There is no DOM environment in this project, so `render()` from `svelte/server` is both
 * the first response and the only thing there is — which suits this page, because the whole
 * point of laying the graph out on the server is that the picture arrives complete in the
 * first response rather than being drawn after hydration. A reader with JavaScript switched
 * off gets the same diagram.
 *
 * The filtering itself is NOT tested here and must not be: it belongs to `Store::graph_for`,
 * is mutation-tested there, and re-asserting it against a hand-written fixture in the
 * browser would only prove that this file can build a graph with nothing secret in it.
 */
function html(graph: Graph, extra: { root?: string | null; error?: string | null } = {}): string {
  return render(Page, {
    // `me` comes from the root layout's load and this page does not read it, but it is part
    // of `PageData` and the type says so.
    props: {
      data: { me: ANONYMOUS, graph, root: extra.root ?? null, error: extra.error ?? null }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

const graph: Graph = {
  nodes: [
    { path: '/rundgang', title: 'Rundgang' },
    { path: '/rundgang/tabellen', title: 'Tabellen' },
    { path: '/rundgang/import-export', title: 'Import und Export' }
  ],
  edges: [
    { from: '/rundgang', to: '/rundgang/tabellen' },
    { from: '/rundgang', to: '/rundgang/import-export' }
  ]
};

const empty: Graph = { nodes: [], edges: [] };

describe('the graph page', () => {
  it('draws one line per edge and one node per page, in the first response', () => {
    const out = html(graph);
    expect(out.match(/<line /g)).toHaveLength(graph.edges.length);
    expect(out.match(/<circle /g)).toHaveLength(graph.nodes.length);
    for (const node of graph.nodes) {
      expect(out).toContain(node.title);
    }
  });

  it('makes every node a link to its page, so the graph is navigable without a script', () => {
    const out = html(graph);
    for (const node of graph.nodes) {
      expect(out).toContain(`href="${node.path}"`);
    }
    expect(out).not.toContain('onclick');
  });

  it('places every node inside the drawing area, with no NaN in the markup', () => {
    const out = html(graph);
    expect(out).not.toContain('NaN');
    const viewBox = out.match(/viewBox="0 0 (\d+) (\d+)"/);
    expect(viewBox).not.toBeNull();
    const [width, height] = [Number(viewBox![1]), Number(viewBox![2])];
    const centres = [...out.matchAll(/<circle [^>]*cx="([-\d.]+)" cy="([-\d.]+)"/g)];
    expect(centres).toHaveLength(graph.nodes.length);
    for (const [, cx, cy] of centres) {
      expect(Number(cx)).toBeGreaterThanOrEqual(0);
      expect(Number(cx)).toBeLessThanOrEqual(width);
      expect(Number(cy)).toBeGreaterThanOrEqual(0);
      expect(Number(cy)).toBeLessThanOrEqual(height);
    }
  });

  it('renders identically twice, so hydration has nothing to disagree with', () => {
    // The layout runs on the server. A random one would place the nodes somewhere else on
    // the client and Svelte would report a hydration mismatch — which is why there is no
    // `Math.random()` in `$lib/graph/layout`.
    expect(html(graph)).toBe(html(graph));
  });

  it('says so in German when there is nothing to draw, rather than showing an empty frame', () => {
    // Not an error, and not a blank canvas that reads as broken. It is also the SAME answer
    // for "this wiki has no links yet" and "none of the links are yours to see": the store
    // deliberately conflates those, and a message that distinguished them here would undo
    // it by saying that something is being withheld.
    const out = html(empty);
    expect(out).not.toContain('<svg');
    expect(out).toContain('Noch keine Verweise');
  });

  it('names the subtree in the empty message when one was asked for', () => {
    const out = html(empty, { root: '/darm' });
    expect(out).toContain('/darm');
    expect(out).toContain('Noch keine Verweise');
  });

  it('does not claim there are no links at all when a subtree only links outward', () => {
    // `Store::graph_for` drops any edge with an end outside the requested root — see
    // `within_root` in crates/gw-store/src/links.rs — so a subtree whose pages all link
    // OUT of it renders exactly this empty graph, even though those pages have real,
    // readable links. The old wording, "Noch keine Verweise unterhalb von /darm", reads as
    // "nothing here links to anything", which is false in that case: they link, they just
    // leave. The page cannot tell apart the three things that produce an empty graph here
    // (nothing exists, nothing is readable, everything leaves) — same as it already could
    // not tell apart the first two — so the wording must be honest for all three at once:
    // it may only claim that no link STAYS inside the subtree, never that none exist.
    // Matched with a collapsed-whitespace regex rather than toContain: the source template
    // wraps this sentence across lines for readability, and the raw SSR markup this test
    // reads keeps that literal whitespace rather than collapsing it the way a browser would.
    const out = html(empty, { root: '/darm' }).replace(/\s+/g, ' ');
    expect(out).toContain('die innerhalb von /darm bleiben');
    expect(out).toContain('Verweise nach außerhalb des Teilbaums werden hier nicht angezeigt');
  });

  it('says the graph could not be loaded rather than pretending it is empty', () => {
    // A dead API and an empty graph are different facts, and "there are no links" is a lie
    // about the first. The admin console makes the same distinction for the same reason.
    const out = html(empty, { error: 'Der Graph konnte nicht geladen werden (503).' });
    expect(out).toContain('Der Graph konnte nicht geladen werden');
    expect(out).not.toContain('Noch keine Verweise');
  });

  it('carries a text twin naming both ends of every edge', () => {
    // ADR 0005: every chart needs a twin that does not depend on seeing it. The SVG conveys
    // "these pages are connected" only to somebody who can look at it.
    const out = html(graph);
    expect(out).toContain('Rundgang verweist auf Tabellen');
    expect(out).toContain('Rundgang verweist auf Import und Export');
  });

  it('draws both edges of a pair whose bare-concatenation key would collide', () => {
    // `/x -> /y/z` and `/x/y -> /z` both stringify to "/x/y/z" under `from + '' + to`. SSR
    // does not care about keyed-each collisions — this only documents that both edges are
    // still in the markup once the key stops colliding; `edgeKey` below is what actually
    // discriminates the fix, since Svelte's client `each` throws `each_key_duplicate` on
    // hydration and that failure never shows up in a server render.
    const ambiguous: Graph = {
      nodes: [
        { path: '/x', title: 'X' },
        { path: '/x/y', title: 'X Y' },
        { path: '/y/z', title: 'Y Z' },
        { path: '/z', title: 'Z' }
      ],
      edges: [
        { from: '/x', to: '/y/z' },
        { from: '/x/y', to: '/z' }
      ]
    };
    const out = html(ambiguous);
    expect(out.match(/<line /g)).toHaveLength(2);
    expect(out).toContain('X verweist auf Y Z');
    expect(out).toContain('X Y verweist auf Z');
  });
});

describe('the accessible edge count', () => {
  it('states the true edge count in aria-label even when close nodes trim a line from the drawing', () => {
    // `edgeLine` returns null — no line left to draw — for two nodes closer together than
    // its trim distance (21 units). A chain of 35 nodes is a deterministic, reproducible
    // fixture (found by running the pure `layout`/`edgeLine` functions directly) where the
    // force layout happens to land node 33 and node 34 inside that trim, so `lines` drops
    // one entry: 33 <line> elements for 34 real edges. The edge still exists — it is a real
    // link between two real pages, just an unlucky one in this particular layout — so
    // `aria-label` must report `data.graph.edges.length` (34), matching the sighted
    // `<figcaption>`, not `lines.length` (33), which would tell a screen-reader user this
    // graph has one fewer connection than the caption right next to it says.
    const n = 35;
    const nodes = Array.from({ length: n }, (_, i) => ({ path: `/n${i}`, title: `N${i}` }));
    const edges = Array.from({ length: n - 1 }, (_, i) => ({ from: `/n${i}`, to: `/n${i + 1}` }));
    const chain: Graph = { nodes, edges };

    const out = html(chain);
    const drawnLines = out.match(/<line /g)?.length ?? 0;
    // Sanity check on the fixture itself: if the layout algorithm ever changes and this no
    // longer trims a line, this test proves nothing and must be replaced with one that does.
    expect(drawnLines).toBeLessThan(edges.length);

    const label = out.match(/aria-label="([^"]*)"/)?.[1] ?? '';
    expect(label).toContain(`${edges.length} Verbindungen`);
    expect(out).toContain(`${edges.length} Verweise`);
  });
});

describe('edgeKey', () => {
  it('gives two edges distinct keys even when a bare "from + to" concatenation would collide', () => {
    // The graph route keys its keyed {#each} blocks by this function. A bare
    // `edge.from + '' + edge.to` concatenation is ambiguous with no separator: these two
    // edges both produce "/x/y/z". Svelte's client `each` throws `each_key_duplicate` on a
    // duplicate key, which fails hydration for the whole page — silently, since SSR itself
    // renders fine either way.
    const a = { from: '/x', to: '/y/z' };
    const b = { from: '/x/y', to: '/z' };
    expect(edgeKey(a)).not.toBe(edgeKey(b));
  });
});

describe('the layout', () => {
  it('is a pure function of its input', () => {
    const once = layout(graph.nodes, graph.edges);
    const twice = layout(graph.nodes, graph.edges);
    expect(once.nodes).toEqual(twice.nodes);
  });

  it('separates two nodes that would otherwise sit on top of each other', () => {
    const pair = [
      { path: '/a', title: 'A' },
      { path: '/b', title: 'B' }
    ];
    const { nodes } = layout(pair, [{ from: '/a', to: '/b' }]);
    expect(Math.hypot(nodes[0].x - nodes[1].x, nodes[0].y - nodes[1].y)).toBeGreaterThan(10);
  });

  it('places a single node without dividing by zero', () => {
    const { nodes } = layout([{ path: '/allein', title: 'Allein' }], []);
    expect(Number.isFinite(nodes[0].x)).toBe(true);
    expect(Number.isFinite(nodes[0].y)).toBe(true);
  });

  it('lays out an empty graph as nothing at all', () => {
    expect(layout([], []).nodes).toEqual([]);
  });
});
