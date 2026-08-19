import type { GraphEdge, GraphNode } from '$lib/api';

/**
 * A force-directed layout, written out rather than installed.
 *
 * **No graph library, deliberately.** d3-force, cytoscape and vis-network are all excellent
 * and all wrong here: this corpus is TENS of pages, the whole layout is the sixty lines
 * below, and the smallest of them costs more bundle than the entire graph route. A library
 * earns its place when the problem outgrows the loop — thousands of nodes, incremental
 * re-layout, clustering — and this problem has not, so adding one now would be paying a
 * permanent cost for a capability nothing has asked for. Revisit when the corpus is in the
 * thousands and this loop is visibly slow, which is a measurement, not a guess.
 *
 * **Deterministic, and that is a requirement rather than a nicety.** Every position below is
 * a function of the node order and the edges — there is no `Math.random()` anywhere — for
 * three reasons: the layout runs during server rendering, so a random one would produce
 * different markup on the server and on the client and hydration would complain; two
 * identical requests must not draw two different pictures; and a test can only assert a
 * position it can predict.
 *
 * The algorithm is Fruchterman–Reingold: every pair of nodes pushes apart, every edge pulls
 * together, and a falling temperature caps how far anything may move per step so the whole
 * thing settles instead of oscillating. It is O(n² · iterations) — 40 nodes is about 400 000
 * distance calculations, a few milliseconds, once per request.
 */

export interface Placed extends GraphNode {
  x: number;
  y: number;
}

/**
 * A stable, collision-free key for an edge, for the graph route's keyed `{#each}` blocks.
 *
 * `edge.from + '' + edge.to` looks harmless and is not: concatenating two slash-prefixed
 * paths with no separator is ambiguous. `/x -> /y/z` and `/x/y -> /z` both produce
 * `"/x/y/z"`. SSR does not notice — it renders whatever list it is given regardless of
 * whether two items share a key — but Svelte's CLIENT `each` throws `each_key_duplicate` on
 * a duplicate key, in dev AND production, which fails hydration for the whole page. Nothing
 * about that shows up in a server-rendered test, which is why this is a named, independently
 * tested function rather than an inline expression: `JSON.stringify` quotes each path, so no
 * choice of `from`/`to` can make two distinct edges collide.
 */
export function edgeKey(edge: { from: string; to: string }): string {
  return JSON.stringify([edge.from, edge.to]);
}

/** The drawn radius of a node, in the same units as the layout. */
export const NODE_RADIUS = 8;

/**
 * Where an edge's line should start and stop.
 *
 * Trimmed at both ends by the node radius (plus a little for the arrowhead), so the line
 * meets the edge of the circle rather than running under it: an arrowhead hidden beneath the
 * target node makes a directed graph look undirected, which is the one thing this diagram is
 * for. Returns `null` for two nodes closer together than the trim, where there is no line
 * left to draw and the trim would otherwise flip it end for end.
 */
export function edgeLine(
  from: { x: number; y: number },
  to: { x: number; y: number },
  headroom = 5
): { x1: number; y1: number; x2: number; y2: number } | null {
  const ox = to.x - from.x;
  const oy = to.y - from.y;
  const distance = Math.hypot(ox, oy);
  const start = NODE_RADIUS;
  const end = NODE_RADIUS + headroom;
  if (distance <= start + end) return null;
  const ux = ox / distance;
  const uy = oy / distance;
  const round = (n: number) => Math.round(n * 100) / 100;
  return {
    x1: round(from.x + ux * start),
    y1: round(from.y + uy * start),
    x2: round(to.x - ux * end),
    y2: round(to.y - uy * end)
  };
}

export interface LayoutResult {
  nodes: Placed[];
  width: number;
  height: number;
}

export interface LayoutOptions {
  width?: number;
  height?: number;
  iterations?: number;
  /** Kept clear at the edges so a label near the border is not clipped. */
  padding?: number;
}

const DEFAULTS = {
  width: 800,
  height: 560,
  iterations: 260,
  padding: 60
};

/** The golden angle, in radians. */
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5));

/**
 * Where each node starts, before any force is applied.
 *
 * A sunflower spiral rather than a circle. A circle is the obvious choice and it is a trap:
 * with every node the same distance from the centre and the same distance from its
 * neighbours, the repulsion forces cancel almost exactly, and a symmetric configuration that
 * cancels is a configuration the simulation cannot leave — the graph stays a ring no matter
 * how many iterations it is given. The spiral breaks that symmetry at the start, which is
 * cheaper and more predictable than breaking it later with noise.
 */
function seed(count: number, width: number, height: number): Array<{ x: number; y: number }> {
  const cx = width / 2;
  const cy = height / 2;
  const spread = Math.min(width, height) * 0.42;
  return Array.from({ length: count }, (_, i) => {
    const radius = spread * Math.sqrt((i + 0.5) / count);
    const angle = i * GOLDEN_ANGLE;
    return { x: cx + radius * Math.cos(angle), y: cy + radius * Math.sin(angle) };
  });
}

export function layout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: LayoutOptions = {}
): LayoutResult {
  const width = options.width ?? DEFAULTS.width;
  const height = options.height ?? DEFAULTS.height;
  const iterations = options.iterations ?? DEFAULTS.iterations;
  const padding = options.padding ?? DEFAULTS.padding;

  if (nodes.length === 0) return { nodes: [], width, height };

  const index = new Map(nodes.map((node, i) => [node.path, i]));
  const positions = seed(nodes.length, width, height);

  // The ideal edge length: spread the available area evenly over the nodes. One node has no
  // pair to be spaced from, so guard the division rather than letting `k` become Infinity.
  //
  // Capped, and the cap is not cosmetic. Fruchterman–Reingold's `sqrt(area / n)` assumes n
  // is large; for a handful of nodes it asks for an ideal edge length of most of the canvas,
  // and the simulation obliges by pinning every node to a different corner — measured with
  // three nodes, which came out at (60, 500), (740, 60) and the middle. A third of the
  // shorter side is a length that still fits inside the frame with its label.
  const k = Math.min(
    Math.sqrt((width * height) / Math.max(nodes.length, 2)),
    Math.min(width, height) / 3
  );
  const cx = width / 2;
  const cy = height / 2;
  // Only edges whose ends are both present can pull. The API guarantees they are — an edge
  // with one end missing is exactly what the permission filter refuses to emit — so this is
  // a guard against a malformed response, not against a normal case.
  const pulls = edges
    .map((edge) => [index.get(edge.from), index.get(edge.to)] as const)
    .filter((pair): pair is readonly [number, number] => pair[0] !== undefined && pair[1] !== undefined);

  let temperature = Math.min(width, height) / 6;
  const cooling = temperature / (iterations + 1);

  const dx = new Float64Array(nodes.length);
  const dy = new Float64Array(nodes.length);

  for (let step = 0; step < iterations; step += 1) {
    dx.fill(0);
    dy.fill(0);

    // Repulsion: every pair, once, applied to both ends.
    for (let i = 0; i < nodes.length; i += 1) {
      for (let j = i + 1; j < nodes.length; j += 1) {
        let ox = positions[i].x - positions[j].x;
        let oy = positions[i].y - positions[j].y;
        let distance = Math.hypot(ox, oy);
        if (distance < 0.01) {
          // Two nodes exactly on top of each other have no direction to push apart along.
          // Nudged by a function of the indices rather than at random, so the tie is broken
          // the same way on every run — see the note about determinism above.
          ox = ((i % 7) - 3) / 10 || 0.1;
          oy = ((j % 5) - 2) / 10 || 0.1;
          distance = Math.hypot(ox, oy);
        }
        const force = (k * k) / distance;
        const fx = (ox / distance) * force;
        const fy = (oy / distance) * force;
        dx[i] += fx;
        dy[i] += fy;
        dx[j] -= fx;
        dy[j] -= fy;
      }
    }

    // Attraction along each edge.
    for (const [a, b] of pulls) {
      const ox = positions[a].x - positions[b].x;
      const oy = positions[a].y - positions[b].y;
      const distance = Math.max(Math.hypot(ox, oy), 0.01);
      const force = (distance * distance) / k;
      const fx = (ox / distance) * force;
      const fy = (oy / distance) * force;
      dx[a] -= fx;
      dy[a] -= fy;
      dx[b] += fx;
      dy[b] += fy;
    }

    // A gentle pull towards the middle. Without it, repulsion alone drives everything into
    // the frame's corners and holds it there — and a graph in two disconnected halves has
    // nothing at all to stop the halves drifting apart for ever, since only edges pull.
    for (let i = 0; i < nodes.length; i += 1) {
      dx[i] += (cx - positions[i].x) * 0.12;
      dy[i] += (cy - positions[i].y) * 0.12;
    }

    for (let i = 0; i < nodes.length; i += 1) {
      const distance = Math.max(Math.hypot(dx[i], dy[i]), 0.01);
      const limit = Math.min(distance, temperature);
      positions[i].x += (dx[i] / distance) * limit;
      positions[i].y += (dy[i] / distance) * limit;
      // Inside the frame at all times, not merely at the end: a node allowed to fly far out
      // and be dragged back arrives with a distorted view of where its neighbours are.
      positions[i].x = Math.min(width - padding, Math.max(padding, positions[i].x));
      positions[i].y = Math.min(height - padding, Math.max(padding, positions[i].y));
    }

    temperature -= cooling;
  }

  return {
    nodes: nodes.map((node, i) => ({
      ...node,
      // Rounded, because the coordinates end up in markup: full doubles would put 17
      // significant figures into every attribute for a precision no screen has.
      x: Math.round(positions[i].x * 100) / 100,
      y: Math.round(positions[i].y * 100) / 100
    })),
    width,
    height
  };
}
