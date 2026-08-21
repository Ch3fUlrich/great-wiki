import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  estimateTextWidth,
  fitLabel,
  GAP,
  LABEL_ASCENT,
  LABEL_FONT_SIZE,
  LABEL_HEIGHT,
  MAX_LABEL_WIDTH,
  overlaps,
  placeLabels,
  touchesCircle,
  type Box
} from './labels';
import { frameHeight, layout, NODE_RADIUS, type Placed } from './layout';
import { CORPUS } from './corpus.fixture';
import type { GraphNode } from '$lib/api';

/**
 * How the label boxes are counted, and why the count is an estimate.
 *
 * There is no DOM in this project and there could not usefully be one here anyway: `layout()`
 * runs during server rendering, where `getComputedTextLength()` does not exist and no font
 * has been loaded. So a label's width is estimated from its character count — see
 * `estimateTextWidth` and the constant it uses — and every number in this file inherits that
 * approximation. The approximation is deliberately on the WIDE side, so a graph that counts
 * clean here is at least as clean when a real font draws it.
 */
function boxAt(node: { title: string; x: number; y: number }, baselineFromCentre: number): Box {
  const width = estimateTextWidth(node.title);
  return {
    x: node.x - width / 2,
    y: node.y + baselineFromCentre - LABEL_ASCENT,
    width,
    height: LABEL_HEIGHT
  };
}

/** Every pair of boxes that share any area. */
function overlappingPairs(boxes: Box[]): number {
  let count = 0;
  for (let i = 0; i < boxes.length; i += 1) {
    for (let j = i + 1; j < boxes.length; j += 1) {
      if (overlaps(boxes[i], boxes[j])) count += 1;
    }
  }
  return count;
}

/**
 * A graph of `n` pages carrying the real corpus's titles, for the sizes either side of it.
 *
 * `variant` re-deals which title lands on which node and re-shapes the tree, because a
 * placement that reaches zero overlaps on exactly one arrangement of exactly one fixture has
 * proved that it can be lucky. Every variant is a fixed function of its number: nothing here
 * is random, and a failure can be reproduced by its name alone.
 */
function sized(n: number, variant = 0): { nodes: GraphNode[]; edges: { from: string; to: string }[] } {
  const titles = CORPUS.nodes.map((node) => node.title);
  const nodes = Array.from({ length: n }, (_, i) => ({
    path: `/s${i}`,
    title: titles[(i * 7 + variant * 3) % titles.length]
  }));
  const edges = [
    ...Array.from({ length: n - 1 }, (_, i) => ({
      from: `/s${i + 1}`,
      to: `/s${Math.floor(i / (2 + (variant % 3)))}`
    })),
    ...Array.from({ length: Math.floor(n / 5) }, (_, i) => ({
      from: `/s${i * 5}`,
      to: `/s${(i * 5 + 7 + variant) % n}`
    }))
  ];
  return { nodes, edges };
}

const SIZES: Array<[string, () => { nodes: GraphNode[]; edges: { from: string; to: string }[] }]> = [
  ['10 pages', () => sized(10)],
  ['35 pages, the corpus this wiki is for', () => CORPUS],
  ['35 pages, dealt differently', () => sized(35, 1)],
  ['80 pages, for headroom', () => sized(80)],
  ['80 pages, dealt differently', () => sized(80, 2)]
];

describe('estimateTextWidth', () => {
  it('scales with the character count at the documented average advance', () => {
    expect(estimateTextWidth('')).toBe(0);
    expect(estimateTextWidth('abcdefghij')).toBeCloseTo(10 * 13 * 0.55, 5);
    expect(estimateTextWidth('abcdefghij'.repeat(2))).toBeCloseTo(
      2 * estimateTextWidth('abcdefghij'),
      5
    );
  });

  it('counts what a reader sees, not what a string holds', () => {
    // The corpus is German and full of non-ASCII characters — »Erhöhte«, »↑«, an em dash —
    // and every one of them is drawn as one character. `String.length` counts UTF-16 code
    // UNITS, so a character outside the basic plane counts twice and its label would be
    // estimated at double its width, shoving its neighbours aside to make room for nothing.
    expect(estimateTextWidth('Erhöhte')).toBeCloseTo(estimateTextWidth('Erhoehe'), 5);
    expect(estimateTextWidth('↑')).toBeCloseTo(estimateTextWidth('a'), 5);
    expect(estimateTextWidth('𝔊')).toBeCloseTo(estimateTextWidth('a'), 5);
  });
});

describe('fitLabel', () => {
  it('leaves a title that fits exactly as it was written', () => {
    for (const title of ['Rundgang', 'Import und Export', 'Table 3: SCFA Dysbalance']) {
      expect(fitLabel(title)).toBe(title);
    }
  });

  it('shortens a title too wide to draw, and says so with an ellipsis', () => {
    const long = CORPUS.nodes.find((node) => node.title.startsWith('Table 4'))!.title;
    const fitted = fitLabel(long);
    expect(fitted).not.toBe(long);
    expect(fitted.endsWith('…')).toBe(true);
    expect(estimateTextWidth(fitted)).toBeLessThanOrEqual(MAX_LABEL_WIDTH);
    // Still recognisably this page and not another: the part that survives is the front of
    // the title, which is where these titles differ from each other.
    expect(long.startsWith(fitted.slice(0, -1))).toBe(true);
  });

  it('never shortens a label away to nothing, whatever it is given', () => {
    // The one thing this must not do. A node with no name hides that a page exists at all.
    for (const title of ['A', 'x'.repeat(400), 'Wortohnejedeleerstelleundsehrlang'.repeat(4)]) {
      expect(fitLabel(title).replace('…', '').length).toBeGreaterThan(0);
    }
  });

  it('cuts at a word rather than mid-word when there is a word to cut at', () => {
    expect(fitLabel('Table 5: Age-Stratified Stepwise Plans with Decision Points')).toBe(
      'Table 5: Age-Stratified…'
    );
  });

  it('cuts mid-word rather than throw most of the label away for a tidy edge', () => {
    // The other side of that trade. »Detaillierte Interventionsstrategien« has its only
    // space at the thirteenth character of the twenty-eight that fit, so cutting at the word
    // would spend more than half the label on tidiness and leave a label that says less.
    expect(fitLabel('Detaillierte Interventionsstrategien — Tabelle 5')).toBe(
      'Detaillierte Interventionsst…'
    );
  });
});

describe('the defect: one label centred under every node', () => {
  /**
   * What this measured before any of it was fixed, and what it measures now.
   *
   * The defect was that the route drew every title centred at `node.y + NODE_RADIUS + 14`
   * whatever was already there, in a fixed 800×560 frame. Measured against the real corpus at
   * commit e69cd0c, that was **44 overlapping pairs of labels out of 35**, and **17 of the 35
   * labels reached past the edge of the frame**, where the SVG viewport cut them mid-word
   * with no ellipsis to admit it. Those two numbers are the baseline this work is judged
   * against, and they cannot be re-measured here, because the code that produced them is
   * gone: `layout` now spreads nodes by the width of their names before anything is drawn.
   *
   * What CAN still be measured, and is below, is the same naive placement applied to what
   * `layout` produces today: what the graph would look like if the label pass were deleted
   * and only the spreading kept. It still collides, which is the point — the two halves of
   * the fix each do real work, and neither is decoration on the other.
   */
  it('still overlaps at corpus size even after the nodes have been spread apart', () => {
    const { nodes } = layout(CORPUS.nodes, CORPUS.edges, { width: 800, height: 560 });
    const boxes = nodes.map((node) => boxAt(node, NODE_RADIUS + 14));
    expect(overlappingPairs(boxes)).toBeGreaterThan(0);
  });

  it('still runs off the edge of the drawing when the title is not shortened', () => {
    // The half of the defect nothing in the markup admitted to: a label wider than the room
    // beside it was not shortened, it was drawn anyway and clipped by the viewport.
    const { nodes } = layout(CORPUS.nodes, CORPUS.edges, { width: 800, height: 560 });
    const clipped = nodes
      .map((node) => boxAt(node, NODE_RADIUS + 14))
      .filter((box) => box.x < 0 || box.x + box.width > 800);
    expect(clipped.length).toBeGreaterThan(0);
  });
});

describe('the labels as they are placed now', () => {
  for (const [name, build] of SIZES) {
    describe(name, () => {
      const graph = build();
      const placed = layout(graph.nodes, graph.edges);

      it('draws no label on top of another one', () => {
        expect(overlappingPairs(placed.nodes.map((node) => node.label.box))).toBe(0);
      });

      it('draws no label on top of a node', () => {
        // A label across a circle is as unreadable as a label across a label, and it also
        // hides the node it is naming.
        for (const label of placed.nodes.map((node) => node.label)) {
          for (const node of placed.nodes) {
            expect(touchesCircle(label.box, node, NODE_RADIUS)).toBe(false);
          }
        }
      });

      it('keeps every label inside the drawing', () => {
        for (const { label } of placed.nodes) {
          expect(label.box.x).toBeGreaterThanOrEqual(0);
          expect(label.box.y).toBeGreaterThanOrEqual(0);
          expect(label.box.x + label.box.width).toBeLessThanOrEqual(placed.width);
          expect(label.box.y + label.box.height).toBeLessThanOrEqual(placed.height);
        }
      });

      it('gives every node a name, and never an empty one', () => {
        expect(placed.nodes).toHaveLength(graph.nodes.length);
        for (const node of placed.nodes) {
          expect(node.label.text.length).toBeGreaterThan(0);
          expect(node.title.startsWith(node.label.text.replace(/…$/, ''))).toBe(true);
        }
      });

      it('keeps every label next to the node it names', () => {
        // A label is only a name if you can tell whose it is. Two rows of label plus the
        // node's own radius is the furthest any candidate position reaches.
        const reach = NODE_RADIUS + GAP + 2 * LABEL_HEIGHT + 2;
        for (const node of placed.nodes) {
          const centre = {
            x: node.label.box.x + node.label.box.width / 2,
            y: node.label.box.y + node.label.box.height / 2
          };
          expect(Math.abs(centre.y - node.y)).toBeLessThanOrEqual(reach);
          expect(Math.abs(centre.x - node.x)).toBeLessThanOrEqual(
            node.label.box.width / 2 + reach
          );
        }
      });

      it('draws the same picture twice, so hydration has nothing to disagree with', () => {
        const again = layout(graph.nodes, graph.edges);
        expect(JSON.stringify(again)).toBe(JSON.stringify(placed));
      });
    });
  }
});

describe('placeLabels, given no room to work with', () => {
  // `layout` spreads the nodes out first, so on a real graph almost every label ends up in
  // the first position it tries — below its node, exactly where the graph has always drawn
  // it. These are the cases that spreading cannot fix and the alternatives exist for, made
  // by hand because a real corpus is not obliging enough to produce them on demand.
  const frame = { width: 400, height: 300, radius: NODE_RADIUS };

  it('moves one of two stacked nodes out from under the other', () => {
    // Twenty-five units apart: there is no room for both labels below their nodes, since the
    // upper one's label would be drawn across the lower one's circle.
    const stacked = [
      { title: 'Erste Seite', x: 200, y: 120 },
      { title: 'Zweite Seite', x: 200, y: 145 }
    ];
    const [first, second] = placeLabels(stacked, frame);
    expect(overlaps(first.box, second.box)).toBe(false);
    expect(first.crowded || second.crowded).toBe(false);
    // Exactly one of them keeps the usual spot below its node; the other goes elsewhere.
    const below = [first.y > stacked[0].y, second.y > stacked[1].y];
    expect(below.filter(Boolean)).toHaveLength(1);
  });

  it('moves a label off a node that is standing where the label would go', () => {
    const blocked = [
      { title: 'Beschriftet', x: 200, y: 120 },
      { title: 'X', x: 200, y: 140 },
      { title: 'Y', x: 260, y: 140 }
    ];
    const [label] = placeLabels(blocked, frame);
    for (const node of blocked) {
      expect(touchesCircle(label.box, node, NODE_RADIUS)).toBe(false);
    }
  });

  it('never leaves a node nameless, even with nowhere at all to put the name', () => {
    // Nine nodes on one spot: no arrangement of nine labels can avoid every collision. The
    // rule is that they are all still drawn — a name overlapping a name is a readability
    // problem, a missing name hides that the page exists.
    const pile = Array.from({ length: 9 }, (_, i) => ({
      title: `Seite ${i} mit einem langen Namen`,
      x: 200,
      y: 150
    }));
    const labels = placeLabels(pile, frame);
    expect(labels).toHaveLength(9);
    for (const label of labels) expect(label.text.length).toBeGreaterThan(0);
    expect(labels.some((label) => label.crowded)).toBe(true);
  });
});

describe('the size the labels are actually drawn at', () => {
  it('is the size every box in this file was measured against', () => {
    // The one coupling in all of this that nothing else would catch. Every box here is
    // derived from LABEL_FONT_SIZE, and what actually draws the text is a `font-size` in the
    // graph route's stylesheet. Change that to 15px and the placement keeps reserving room
    // for 13px text: no error, no failing type, just labels quietly touching again. So the
    // stylesheet is read rather than trusted — the same reason `fonts.test.ts` reads
    // `tokens.css` instead of asserting against a TypeScript copy of it.
    const route = readFileSync(new URL('../../routes/graph/+page.svelte', import.meta.url), 'utf8');
    const nodesText = route.slice(route.indexOf('.nodes text {'));
    const size = /font-size:\s*(\d+)px;/.exec(nodesText);
    expect(size, 'the graph route no longer sets a px font-size on .nodes text').not.toBeNull();
    expect(Number(size![1])).toBe(LABEL_FONT_SIZE);
  });
});

describe('the frame', () => {
  it('grows with the corpus, downwards', () => {
    // Downwards and not sideways, because the SVG is scaled to the width of the column it is
    // drawn in: a wider viewBox shrinks every label on screen, a taller one does not.
    expect(frameHeight(3)).toBe(560);
    expect(frameHeight(35)).toBeGreaterThan(frameHeight(10));
    expect(frameHeight(80)).toBeGreaterThan(frameHeight(35));
    expect(layout(CORPUS.nodes, CORPUS.edges).width).toBe(1100);
  });

  it('does not draw three pages as a letterbox', () => {
    const three = CORPUS.nodes.slice(0, 3);
    expect(layout(three, []).height).toBe(560);
  });
});

describe('the layout still holds its old promises', () => {
  const nodes: Placed[] = layout(CORPUS.nodes, CORPUS.edges).nodes;

  it('places every node inside the frame', () => {
    const { width, height } = layout(CORPUS.nodes, CORPUS.edges);
    for (const node of nodes) {
      expect(node.x).toBeGreaterThanOrEqual(0);
      expect(node.x).toBeLessThanOrEqual(width);
      expect(node.y).toBeGreaterThanOrEqual(0);
      expect(node.y).toBeLessThanOrEqual(height);
      expect(Number.isFinite(node.x)).toBe(true);
      expect(Number.isFinite(node.y)).toBe(true);
    }
  });

  it('never puts two nodes on the same spot', () => {
    for (let i = 0; i < nodes.length; i += 1) {
      for (let j = i + 1; j < nodes.length; j += 1) {
        expect(Math.hypot(nodes[i].x - nodes[j].x, nodes[i].y - nodes[j].y)).toBeGreaterThan(
          2 * NODE_RADIUS
        );
      }
    }
  });
});
