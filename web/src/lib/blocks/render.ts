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

// Mirrors `gw_core::MarkKind` — the wire name IS the Yjs attribute key `gw-collab` reads and
// writes (`doc.rs::mark_key_of` is the server's serde `rename_all = "camelCase"` name for the
// kind), so `strong`/`em` here, never TipTap's own `bold`/`italic`. `web/src/lib/editor/
// extensions.ts` renames TipTap's Bold and Italic extensions for exactly this reason, and
// pins the whole set against the server's with a test the way `SERVER_BLOCK_KINDS` is.
export type MarkKind = 'strong' | 'em' | 'code' | 'strike' | 'link';

// A link carries EITHER `doc` (internal, resolved by the server — Task 7) or `href`
// (external). Mirrors `gw_core::Mark`.
export interface Mark {
  kind: MarkKind;
  attrs?: Record<string, unknown>;
}

// The schemes a stored link may become a real `<a href>` for. `mailto:` is here because a
// wiki links to addresses; `tel:`, `ftp:` and the rest of TipTap's own allow-list are not,
// because nothing in this corpus uses them and every scheme admitted is a scheme that has to
// be argued about again later.
const LINK_SCHEMES = new Set(['http:', 'https:', 'mailto:']);

/**
 * `href` if a browser may be handed it, `null` if it must be rendered as plain text instead.
 *
 * A link's `href` is a string that reached the database from imported markdown, from the
 * editor's Link control, or from any writer added later; `gw_core::Mark::link_to_url` does
 * not validate it and neither does `gw-collab`. `<a href="javascript:…">` is therefore a
 * stored cross-site-scripting payload that anyone with write access to one page can leave for
 * every reader of a public wiki, including an admin — and there is no Content-Security-Policy
 * to catch it (a known gap, recorded in `docs/operations/running-in-production.md`). So the
 * check lives at the sink, where it covers every producer at once rather than every producer
 * having to remember.
 *
 * Judged by the WHATWG `URL` parser rather than by a regex on the string, because that is
 * exactly the parser the browser will use on the value: it lower-cases the scheme, strips
 * leading and trailing whitespace and *removes* embedded tabs and newlines, so `JaVaScRiPt:`
 * and `java\nscript:` are the same URL to it and to this. Relative references keep working —
 * resolved against a base, they take the base's scheme, which is the honest answer for a link
 * that has no scheme of its own to abuse.
 */
export function safeHref(href: unknown): string | null {
  if (typeof href !== 'string' || href.trim() === '') return null;
  let scheme: string;
  try {
    // The base is a placeholder and never reaches the page: what is returned is the ORIGINAL
    // string, so a relative link stays relative and resolves against the real page it is on.
    scheme = new URL(href, 'https://wiki.invalid/').protocol;
  } catch {
    return null;
  }
  return LINK_SCHEMES.has(scheme) ? href : null;
}

export interface Block {
  kind: BlockKind;
  attrs?: Record<string, unknown>;
  content?: Block[];
  text?: string;
  // Outermost first, innermost last — the order `gw_core::MARK_ORDER` sorts a leaf's marks
  // into and `gw-collab` preserves on read. This renderer trusts that order rather than
  // re-deriving it: nesting a leaf's marks is just wrapping in array order, and re-sorting
  // here would be the second ordering the server-side docs warn against maintaining.
  marks?: Mark[];
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
