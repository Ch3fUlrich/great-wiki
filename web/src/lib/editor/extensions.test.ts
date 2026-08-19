import { describe, expect, it } from 'vitest';
import { Node as PmNode } from '@tiptap/pm/model';
import * as Y from 'yjs';
import { prosemirrorJSONToYDoc } from '@tiptap/y-tiptap';
import {
  CONTENT_FIELD,
  EDITOR_MARK_NAMES,
  EDITOR_NODE_NAMES,
  SERVER_BLOCK_KINDS,
  SERVER_MARK_KINDS,
  editorSchema
} from './extensions';

/**
 * The editor's schema is a contract with `gw-core::BlockKind`, and every way of breaking it
 * is silent.
 *
 * Two mechanisms make it silent, both read out of `@tiptap/y-tiptap`'s sync plugin and both
 * verified against the installed source rather than remembered:
 *
 * 1. `createNodeFromYElement` calls `schema.node(el.nodeName, …)` and, in its `catch`,
 *    **deletes the element from the Y.Doc**. So a block kind the schema does not know is not
 *    "skipped" — opening the editor DESTROYS it in the CRDT, broadcasts the deletion to
 *    every other editor, and the janitor files the result as a revision thirty seconds
 *    later. Nothing throws and nothing is logged.
 * 2. `updateYFragment` ends its attribute pass with "remove all keys that are no longer in
 *    pAttrs" — `for (const key in yDomAttrs) if (pAttrs[key] === undefined)
 *    yDomFragment.removeAttribute(key)`. ProseMirror's `computeAttrs` iterates the SCHEMA's
 *    attributes, so an attribute the schema does not declare never survives into the node —
 *    and is therefore deleted from the CRDT on the first edit that touches the node.
 *
 * The corpus this runs on is a family's medical reference with tables in it, so both of
 * those are data loss on real pages, not hypotheticals.
 */

/**
 * `gw_core::BlockKind`, in the camelCase serde emits — which is also, by construction, the
 * XML element tag `gw-collab` writes (`doc.rs::tag_of` derives the tag from serde), and
 * therefore exactly the node name TipTap will look up in this schema.
 *
 * Thirteen, not the nine the M3 plan's sample lists: the plan predates the table kinds.
 */
const SERVER_KINDS = [
  'doc',
  'paragraph',
  'heading',
  'bulletList',
  'orderedList',
  'listItem',
  'blockquote',
  'codeBlock',
  'table',
  'tableRow',
  'tableHeader',
  'tableCell',
  'text'
];

/** One node per kind, mirroring `gw-collab/src/fixtures.rs::one_per_kind`. */
const ONE_PER_KIND = {
  type: 'doc',
  content: [
    { type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text: 'Größe' }] },
    { type: 'paragraph', content: [{ type: 'text', text: 'Ein Satz.' }] },
    {
      type: 'bulletList',
      content: [
        { type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'a' }] }] }
      ]
    },
    {
      type: 'orderedList',
      attrs: { start: 3 },
      content: [
        { type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'b' }] }] }
      ]
    },
    { type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'zitat' }] }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}' }] },
    {
      type: 'table',
      content: [
        {
          type: 'tableRow',
          content: [
            {
              type: 'tableHeader',
              attrs: { align: 'center' },
              content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Kopf' }] }]
            }
          ]
        },
        {
          type: 'tableRow',
          content: [
            {
              type: 'tableCell',
              attrs: { align: 'right' },
              content: [{ type: 'paragraph', content: [{ type: 'text', text: '42' }] }]
            }
          ]
        }
      ]
    }
  ]
};

describe('the editor schema', () => {
  it('names exactly the block kinds the server can store', () => {
    // Both directions matter and both are destructive. A kind the editor lacks is deleted
    // from the CRDT on open; a kind only the editor has survives in the CRDT but cannot be
    // expressed by `to_block`, so it vanishes from every revision, search index and export.
    expect([...EDITOR_NODE_NAMES].sort()).toEqual([...SERVER_KINDS].sort());
    expect([...SERVER_BLOCK_KINDS].sort()).toEqual([...SERVER_KINDS].sort());
  });

  it('offers exactly the marks the server can store', () => {
    // Both directions matter, the same way they do for block kinds above: a mark the editor
    // lacks can never be typed, and a mark only the editor has is written into the CRDT under
    // a key `to_block` does not recognise and silently dropped at the next publish.
    expect([...EDITOR_MARK_NAMES].sort()).toEqual(['code', 'em', 'link', 'strike', 'strong']);
    expect([...SERVER_MARK_KINDS].sort()).toEqual(['code', 'em', 'link', 'strike', 'strong']);
  });

  it('writes a mark into the CRDT under gw-collab\'s key, not TipTap\'s own mark name', () => {
    // THE risk this schema exists to close. `crates/gw-collab/src/doc.rs::mark_key_of` keys a
    // leaf's Yjs formatting attributes by `MarkKind`'s serde name — `strong`, `em` — and reads
    // out of `@tiptap/y-tiptap`'s installed source confirm the Yjs attribute key IS the
    // ProseMirror mark's *type name* (`marksToAttributes`: `pattrs[mark.type.name] =
    // mark.attrs`). TipTap's own Bold and Italic extensions are named `bold` and `italic` —
    // verified by running exactly this conversion before `contentExtensions` renamed them,
    // which wrote the attribute keys `bold` and `italic` onto the wire, not `strong`/`em`.
    // `prosemirrorJSONToYDoc` is the same conversion `@tiptap/extension-collaboration` runs
    // when a live editor syncs into a fresh Y.Doc, so this test exercises the real mechanism,
    // not a description of it — and it is the one this whole feature can silently regress
    // without any other test noticing, because nothing else here crosses the CRDT boundary.
    const doc = {
      type: 'doc',
      content: [
        {
          type: 'paragraph',
          content: [
            { type: 'text', text: 'fett', marks: [{ type: 'strong' }] },
            { type: 'text', text: 'kursiv', marks: [{ type: 'em' }] },
            { type: 'text', text: 'quer', marks: [{ type: 'strike' }] },
            { type: 'text', text: 'code', marks: [{ type: 'code' }] },
            {
              type: 'text',
              text: 'link',
              marks: [{ type: 'link', attrs: { href: 'https://example.org' } }]
            }
          ]
        }
      ]
    };

    const ydoc = prosemirrorJSONToYDoc(editorSchema, doc, CONTENT_FIELD);
    const paragraph = ydoc.getXmlFragment(CONTENT_FIELD).get(0);
    if (!(paragraph instanceof Y.XmlElement)) throw new Error('expected a paragraph element');

    const written: Record<string, unknown> = {};
    for (let i = 0; i < paragraph.length; i += 1) {
      const child = paragraph.get(i);
      if (!(child instanceof Y.XmlText)) continue;
      for (const chunk of child.toDelta() as Array<{ attributes?: Record<string, unknown> }>) {
        Object.assign(written, chunk.attributes ?? {});
      }
    }
    const keys = new Set(Object.keys(written));

    expect([...keys].sort()).toEqual(['code', 'em', 'link', 'strike', 'strong']);
    // Named explicitly, not just absent-by-omission: these are the exact wrong keys a naive
    // `Bold`/`Italic` would have written, and the failure they cause (a silently dropped mark)
    // has no other test that would catch it.
    expect(keys.has('bold')).toBe(false);
    expect(keys.has('italic')).toBe(false);

    // The keys are only half the wire contract, and the half that was pinned shipped a bug
    // in the other half. `marksToAttributes` writes `pattrs[mark.type.name] = mark.attrs` —
    // the whole attribute set ProseMirror's `computeAttrs` produced, which is every attribute
    // the mark DECLARES with its default filled in, not the one attribute that was set. Stock
    // TipTap `Link` declares `target`, `rel`, `class` and `title` beside `href`, so this delta
    // carried all five, `gw-collab::attrs_to_marks` copied them verbatim into `Mark::attrs`,
    // and `gw-api::export::render_file` — which compares the whole serialised tree against
    // what its own markdown re-imports as — refused every page containing a link, which fails
    // the entire export run. `Anchor` in `extensions.ts` trims the declaration to `href`; this
    // is what keeps it trimmed. See `crates/gw-api/tests/export.rs` for the other side.
    expect(written.link).toEqual({ href: 'https://example.org' });
  });

  it('drops an attribute it does not declare, which is the mechanism that loses them', () => {
    // Not a test of our code — a test of ProseMirror's `computeAttrs`, stated here because
    // the test below is meaningless without it. An undeclared attribute does not error and
    // does not survive; it simply is not there, and `updateYFragment` then removes it from
    // the CRDT because the ProseMirror node no longer has it.
    const node = editorSchema.nodes.paragraph.create({ align: 'right' });
    expect(node.attrs.align).toBeUndefined();
  });

  it('keeps every attribute the server writes', () => {
    // The exact set `gw-core::markdown` emits: heading level, code-block language, ordered
    // list start (only when it is not 1), and cell alignment on both cell kinds. Alignment
    // is the one that would actually have been lost — it is per cell in this model, and
    // there are tables in the corpus.
    const kept = (kind: string, attrs: Record<string, unknown>) =>
      editorSchema.nodes[kind].create(attrs).attrs;

    expect(kept('heading', { level: 3 }).level).toBe(3);
    expect(kept('codeBlock', { language: 'rust' }).language).toBe('rust');
    expect(kept('orderedList', { start: 7 }).start).toBe(7);
    expect(kept('tableCell', { align: 'right' }).align).toBe('right');
    expect(kept('tableHeader', { align: 'center' }).align).toBe('center');
  });

  it('accepts a document holding one of every kind, nesting included', () => {
    // `create` proves an attribute survives; only a whole tree proves the CONTENT
    // expressions do — that `doc` accepts a table, that a row accepts a header cell, and
    // that a cell holds a paragraph rather than bare text. `check()` is what validates
    // those, and it throws rather than returning false.
    const doc = PmNode.fromJSON(editorSchema, ONE_PER_KIND);
    expect(() => doc.check()).not.toThrow();
    expect(doc.textBetween(0, doc.content.size, ' ')).toContain('Größe');
  });
});
