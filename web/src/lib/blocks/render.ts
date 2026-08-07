import { slugify } from '$lib/slug';

// Mirrors crates/gw-core/src/block.rs. Kinds the renderer does not know are skipped
// rather than rendered raw — that is what makes an unknown block safe.
export type BlockKind =
  | 'doc' | 'paragraph' | 'heading' | 'bulletList' | 'orderedList'
  | 'listItem' | 'blockquote' | 'codeBlock' | 'text';

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

export function plainText(block: Block): string {
  const parts: string[] = [];
  const walk = (b: Block) => {
    if (b.text) parts.push(b.text);
    b.content?.forEach(walk);
  };
  walk(block);
  return parts.join(' ').replace(/\s+/g, ' ').trim();
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
