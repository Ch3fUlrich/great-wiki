/**
 * The editor's node schema, and the one place it is allowed to be decided.
 *
 * # Why this file is the dangerous one
 *
 * `gw-collab` holds the live document as a Yjs XML fragment whose element tags are the
 * camelCase serde names of `gw_core::BlockKind`. TipTap builds a ProseMirror document out of
 * that fragment by looking each tag up in *this* schema. The two failure modes when they
 * disagree are both silent and both destructive, and both were read out of the installed
 * `@tiptap/y-tiptap` source rather than assumed:
 *
 * - **A tag this schema does not name is deleted from the CRDT.** `createNodeFromYElement`
 *   wraps `schema.node(el.nodeName, …)` in a `try`, and its `catch` runs
 *   `el._item.delete(transaction)`. The deletion is a normal CRDT operation: it is
 *   broadcast to every other editor and snapshotted into a revision by the next sweep.
 * - **An attribute this schema does not declare is deleted with it.** ProseMirror's
 *   `computeAttrs` only copies attributes the schema declares, and `updateYFragment` then
 *   removes from the Yjs element "all keys that are no longer in pAttrs".
 *
 * So the schema is not a matter of which features to offer. It is the set of things a page
 * can contain and still survive being opened in the editor. `extensions.test.ts` asserts it
 * against the server's kinds and against the attributes `gw-core::markdown` writes.
 *
 * # Why there are no marks
 *
 * `gw_core::Block` has four fields — `kind`, `attrs`, `content`, `text` — and none of them
 * holds inline formatting. Yjs *can* carry bold, italic and links; `CollabDoc::to_block`
 * keeps the text and drops the emphasis, because there is nowhere to put it (see the
 * `gw-collab` module docs, and M4, which closes this).
 *
 * That leaves two honest options: a toolbar with a warning, or no marks. This takes no
 * marks, because a warning is a thing a person reads once and an editor is a thing they use
 * every day — and because with the marks absent from the schema the editor is exactly as
 * expressive as a revision is. What is on screen is what will be stored. Ctrl+B does
 * nothing, pasted rich text arrives as plain text and is visibly plain immediately, and
 * nothing can be lost between typing and publishing.
 *
 * The editor still says so in words (see `Editor.svelte`), because "why is bold not
 * working" deserves an answer on the screen where it is not working.
 *
 * # Versions
 *
 * Pinned exactly, not with a caret. Two of the things this file depends on are undocumented
 * defaults of the packages themselves — `field: 'default'` in
 * `@tiptap/extension-collaboration`, and `align` being a declared attribute of
 * `@tiptap/extension-table`'s cells — and a minor release that changed either would lose
 * content with a green test suite, because the test asserts the schema, not the version.
 */
import { getSchema } from '@tiptap/core';
import type { Extensions } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import type { BlockKind } from '$lib/blocks/render';

/**
 * The Yjs fragment the document lives in.
 *
 * `gw-collab`'s `CONTENT_FIELD`, and NOT `@tiptap/extension-collaboration`'s default, which
 * is `'default'`. Getting this wrong is the quietest bug in the whole feature: both sides
 * work perfectly, against two different, permanently empty fragments of the same Y.Doc. No
 * error, no warning, an editor that opens blank and a page that never changes.
 */
export const CONTENT_FIELD = 'content';

/**
 * The kinds `gw_core::BlockKind` serialises to, which are also the CRDT's element tags.
 *
 * `satisfies readonly BlockKind[]` ties this to the renderer's mirror of the same enum, so
 * naming a kind that does not exist fails the type check. The `AssertNever` line below
 * covers the other direction: a kind added to `BlockKind` and forgotten here.
 */
export const SERVER_BLOCK_KINDS = [
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
] as const satisfies readonly BlockKind[];

/** Compile-time only: fails if `BlockKind` holds a kind `SERVER_BLOCK_KINDS` does not name. */
type AssertNever<T extends never> = T;
type _EveryKindIsNamed = AssertNever<Exclude<BlockKind, (typeof SERVER_BLOCK_KINDS)[number]>>;

/**
 * Every extension that contributes a node or a mark. Deliberately not the whole editor: the
 * collaboration extensions carry no schema and need a live Y.Doc, so they are added where
 * the editor is built and this list stays something a test can evaluate with no browser.
 *
 * Each `false` below removes something the server cannot store, and each is a node or mark
 * that would otherwise be lost — quietly on publish, or loudly by the deletion path above:
 *
 * - `hardBreak`, `horizontalRule` — no `BlockKind` for either. They would live in the CRDT
 *   and be dropped by `to_block`, taking the paragraph break or the rule with them.
 * - `bold`, `italic`, `strike`, `code`, `underline`, `link` — see the module docs.
 * - `undoRedo` — ProseMirror's own history is *wrong* under a CRDT: it would undo other
 *   people's edits along with your own. `Collaboration` installs a Yjs-aware undo manager
 *   scoped to this client, and registering both means two Ctrl+Z handlers fighting.
 * - `trailingNode` — it appends an empty paragraph to any document that does not end in
 *   one, which for a page ending in a table is a content change caused by *opening* the
 *   editor, and therefore an autosaved revision nobody asked for. `gapcursor` (kept) is the
 *   accessible way to put a caret after a trailing block.
 */
export function contentExtensions(): Extensions {
  return [
    StarterKit.configure({
      bold: false,
      italic: false,
      strike: false,
      code: false,
      underline: false,
      link: false,
      hardBreak: false,
      horizontalRule: false,
      undoRedo: false,
      trailingNode: false
    }),
    // `resizable` stays off: column resizing writes a `colwidth` attribute that `gw-core`
    // has no field for, and the reader's table renderer sizes columns itself.
    Table.configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell
  ];
}

/**
 * The schema those extensions actually produce.
 *
 * Built here rather than described, so the test asserts what ProseMirror will really do
 * with a tag and an attribute — not what this file claims it does.
 *
 * A fresh list each time it is asked for: TipTap's extension manager writes the editor back
 * onto every extension it resolves, so handing the same instances to this probe and to a
 * live editor would make one of them hold a reference to the other's editor.
 */
export const editorSchema = getSchema(contentExtensions());

/** The node names in that schema. Compared against the server's kinds by the test. */
export const EDITOR_NODE_NAMES: readonly string[] = Object.keys(editorSchema.nodes);
