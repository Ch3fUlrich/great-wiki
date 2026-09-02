import { slugify } from '$lib/slug';

// Mirrors crates/gw-core/src/block.rs. Kinds the renderer does not know are skipped
// rather than rendered raw — that is what makes an unknown block safe.
export type BlockKind =
  | 'doc' | 'paragraph' | 'heading' | 'bulletList' | 'orderedList'
  | 'listItem'
  // A checklist and one of its lines. Two kinds rather than a `checked` attribute on
  // `listItem`, because that is how TipTap models it and `BlockKind` mirrors the editor
  // exactly — see `gw_core::BlockKind::TaskList`. A `taskItem` carries `checked` and
  // nothing else; the uuid the data model gives a task is minted by the store on publish,
  // never by the converter.
  | 'taskList' | 'taskItem'
  | 'blockquote' | 'codeBlock'
  // `tableHeader` is a header *cell* (`th`), not a header row — a row is a `tableRow`
  // whichever kind of cell it holds. That is how ProseMirror models it, and it is what
  // lets the renderer choose `th` over `td` from the cell alone.
  | 'table' | 'tableRow' | 'tableHeader' | 'tableCell'
  // A file placed in the prose (D-15). An atom: no content, and its whole meaning is the two
  // attributes `placedFile` below reads. A reference to a row in the page's `Anhänge` list,
  // never a possession — see `gw_core::BlockKind::Attachment`.
  | 'attachment'
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
 * every reader of a public wiki, including an admin. So the check lives at the sink, where it
 * covers every producer at once rather than every producer having to remember.
 *
 * There is a Content-Security-Policy behind this now (`kit.csp` in `web/vite.config.ts`, and
 * `docs/decisions/0007-content-security-policy.md`), and `script-src` with no
 * `'unsafe-inline'` would refuse a `javascript:` navigation on its own. It is the second
 * line and not the first: this function is what makes such a link render as PLAIN TEXT,
 * which is a page that reads correctly rather than a page with a dead control on it, and it
 * is the only one of the two that still works wherever the policy is not in force.
 *
 * Judged by the WHATWG `URL` parser rather than by a regex on the string, because that is
 * exactly the parser the browser will use on the value: it lower-cases the scheme, strips
 * leading and trailing whitespace and *removes* embedded tabs and newlines, so `JaVaScRiPt:`
 * and `java\nscript:` are the same URL to it and to this. Relative references keep working —
 * resolved against a base, they take the base's scheme, which is the honest answer for a link
 * that has no scheme of its own to abuse.
 *
 * A protocol-relative `//host/path` is therefore ALLOWED, which is deliberate rather than an
 * oversight: it takes the base's `https:` and points off-site, and `https://host/path` spelled
 * out in full is allowed too — refusing the shorter spelling would deny nothing an author
 * cannot say another way. `normalizeLinkAddress` (`$lib/editor/linkAddress`) reads such an
 * address the same way, as an address of somewhere else; it used to disagree, silently turning
 * it into a path of this wiki.
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

/** What an `attachment` block says: which file on this page, and what it shows. */
export interface PlacedFile {
  /** The name the file has **on the page this block is in**. Never a path and never a hash. */
  filename: string;
  /** What the picture shows. May be empty; the reader falls back to the filename. */
  alt: string;
}

/**
 * The file an `attachment` block places, or `null` for a block that names none.
 *
 * **The page half of the address is where the block IS**, and is deliberately not stored:
 * a placement is a top-level block of one document's body, so "which page" is never in
 * question, and a stored page name would be an address that outlives a move. That matters
 * beyond tidiness — a download is authorised against the page it was reached through (D-16),
 * so the only address this interface may ever use is the `href` the API built for THIS
 * page's list. Nothing here assembles one, which is why this returns a name to look up
 * rather than a URL to fetch.
 *
 * `null` rather than a guess for a block whose `filename` is missing or is not a string.
 * The renderer draws nothing for it: a placement that cannot say which file it means is a
 * malformed block, and inventing an empty name would ask the list for a file called "".
 */
export function placedFile(block: Block): PlacedFile | null {
  const filename = block.attrs?.filename;
  if (typeof filename !== 'string' || filename.trim() === '') return null;
  const alt = block.attrs?.alt;
  return { filename, alt: typeof alt === 'string' ? alt : '' };
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

/**
 * A code block's text exactly as it was typed — every newline, every space of indentation.
 *
 * **Not `plainText`, and `plainText` must not be widened to do this.** That function ends
 * `.replace(/\s+/g, ' ').trim()`, and it is a byte-for-byte mirror of
 * `gw_core::Block::plain_text`: it feeds every heading anchor id, the outline, a table's
 * column labels and (at M7) the search index, with shared cases duplicated in both test
 * suites so that a drift between the two turns one of them red. Collapsing whitespace is
 * exactly right for prose — a mark boundary must not put a full stop off the end of its
 * word — and exactly wrong for a fence, where the whitespace IS the content: it made every
 * code block on the site one line with its indentation gone, and would hand a diagram
 * renderer `graph TD; A-->B;`, which parses as nothing.
 *
 * So the two questions are asked separately. `gw_core::Block::diff_text` is the server-side
 * counterpart: the structural diff reads a fence this same verbatim way, so a revision that
 * only reindents one is reported rather than swallowed.
 *
 * Leaves are concatenated with NOTHING between them. One fence can reach the reader as more
 * than one text leaf, and a separator would insert a character the author never typed into
 * the middle of a line of code.
 */
export function codeText(block: Block): string {
  let out = block.text ?? '';
  for (const child of block.content ?? []) out += codeText(child);
  return out;
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
