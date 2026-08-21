/**
 * Where each node's title is drawn.
 *
 * Split out of `layout.ts` because placing a node and placing its NAME are different problems
 * with different geometry. A node is a point that repels its neighbours the same in every
 * direction; a label is a long, thin box — the titles in this wiki run to eighty-four
 * characters — that must not land on another label, must not land on a node, and must not
 * fall off the edge of the drawing. The force layout cannot express any of that, and the one
 * thing it may not give up in order to try is determinism.
 *
 * Nothing here imports `layout.ts`, deliberately: the node radius arrives as an argument
 * instead, so the dependency runs one way (layout uses labels) and neither file has to be
 * read to understand the other.
 */

/** The size the graph route sets on `.nodes text`. Keep the two in step. */
export const LABEL_FONT_SIZE = 13;

/**
 * The average advance width of one character, in ems.
 *
 * **This is an approximation, and it is the only one in this file.** A label's real width can
 * only come from the font, and there is no font here: `layout()` runs during server
 * rendering, where there is no DOM, no `getComputedTextLength()` and no loaded face — and the
 * face is not even knowable, since `--font-sans` is IBM Plex Sans with a system fallback and
 * the reader may have neither.
 *
 * So a label's width is its character count times this constant. 0.55 em is measured rather
 * than guessed: the thirty-five real titles in `corpus.fixture.ts`, set in DejaVu Sans (the
 * widest of the plausible fallbacks), average 0.518 em per character, the widest single title
 * 0.583 and the narrowest 0.489. Rounding UP to 0.55 is deliberate — over-estimating a label
 * can only make the placement below more cautious, which costs a little air, whereas
 * under-estimating draws two labels on top of each other and reports the result as clean. A
 * narrower face (IBM Plex Sans itself, Helvetica) simply leaves more room than was reserved.
 *
 * What a single constant cannot model is a title of nothing but capitals or nothing but `i`s,
 * about 0.72 and 0.28 em. A per-character table would fix that and is not worth writing until
 * a real title makes it wrong.
 */
export const AVERAGE_ADVANCE = 0.55;

/**
 * The box one line of label occupies, and where its baseline sits inside it.
 *
 * SVG positions text by its BASELINE, not by its top, so the box has to be derived from it:
 * the ascent above and the descent below. 0.8 em / 0.35 em is the usual envelope for a sans
 * face at this size — an `M` reaches about 0.72 em above the baseline and a `g` about 0.21
 * below — with the same deliberate rounding-up as the advance width above.
 */
export const LABEL_ASCENT = LABEL_FONT_SIZE * 0.8;
export const LABEL_HEIGHT = LABEL_FONT_SIZE * 1.15;

/**
 * The widest a label may be drawn before it is shortened, in layout units.
 *
 * A cap is not a nicety at this corpus: twelve of the owner's thirty-five titles are over
 * fifty characters and the longest is eighty-four, which at 13px is 600 units — three
 * quarters of the drawing that USED to be 800 wide. Such a label cannot be placed anywhere
 * without hitting something, and before this it was not placed anywhere: it was centred on
 * its node, ran off the side of the frame and was cut mid-word by the SVG viewport, silently
 * and with no ellipsis to admit it. Seventeen of the thirty-five were being cut that way.
 *
 * 210 units is about twenty-eight characters and a fifth of the frame's width, which is
 * enough to tell "Table 2: Clinical Monitoring…" from "Table 3: SCFA Dysbalance" — the job a
 * label in a diagram has. **The whole title is never lost**: it stays in the `<title>` a
 * pointer shows, and in the text twin below the drawing that a screen reader reads.
 */
export const MAX_LABEL_WIDTH = 210;

/** Clear air between a label and the node it names. */
export const GAP = 5;

/** The extra step out to a second row of label, so two rows do not sit edge to edge. */
const CROWD = 2;

/** An axis-aligned box, `x`/`y` at its top-left corner. */
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * How wide this text will be drawn, near enough to place it by.
 *
 * Counted in CODE POINTS rather than UTF-16 code units: `String.length` counts a character
 * outside the basic plane twice, which would estimate such a title at double its width and
 * shove everything around it aside to make room for nothing.
 */
export function estimateTextWidth(text: string, fontSize = LABEL_FONT_SIZE): number {
  return [...text].length * fontSize * AVERAGE_ADVANCE;
}

/**
 * The title as it will be drawn: whole if it fits, cut at a word with an ellipsis if not.
 *
 * The ellipsis is one character and is counted, so the result still fits. Cutting at the last
 * space rather than mid-word costs a few units of width and buys a label that reads as
 * language — but only when there is a space near the end, since a single eighty-character
 * word cut back to its first space would be shortened to nothing.
 */
export function fitLabel(title: string, maxWidth = MAX_LABEL_WIDTH): string {
  const chars = [...title];
  if (estimateTextWidth(title) <= maxWidth) return title;
  const room = Math.max(1, Math.floor(maxWidth / (LABEL_FONT_SIZE * AVERAGE_ADVANCE)) - 1);
  const cut = chars.slice(0, room).join('');
  const space = cut.lastIndexOf(' ');
  const kept = space >= room * 0.6 ? cut.slice(0, space) : cut.trimEnd();
  return `${kept}…`;
}

/** Whether two boxes share any area at all. Touching edges do not count as overlapping. */
export function overlaps(a: Box, b: Box, gap = 0): boolean {
  return (
    a.x < b.x + b.width + gap &&
    b.x < a.x + a.width + gap &&
    a.y < b.y + b.height + gap &&
    b.y < a.y + a.height + gap
  );
}

/** How much area two boxes share. Zero when they do not touch. */
function sharedArea(a: Box, b: Box): number {
  const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

/** Whether a box reaches a circle. The nearest point of the box to the centre decides. */
export function touchesCircle(box: Box, circle: { x: number; y: number }, radius: number): boolean {
  const nx = Math.min(Math.max(circle.x, box.x), box.x + box.width);
  const ny = Math.min(Math.max(circle.y, box.y), box.y + box.height);
  return Math.hypot(circle.x - nx, circle.y - ny) < radius;
}

/** One title, placed. `x`/`y` are where the SVG `<text>` goes; `box` is what it covers. */
export interface PlacedLabel {
  /** The text as drawn, which is the title unless it was too wide — see `fitLabel`. */
  text: string;
  x: number;
  y: number;
  anchor: 'start' | 'middle' | 'end';
  box: Box;
  /** True when no position was free of everything and the least bad one was taken. */
  crowded: boolean;
}

type Spot = { box: Box; anchor: PlacedLabel['anchor']; x: number };

const centred = (node: { x: number; y: number }, w: number, h: number, dy: number): Spot => ({
  box: { x: node.x - w / 2, y: node.y + dy, width: w, height: h },
  anchor: 'middle',
  x: node.x
});

const rightOf = (
  node: { x: number; y: number },
  w: number,
  h: number,
  dx: number,
  dy: number
): Spot => ({
  box: { x: node.x + dx, y: node.y + dy, width: w, height: h },
  anchor: 'start',
  x: node.x + dx
});

const leftOf = (
  node: { x: number; y: number },
  w: number,
  h: number,
  dx: number,
  dy: number
): Spot => ({
  box: { x: node.x - dx - w, y: node.y + dy, width: w, height: h },
  anchor: 'end',
  x: node.x - dx
});

/**
 * The positions a label is tried in, in the order it tries them.
 *
 * Below the node first, because that is where a reader expects a caption and where this graph
 * has always put it — the ring of alternatives exists only for the labels that cannot have
 * it, and on a graph with room most labels still sit exactly where they used to. Then above,
 * then the two sides, then the four diagonals, then a second row further out, which is what
 * rescues a node in the middle of a cluster whose eight neighbours are all taken.
 */
function spots(node: { x: number; y: number }, w: number, h: number, r: number): Spot[] {
  const near = r + GAP;
  const diag = r * 0.71 + GAP;
  return [
    centred(node, w, h, near),
    centred(node, w, h, -near - h),
    rightOf(node, w, h, near, -h / 2),
    leftOf(node, w, h, near, -h / 2),
    rightOf(node, w, h, diag, diag),
    leftOf(node, w, h, diag, diag),
    rightOf(node, w, h, diag, -diag - h),
    leftOf(node, w, h, diag, -diag - h),
    centred(node, w, h, near + h + CROWD),
    centred(node, w, h, -near - 2 * h - CROWD),
    rightOf(node, w, h, near, near + h),
    leftOf(node, w, h, near, near + h),
    rightOf(node, w, h, near, -near - 2 * h),
    leftOf(node, w, h, near, -near - 2 * h)
  ];
}

export interface LabelOptions {
  width: number;
  height: number;
  /** The drawn radius of a node, which a label must clear. */
  radius: number;
}

/**
 * Place every label, in one deterministic pass.
 *
 * Greedy, in a fixed order — widest title first, ties broken by the order the nodes arrived
 * in — because the widest label has the fewest places it can go, and leaving it until last
 * means it has none. Each label takes the first position that is clear of the labels already
 * placed, of every node circle, and of the frame's edge.
 *
 * **A label is never dropped and never blanked.** When no position is clear, the label takes
 * the position that collides LEAST rather than a fixed fallback, and is marked `crowded`:
 * a name overlapping another name is a readability problem, whereas a missing name hides that
 * the page exists at all, which is a correctness one. The scoring below is what makes "least"
 * meaningful — a label on top of another label is the worst outcome, running off the edge of
 * the frame is next, and clipping a node circle by a unit or two is a blemish.
 */
export function placeLabels<T extends { title: string; x: number; y: number }>(
  nodes: T[],
  options: LabelOptions
): PlacedLabel[] {
  const { width: frameWidth, height: frameHeight, radius } = options;
  const height = LABEL_HEIGHT;

  const entries = nodes.map((node, i) => {
    const text = fitLabel(node.title);
    return { node, i, text, width: estimateTextWidth(text) };
  });
  const order = [...entries].sort((a, b) => b.width - a.width || a.i - b.i);

  const taken: Box[] = [];
  const placed: PlacedLabel[] = new Array(nodes.length);

  const outsideFrame = (box: Box) =>
    Math.max(0, -box.x) +
    Math.max(0, box.x + box.width - frameWidth) +
    Math.max(0, -box.y) +
    Math.max(0, box.y + box.height - frameHeight);

  /**
   * Zero for a position that is clear of everything; larger the worse it is.
   *
   * `owner` — the node being labelled — is skipped, and not as an optimisation. Every
   * candidate below is built to clear its own node by exactly `GAP`, so testing it against
   * that node asks floating-point arithmetic whether 13 is less than 13, and an answer of
   * "yes" would score a label as colliding with the very node it belongs to and send it off
   * to some worse position for nothing.
   */
  const cost = (box: Box, owner: T) => {
    let score = 0;
    for (const other of taken) score += 4 * sharedArea(box, other);
    score += 30 * outsideFrame(box);
    for (const other of nodes) {
      if (other === owner) continue;
      if (touchesCircle(box, other, radius + GAP)) score += 200;
    }
    return score;
  };

  for (const { node, i, text, width } of order) {
    // Starting from the first candidate rather than from "nothing chosen yet" is what makes
    // "a label is always placed" a property of the code rather than a promise about it.
    const candidates = spots(node, width, height, radius);
    let best = candidates[0];
    let bestCost = cost(best.box, node);
    for (let c = 1; c < candidates.length && bestCost > 0; c += 1) {
      const score = cost(candidates[c].box, node);
      if (score < bestCost) {
        best = candidates[c];
        bestCost = score;
      }
    }

    taken.push(best.box);
    placed[i] = {
      text,
      x: round(best.x),
      y: round(best.box.y + LABEL_ASCENT),
      anchor: best.anchor,
      box: best.box,
      crowded: bestCost > 0
    };
  }

  return placed;
}

/**
 * Push nodes apart until no node's LABEL lands on another node's label or circle.
 *
 * This is the half of the fix the label placement above cannot do on its own. A force layout
 * treats every node as a point, so it spaces a page called "Darm" exactly as far from its
 * neighbour as one called "4.4 Early Childhood (2 to 6 Years)…", and no amount of moving the
 * second one's label around afterwards invents room that was never left for it. So each node
 * is given the footprint its name actually needs — as wide as the label, as tall as a label
 * above plus a label below — and any two footprints that overlap are pushed apart along the
 * axis they overlap least, both of them, until they do not.
 *
 * Least-overlap is the whole of why it converges. Pushing along the axis of least RELATIVE
 * overlap was tried first and jams: label footprints are wide and short, so it almost always
 * chooses to push sideways, demands two hundred units of clearance per pair, runs out of
 * frame and stops with two thirds of the collisions still there (measured: 22 overlapping
 * pairs down to 16). Pushing along the axis of least ABSOLUTE overlap prefers the cheap
 * vertical shuffle instead, and settles that same graph to none at all.
 *
 * Deterministic throughout: pairs in index order, ties broken towards the lower index, no
 * randomness, so two identical requests separate identically. Each pair moves three tenths of
 * the way apart per sweep rather than the whole way at once, because the whole way has two
 * nodes trade places and shove each other back and forth for ever.
 *
 * Four hundred sweeps, which sounds like a lot and costs 1.4 ms for thirty-five nodes and
 * 3.3 ms for eighty — the force loop it follows is fifteen times that. It is a relaxation, not
 * a solve, so it is worth the sweeps: at 160 the graph is settled at every size measured
 * except eighty and a hundred and twenty nodes, where one label was still left sitting on
 * another. It stops early the moment nothing overlaps.
 */
export function separate<T extends { title: string; x: number; y: number }>(
  nodes: T[],
  options: LabelOptions & { iterations?: number; margin?: number }
): T[] {
  const { width: frameWidth, height: frameHeight, radius } = options;
  const iterations = options.iterations ?? 400;
  const margin = options.margin ?? 0;

  const at = nodes.map((node) => ({ x: node.x, y: node.y }));
  // Half a footprint. The extra `radius + GAP` on the x axis is not padding: without it two
  // nodes can be spaced by exactly their two half-labels and the second node's CIRCLE then
  // sits right at the end of the first one's label, which `placeLabels` refuses — so the
  // separation would report itself finished and the placement would still have work to do.
  const half = nodes.map((node) => ({
    x: Math.max(radius, estimateTextWidth(fitLabel(node.title)) / 2) + radius + GAP,
    y: radius + GAP + LABEL_HEIGHT
  }));

  for (let step = 0; step < iterations; step += 1) {
    let moved = false;
    for (let i = 0; i < at.length; i += 1) {
      for (let j = i + 1; j < at.length; j += 1) {
        const dx = at[j].x - at[i].x;
        const dy = at[j].y - at[i].y;
        const px = half[i].x + half[j].x - Math.abs(dx);
        const py = half[i].y + half[j].y - Math.abs(dy);
        if (px <= 0 || py <= 0) continue;
        moved = true;
        if (px < py) {
          const push = (dx >= 0 ? 1 : -1) * px * 0.3;
          at[i].x -= push;
          at[j].x += push;
        } else {
          const push = (dy >= 0 ? 1 : -1) * py * 0.3;
          at[i].y -= push;
          at[j].y += push;
        }
      }
    }

    // Back inside the frame, by as much room as this node's own label needs — a wide label
    // centred on a node near the edge is the other way a label leaves the drawing.
    const roomY = Math.max(margin, radius + GAP + LABEL_HEIGHT);
    for (let i = 0; i < at.length; i += 1) {
      const roomX = Math.max(margin, half[i].x - radius - GAP);
      at[i].x = clamp(at[i].x, roomX, frameWidth - roomX);
      at[i].y = clamp(at[i].y, roomY, frameHeight - roomY);
    }

    if (!moved) break;
  }

  return nodes.map((node, i) => ({ ...node, x: at[i].x, y: at[i].y }));
}

function clamp(n: number, low: number, high: number): number {
  return high < low ? (low + high) / 2 : Math.min(high, Math.max(low, n));
}

/** Two decimal places, because these numbers end up in markup. See `layout()`. */
function round(n: number): number {
  return Math.round(n * 100) / 100;
}
