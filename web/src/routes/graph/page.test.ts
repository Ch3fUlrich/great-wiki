import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { layout } from '$lib/graph/layout';
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
