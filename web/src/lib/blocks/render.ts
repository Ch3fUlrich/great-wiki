import { slugify } from '$lib/slug';

// Mirrors crates/gw-core/src/block.rs. Kinds the renderer does not know are skipped
// rather than rendered raw — that is what makes an unknown block safe.
export type BlockKind =
  | 'doc' | 'paragraph' | 'heading' | 'bulletList' | 'orderedList'
  | 'listItem' | 'blockquote' | 'codeBlock'
  // `tableHeader` is a header *cell* (`th`), not a header row — a row is a `tableRow`
  // whichever kind of cell it holds. That is how ProseMirror models it, and it is what
  // lets the renderer choose `th` over `td` from the cell alone.
  | 'table' | 'tableRow' | 'tableHeader' | 'tableCell'
  | 'text';

export interface Block {
  kind: BlockKind;
  attrs?: Record<string, unknown>;
  content?: Block[];
  text?: string;
}

export interface Heading {
  level: number;
  text: string;
  id: string;
}

// A byte-for-byte mirror of `Block::plain_text` in crates/gw-core/src/block.rs, down to
// the rule about where a space goes. A BLOCK boundary is written as a space, or the last
// word of one block fuses to the first of the next ("MaßEin"). Adjacent inline text leaves
// of one parent get NOTHING between them: they are one run of prose that a mark boundary
// happened to split, and a space there would put a full stop off the end of its word —
// "Siehe das Handbuch ." — in this outline, in a heading's anchor id and in a table's
// column labels. The shared cases live in both test suites; if the two drift, one goes red.
export function plainText(block: Block): string {
  let out = '';
  const walk = (b: Block) => {
    if (b.text) out += b.text;
    let previous: BlockKind | undefined;
    for (const child of b.content ?? []) {
      if (child.kind !== 'text' || (previous !== undefined && previous !== 'text')) out += ' ';
      walk(child);
      previous = child.kind;
    }
  };
  walk(block);
  return out.replace(/\s+/g, ' ').trim();
}

export function outline(block: Block): Heading[] {
  const out: Heading[] = [];
  const walk = (b: Block) => {
    if (b.kind === 'heading') {
      const raw = Number(b.attrs?.level ?? 1);
      const level = Math.min(6, Math.max(1, Number.isFinite(raw) ? raw : 1));
      const text = plainText(b);
      out.push({ level, text, id: slugify(text) });
      return; // headings do not nest
    }
    b.content?.forEach(walk);
  };
  walk(block);
  return out;
}
