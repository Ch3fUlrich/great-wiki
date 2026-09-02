import { describe, expect, it } from 'vitest';
import { Node as PmNode } from '@tiptap/pm/model';
import * as Y from 'yjs';
import {
  initProseMirrorDoc,
  prosemirrorJSONToYDoc,
  updateYFragment,
  yXmlFragmentToProseMirrorRootNode
} from '@tiptap/y-tiptap';
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
 * Sixteen, not the nine the M3 plan's sample lists: the plan predates the table kinds,
 * `taskList`/`taskItem` arrived with piece 3's checkbox, and `attachment` with piece 4's
 * placed files.
 */
const SERVER_KINDS = [
  'doc',
  'paragraph',
  'heading',
  'bulletList',
  'orderedList',
  'listItem',
  'taskList',
  'taskItem',
  'blockquote',
  'codeBlock',
  'table',
  'tableRow',
  'tableHeader',
  'tableCell',
  'attachment',
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
    {
      // Nested, because that is what `- [ ] a` / `  - [x] b` imports as and a `taskItem`
      // whose content expression is the stock `paragraph+` cannot hold it.
      type: 'taskList',
      content: [
        {
          type: 'taskItem',
          attrs: { checked: false },
          content: [
            { type: 'paragraph', content: [{ type: 'text', text: 'Milch kaufen' }] },
            {
              type: 'taskList',
              content: [
                {
                  type: 'taskItem',
                  attrs: { checked: true },
                  content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Vollmilch' }] }]
                }
              ]
            }
          ]
        }
      ]
    },
    { type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'zitat' }] }] },
    { type: 'codeBlock', attrs: { language: 'rust' }, content: [{ type: 'text', text: 'fn main() {}' }] },
    // A file placed in the prose, at the top level — the only place the schema admits one.
    { type: 'attachment', attrs: { filename: 'befund.png', alt: 'Röntgenbild, seitlich' } },
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
    // `checked` is the whole state of a checkbox, and it is the one attribute in this list
    // whose loss is invisible: a task item stripped of it still renders, still exports, and
    // simply reads as not done. `gw_core::BlockKind::TaskItem` writes it even when false
    // for the same reason.
    expect(kept('taskItem', { checked: true }).checked).toBe(true);
    expect(kept('taskItem', { checked: false }).checked).toBe(false);
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

  // --- The deletion path, exercised rather than described -------------------------------
  //
  // Every test above asserts the SCHEMA. None of them opens a Y.Doc the server wrote, which
  // is the thing that actually destroys a page: `createNodeFromYElement` looks the element
  // tag up in the schema and, in its `catch`, deletes the element from the CRDT. The two
  // tests below drive that function with a document `gw-collab` really could have written,
  // so a kind missing from the schema fails here as an emptied Y.Doc — the same shape as
  // the loss, not a restatement of the rule.

  /**
   * A Y.Doc built the way `gw-collab::doc.rs::write_children` builds one: an element per
   * block, tagged with the serde name of its `BlockKind`, its `attrs` set as XML attributes
   * with their JSON types intact, and a text leaf as an `XmlText`.
   */
  function serverWrittenChecklist(attrs: Record<string, unknown> = { checked: true }): Y.Doc {
    const ydoc = new Y.Doc();
    const paragraph = new Y.XmlElement('paragraph');
    const leaf = new Y.XmlText();
    leaf.insert(0, 'Milch kaufen');
    paragraph.insert(0, [leaf]);

    const item = new Y.XmlElement('taskItem');
    // Real JSON values, not strings: `insert_attribute` on the Rust side writes `Any::Bool`
    // for a boolean, and `getAttributes()` hands JavaScript the boolean back.
    for (const [key, value] of Object.entries(attrs)) {
      item.setAttribute(key, value as string);
    }
    item.insert(0, [paragraph]);

    const list = new Y.XmlElement('taskList');
    list.insert(0, [item]);
    ydoc.getXmlFragment(CONTENT_FIELD).insert(0, [list]);
    return ydoc;
  }

  it('does not delete a checklist from the CRDT when the editor opens the page', () => {
    // THE data-loss test. Before `taskList`/`taskItem` were in this schema, opening a page
    // holding a checkbox ran `schema.node('taskList', …)`, threw, and deleted the element
    // — broadcast to every other editor, snapshotted into a revision by the next sweep.
    // Nothing threw, nothing was logged, and the page came back one list shorter.
    const ydoc = serverWrittenChecklist();
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);

    const doc = yXmlFragmentToProseMirrorRootNode(fragment, editorSchema);

    // The Y.Doc still holds what the server put in it.
    expect(fragment.length).toBe(1);
    expect((fragment.get(0) as Y.XmlElement).nodeName).toBe('taskList');
    // And the editor really built the node, rather than skipping it into nothing.
    expect(doc.childCount).toBe(1);
    expect(doc.firstChild?.type.name).toBe('taskList');
    expect(doc.firstChild?.firstChild?.type.name).toBe('taskItem');
    expect(doc.firstChild?.firstChild?.attrs.checked).toBe(true);
    expect(doc.textBetween(0, doc.content.size, ' ')).toContain('Milch kaufen');
  });

  it("writes a task item's `checked` back to the CRDT under that name and nothing beside it", () => {
    // The other half of the wire contract, and the half the `Link` mark got wrong: a task
    // item's attributes travel to `gw_core::Block::attrs` verbatim, so an attribute TipTap
    // declares that `gw-core` never writes would be minted into every stored task the first
    // time somebody edited it, and `gw_api::export::render_file` — which compares the tree
    // against what its own markdown re-imports as — would refuse the page. `{checked}` is
    // exactly what `gw_core::markdown` writes, so `{checked}` is all this may carry.
    const doc = {
      type: 'doc',
      content: [
        {
          type: 'taskList',
          content: [
            {
              type: 'taskItem',
              attrs: { checked: true },
              content: [{ type: 'paragraph', content: [{ type: 'text', text: 'erledigt' }] }]
            }
          ]
        }
      ]
    };

    const ydoc = prosemirrorJSONToYDoc(editorSchema, doc, CONTENT_FIELD);
    const list = ydoc.getXmlFragment(CONTENT_FIELD).get(0);
    if (!(list instanceof Y.XmlElement)) throw new Error('expected a taskList element');
    expect(list.nodeName).toBe('taskList');
    // `gw_core::markdown` gives a task list no attributes at all, so neither may this.
    expect(list.getAttributes()).toEqual({});

    const item = list.get(0);
    if (!(item instanceof Y.XmlElement)) throw new Error('expected a taskItem element');
    expect(item.nodeName).toBe('taskItem');
    expect(item.getAttributes()).toEqual({ checked: true });
  });

  it('keeps an unticked box unticked rather than letting it fall back to a default', () => {
    // `false` is the attribute's default, and a default is exactly what a round trip can
    // lose without anybody noticing — the page still renders, the box is simply empty. It
    // is also the direction that cannot be spotted by eye, because an unticked box is what
    // most boxes look like.
    const doc = {
      type: 'doc',
      content: [
        {
          type: 'taskList',
          content: [
            {
              type: 'taskItem',
              attrs: { checked: false },
              content: [{ type: 'paragraph', content: [{ type: 'text', text: 'offen' }] }]
            }
          ]
        }
      ]
    };
    const ydoc = prosemirrorJSONToYDoc(editorSchema, doc, CONTENT_FIELD);
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);
    const item = (fragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    expect(item.getAttributes()).toEqual({ checked: false });

    // …and back out again, which is the direction the reader and the exporter see.
    const back = yXmlFragmentToProseMirrorRootNode(fragment, editorSchema);
    expect(back.firstChild?.firstChild?.attrs.checked).toBe(false);
  });

  it("keeps a task's id through an edit, so the board does not shed the card", () => {
    // The attribute whose loss costs the most, and the one nothing on the page would show.
    // A task block carries a uuid in `attrs` — minted by the store during reconciliation on
    // publish, or by the editor when somebody types a new checkbox line — and that uuid is
    // the ONLY thing tying the line to its record: its status, its assignee, its due date.
    //
    // Stock `TaskItem` declares `checked` and nothing else. So an undeclared `id` takes the
    // documented path: `computeAttrs` never copies it into the ProseMirror node, and
    // `updateYFragment`'s closing pass — "remove all keys that are no longer in pAttrs" —
    // deletes it from the CRDT on the first edit that touches the item. The next publish
    // then sees a block with no id, mints a fresh one, and marks the ORIGINAL task
    // detached: the card leaves the board carrying its due date and its assignee with it,
    // once per edit, silently. This is the table-alignment near-miss again, with a worse
    // blast radius.
    const id = '0199c0de-0000-7000-8000-000000000001';
    const ydoc = serverWrittenChecklist({ checked: true, id });
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);

    const { doc, meta } = initProseMirrorDoc(fragment, editorSchema);
    expect(doc.firstChild?.firstChild?.attrs.id).toBe(id);

    // The edit. `updateYFragment` is what `@tiptap/extension-collaboration`'s sync plugin
    // runs on every transaction, so this is the real write-back and not a stand-in for one.
    const json = doc.toJSON();
    json.content[0].content[0].content[0].content[0].text = 'Milch und Brot kaufen';
    const edited = PmNode.fromJSON(editorSchema, json);
    ydoc.transact(() => updateYFragment(ydoc, fragment, edited, meta));

    const item = (fragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    expect(item.getAttributes()).toEqual({ checked: true, id });
    expect(fragment.toString()).toContain('Milch und Brot kaufen');
  });

  // --- placed files (D-15) --------------------------------------------------------------
  //
  // The kind with the least on screen to notice its loss by. A checklist that loses `checked`
  // still renders as a list; a placement that loses `filename` is a picture of nothing, with
  // nothing left anywhere to say which picture it had been — the name is half of the address
  // a download is authorised through (D-16) and the block carries no second copy of it.

  /**
   * A Y.Doc holding a placement, built the way `gw-collab::doc.rs::write_children` builds
   * one: an element tagged with the serde name of its `BlockKind`, its `attrs` set as XML
   * attributes, and no children at all.
   */
  function serverWrittenPlacement(attrs: Record<string, unknown>): Y.Doc {
    const ydoc = new Y.Doc();
    const placement = new Y.XmlElement('attachment');
    for (const [key, value] of Object.entries(attrs)) {
      placement.setAttribute(key, value as string);
    }
    const after = new Y.XmlElement('paragraph');
    const leaf = new Y.XmlText();
    leaf.insert(0, 'Und der Text danach.');
    after.insert(0, [leaf]);
    ydoc.getXmlFragment(CONTENT_FIELD).insert(0, [placement, after]);
    return ydoc;
  }

  it('does not delete a placed file from the CRDT when the editor opens the page', () => {
    // THE data-loss test for this kind, and the same one that had to be written for
    // `taskList`: before it was in this schema, opening a page holding one ran
    // `schema.node('attachment', …)`, threw, and deleted the element — broadcast to every
    // other editor and snapshotted into a revision by the next sweep. Nothing threw and
    // nothing was logged; the page simply came back one picture shorter.
    const ydoc = serverWrittenPlacement({ filename: 'befund.png', alt: 'Röntgenbild' });
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);

    const doc = yXmlFragmentToProseMirrorRootNode(fragment, editorSchema);

    // The Y.Doc still holds what the server put in it.
    expect(fragment.length).toBe(2);
    expect((fragment.get(0) as Y.XmlElement).nodeName).toBe('attachment');
    // And the editor really built the node, rather than skipping it into nothing.
    expect(doc.childCount).toBe(2);
    expect(doc.firstChild?.type.name).toBe('attachment');
    expect(doc.firstChild?.attrs).toEqual({ filename: 'befund.png', alt: 'Röntgenbild' });
    expect(doc.textBetween(0, doc.content.size, ' ')).toContain('Und der Text danach.');
  });

  it('keeps a placement\'s filename and description through an edit somewhere else', () => {
    // `updateYFragment` is what `@tiptap/extension-collaboration`'s sync plugin runs on every
    // transaction, so this is the real write-back. An attribute the schema did not declare
    // would be gone from the Y.Doc after this — the mechanism that nearly took a task's uuid
    // and table column alignment with it — and here it would take the only statement of which
    // file the page shows.
    const attrs = { filename: 'befund.png', alt: 'Röntgenbild, seitlich' };
    const ydoc = serverWrittenPlacement(attrs);
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);

    const { doc, meta } = initProseMirrorDoc(fragment, editorSchema);
    expect(doc.firstChild?.attrs).toEqual(attrs);

    const json = doc.toJSON();
    json.content[1].content[0].text = 'Und der Text danach, geändert.';
    const edited = PmNode.fromJSON(editorSchema, json);
    ydoc.transact(() => updateYFragment(ydoc, fragment, edited, meta));

    expect((fragment.get(0) as Y.XmlElement).getAttributes()).toEqual(attrs);
    expect(fragment.toString()).toContain('geändert');
  });

  it('writes a placement back under exactly the two attributes the importer states', () => {
    // The other half of the wire contract, and the half stock `Link` got wrong: whatever
    // ProseMirror's `computeAttrs` produces travels verbatim into `gw_core::Block::attrs`, and
    // `gw_api::export` compares a placement's attributes WHOLE — there is deliberately no
    // reduction forgiving them, unlike a link's or a task's. So a third attribute declared in
    // `extensions.ts` would be minted onto every placement and refuse the page on export,
    // permanently, on the owner's backup path.
    const ydoc = prosemirrorJSONToYDoc(
      editorSchema,
      {
        type: 'doc',
        content: [{ type: 'attachment', attrs: { filename: 'a.png', alt: 'x' } }]
      },
      CONTENT_FIELD
    );
    const placement = ydoc.getXmlFragment(CONTENT_FIELD).get(0);
    if (!(placement instanceof Y.XmlElement)) throw new Error('expected an attachment element');
    expect(placement.nodeName).toBe('attachment');
    expect(placement.getAttributes()).toEqual({ filename: 'a.png', alt: 'x' });
  });

  it('keeps an empty description empty rather than letting it fall back to null', () => {
    // `''` is the attribute's default and exactly what `gw_core::markdown` writes for
    // `![](anhang:a.png)`. A `null` default here would put `alt: null` on every placement the
    // editor touched, against the `alt: ""` the same file re-imports as — two values for one
    // document, and `render_file` refuses the page rather than choosing between them.
    const ydoc = prosemirrorJSONToYDoc(
      editorSchema,
      { type: 'doc', content: [{ type: 'attachment', attrs: { filename: 'a.png' } }] },
      CONTENT_FIELD
    );
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);
    expect((fragment.get(0) as Y.XmlElement).getAttributes()).toEqual({
      filename: 'a.png',
      alt: ''
    });
    const back = yXmlFragmentToProseMirrorRootNode(fragment, editorSchema);
    expect(back.firstChild?.attrs.alt).toBe('');
  });

  it('admits a placement in the document and nowhere the importer would not read one', () => {
    // The schema half of a rule whose other half is in `gw_core::markdown`. A placement in a
    // list item is a node ProseMirror cannot build, and `createNodeFromYElement` answers that
    // by deleting the element from the CRDT; one in a table cell or a blockquote exports to
    // markdown that re-imports as text, which refuses the page forever. `check()` is what
    // validates a content expression, and it throws rather than returning false.
    const top = PmNode.fromJSON(editorSchema, {
      type: 'doc',
      content: [{ type: 'attachment', attrs: { filename: 'a.png', alt: '' } }]
    });
    expect(() => top.check()).not.toThrow();

    for (const nested of [
      {
        type: 'bulletList',
        content: [
          {
            type: 'listItem',
            content: [{ type: 'attachment', attrs: { filename: 'a.png', alt: '' } }]
          }
        ]
      },
      {
        type: 'blockquote',
        content: [{ type: 'attachment', attrs: { filename: 'a.png', alt: '' } }]
      },
      {
        type: 'table',
        content: [
          {
            type: 'tableRow',
            content: [
              {
                type: 'tableCell',
                content: [{ type: 'attachment', attrs: { filename: 'a.png', alt: '' } }]
              }
            ]
          }
        ]
      }
    ]) {
      expect(
        () => PmNode.fromJSON(editorSchema, { type: 'doc', content: [nested] }).check(),
        `a placement inside a ${nested.type} must not be a valid document`
      ).toThrow();
    }
  });

  it('carries no mark, because the CRDT has nowhere to put one on an element', () => {
    // `gw-collab` writes a leaf's marks as Yjs TEXT formatting attributes
    // (`doc.rs::mark_key_of`) and has no representation for a mark on an element. A schema
    // that let one be applied would sync it, publish it, and lose it in silence.
    expect(editorSchema.nodes.attachment.spec.marks).toBe('');
  });

  it('mints no id of its own for a task that has none yet', () => {
    // The other direction, and it matters just as much: `gw_core::markdown` is a pure
    // function and gives an imported checkbox no id at all, because `gw_api::export`
    // re-imports its own output and compares — a randomly minted id would differ on every
    // run and refuse the page forever. An `id` DEFAULT that was anything but `null` would
    // put that same invented value on every task the editor touched.
    const ydoc = serverWrittenChecklist({ checked: false });
    const fragment = ydoc.getXmlFragment(CONTENT_FIELD);
    const { doc, meta } = initProseMirrorDoc(fragment, editorSchema);
    expect(doc.firstChild?.firstChild?.attrs.id).toBeNull();

    ydoc.transact(() => updateYFragment(ydoc, fragment, doc, meta));
    const item = (fragment.get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    expect(item.getAttributes()).toEqual({ checked: false });
  });
});
