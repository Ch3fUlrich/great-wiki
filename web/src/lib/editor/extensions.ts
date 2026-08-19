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
 * # Marks, and the one thing that makes them dangerous
 *
 * `gw_core::Block` grew a fifth field, `marks`, and `gw-collab` now writes and reads them —
 * see the `gw-collab` module docs and `crates/gw-core/src/block.rs::MarkKind`. So this file no
 * longer leaves marks out of the schema; it enables exactly the five the server can store:
 * `strong`, `em`, `code`, `strike`, `link`.
 *
 * The part that is easy to get wrong is *silent*, the same shape as the node-tag risk above,
 * and it was verified the same way — against the installed source, not assumed. `gw-collab`'s
 * `mark_key_of` keys a leaf's Yjs formatting attributes by `MarkKind`'s serde name (`strong`,
 * `em`, …). `@tiptap/y-tiptap`'s `marksToAttributes` keys the SAME attributes by the
 * ProseMirror mark's own type name (`pattrs[mark.type.name] = mark.attrs`, read out of the
 * installed `dist/y-tiptap.js`). TipTap's stock `Bold` and `Italic` extensions are named
 * `bold` and `italic` — not `strong`/`em` — which was proved by actually running
 * `prosemirrorJSONToYDoc` against the unmodified extensions before this file renamed them: it
 * wrote the attribute keys `bold` and `italic` onto the wire. `gw-collab::kind_of_mark_key`
 * does not recognise either one, so `attrs_to_marks` silently drops the mark and `to_block`
 * publishes the plain text — a bold word that types as bold, syncs as bold to every other
 * browser, and vanishes at the next publish with every test green, because nothing else
 * crosses that boundary. `Code`, `Strike` and `Link` already carry the right names by
 * coincidence (verified the same way); only `Bold` and `Italic` are renamed below.
 * `extensions.test.ts` pins the wire keys directly, not just the schema's mark names, so a
 * regression here fails loudly instead of at publish time on somebody's real edit.
 *
 * Marks NOT in this set — `underline` — stay off. `MarkKind` in `gw-core` does not have one;
 * enabling it would repeat the exact failure this section exists to prevent, just for a mark
 * the server can never be taught to keep.
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
import { Bold } from '@tiptap/extension-bold';
import { Italic } from '@tiptap/extension-italic';
import { Strike } from '@tiptap/extension-strike';
import { Code } from '@tiptap/extension-code';
import { Link } from '@tiptap/extension-link';
import type { BlockKind, MarkKind } from '$lib/blocks/render';

/**
 * TipTap's `Bold`/`Italic`, renamed to the Yjs attribute key `gw-collab::mark_key_of` expects.
 *
 * `extend({ name })` is TipTap's own supported mechanism for this — `Extendable.extend` sets
 * `extension.name` straight from the config, and every internal use of `this.name`
 * (`toggleBold`'s `commands.toggleMark(this.name)`, `isActive`, the schema mark-type name
 * itself) reads the new one. Nothing else about the extension changes: `Mod-B` still works,
 * pasted `<strong>` HTML still parses in via `parseHTML`'s own tag list, which does not
 * depend on `this.name` at all.
 */
// One more thing about `extend({ name })`, inert today and destructive the day it is not:
// `Bold`'s and `Italic`'s `parseMarkdown` hooks call `helpers.applyMark("bold")` and
// `("italic")` — string literals, not `this.name` (verified in the installed
// `@tiptap/extension-bold@3.30.0`; `Strike`, `Code` and `Link` hardcode names that happen to
// still be right). Nothing imports `@tiptap/markdown`, so no code path reaches them. A later
// task wiring TipTap's markdown pipeline in would find bold and italic silently missing from
// every parsed document, and this rename is why. Override `parseMarkdown` alongside `name` if
// that day comes.
const Strong = Bold.extend({ name: 'strong' });
const Em = Italic.extend({ name: 'em' });

/**
 * TipTap's `Link`, trimmed to the ONE attribute `gw_core::Mark` has any use for.
 *
 * Stock `Link.addAttributes()` declares five — `href`, plus `target` (default `_blank`),
 * `rel` (default `noopener noreferrer nofollow`), `class` and `title` (both `null`). The
 * module docs above explain that an attribute this schema does not declare is deleted; this
 * is the mirror-image hazard, and it cost more: an attribute the schema DOES declare is
 * *created*. ProseMirror's `computeAttrs` fills every declared default in,
 * `marksToAttributes` writes `pattrs[mark.type.name] = mark.attrs` — the whole map — onto the
 * wire, and `gw-collab::attrs_to_marks` copies the whole map verbatim into `Mark::attrs`. So a
 * link stored `{href}` by the markdown importer becomes `{href, target, rel, class, title}`
 * the first time anybody edits the paragraph it sits in (y-tiptap's `equalAttrs` sees 1 key
 * against 5 and rewrites the Y.Doc).
 *
 * Nothing reads those four. `BlockView` renders its own fixed `rel` and no `target`, and
 * `gw-core::markdown` has no syntax for any of them. What they did do is fail `gw-api`'s
 * export: `render_file` compares the document against what its own markdown re-imports as,
 * `[text](href)` comes back as `{href}` alone, the trees differ, the page is refused — and
 * `export` bails on the first refusal, so one link made the whole wiki unexportable. That is
 * the owner's backup path, and `FIDELITY_WARNING` promises links survive it.
 *
 * `Link.configure({ HTMLAttributes: {…} })` is NOT this fix and was tried: it changes what
 * `renderHTML` puts in the editor's own DOM, while the declared attributes — the ones that
 * reach the CRDT — stay exactly as they were. Only the declaration matters here.
 *
 * `parseHTML` is carried over from the stock declaration rather than left to TipTap's
 * default, which is `fromString(element.getAttribute(name))` and would coerce an href like
 * `"2024"` into the number 2024. `target`/`rel` are still emitted into the editor's DOM by
 * `renderHTML`, which merges `options.HTMLAttributes` — dropping the attributes does not drop
 * the protection on the editor's own rendered anchors.
 */
const Anchor = Link.extend({
  addAttributes: () => ({
    href: { default: null, parseHTML: (element: HTMLElement) => element.getAttribute('href') }
  })
});

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
 * - `bold`, `italic`, `strike`, `code`, `link` — StarterKit's own bundled marks stay off so
 *   the renamed `Strong`/`Em` below and the standalone `Strike`/`Code`/`Link` are each added
 *   exactly once, under one name, rather than StarterKit's registering `bold` a second time
 *   alongside `Strong` and the two silently fighting over the same keyboard shortcut.
 * - `underline` — no `MarkKind` for it. See the module docs' last paragraph.
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
    TableCell,
    // The five marks the server can store, `Strong`/`Em` renamed per the module docs above;
    // `Strike`, `Code` and `Anchor` keep TipTap's own names because those already agree with
    // `MarkKind`'s serde names. `Anchor` is `Link` with its attribute declaration trimmed to
    // `href` — see its doc comment; the four it dropped are what made every page holding a
    // link refuse to export.
    Strong,
    Em,
    Strike,
    Code,
    Anchor
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

/**
 * The mark kinds `gw_core::MarkKind` serialises to — same construction as
 * `SERVER_BLOCK_KINDS`, and the same reason: `satisfies readonly MarkKind[]` fails the type
 * check if a kind is named here that `MarkKind` does not have, and `_EveryMarkIsNamed` below
 * fails it in the other direction, if `MarkKind` grows one this file forgets.
 */
export const SERVER_MARK_KINDS = [
  'strong',
  'em',
  'code',
  'strike',
  'link'
] as const satisfies readonly MarkKind[];

/** Compile-time only: fails if `MarkKind` holds a kind `SERVER_MARK_KINDS` does not name. */
type _EveryMarkIsNamed = AssertNever<Exclude<MarkKind, (typeof SERVER_MARK_KINDS)[number]>>;

/** The mark names in that schema. Compared against the server's kinds by the test. */
export const EDITOR_MARK_NAMES: readonly string[] = Object.keys(editorSchema.marks);
