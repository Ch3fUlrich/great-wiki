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
 * Pinned exactly, not with a caret. Three of the things this file depends on are
 * undocumented defaults of the packages themselves — `field: 'default'` in
 * `@tiptap/extension-collaboration`, `align` being a declared attribute of
 * `@tiptap/extension-table`'s cells, and `checked` being one of `@tiptap/extension-list`'s
 * `TaskItem` — and a minor release that changed any of them would lose content with a green
 * test suite, because the test asserts the schema, not the version.
 *
 * Six of the packages imported below are not named in `package.json`: `extension-bold`,
 * `-italic`, `-strike`, `-code`, `-link` and `-list` all arrive as `@tiptap/starter-kit`'s
 * own dependencies, pinned by it to the same exact 3.30.0 (checked in `package-lock.json`,
 * not assumed). They are therefore as pinned as the declared ones, and adding them to
 * `package.json` would only give the resolver a second place to disagree with itself.
 */
import { getSchema, Node } from '@tiptap/core';
import type { Extensions } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { Document } from '@tiptap/extension-document';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import { TaskItem, TaskList } from '@tiptap/extension-list';
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
 * TipTap's `TaskItem`, taught the ONE attribute the stock extension does not know about.
 *
 * A task block carries a uuid in `attrs` beside `checked` — minted by the store during
 * reconciliation on publish, or here, when somebody types a new checkbox line — and that
 * uuid is the only thing tying the line to its record: its status, its assignee, its due
 * date. Nothing on the page shows it, which is exactly what makes losing it invisible.
 *
 * Stock `TaskItem.addAttributes()` returns `{ checked }` and nothing else (read out of the
 * installed `@tiptap/extension-list@3.30.0`). The module docs above say what happens to an
 * attribute this schema does not declare, and it is not "ignored": `computeAttrs` never
 * copies it into the ProseMirror node, and `updateYFragment`'s closing pass — "remove all
 * keys that are no longer in pAttrs" — deletes it from the Y.Doc on the first edit that
 * touches the item. The next publish would then find a block with no id, mint a fresh one,
 * and mark the ORIGINAL task detached. The board would shed a card, carrying its due date
 * and its assignee away with it, once per edit, in silence. This is the same mechanism that
 * nearly destroyed table column alignment across 21 tables, with a worse blast radius,
 * because a detached task is not visibly a bug — it is a state the design has a name for.
 *
 * `this.parent?.()` rather than a fresh literal, so `checked` keeps the stock declaration
 * — its `parseHTML` reads `data-checked`, and re-stating that here would be a second copy
 * to drift.
 *
 * Three deliberate choices about `id` itself:
 *
 * - **`default: null`.** Never a generated value. `gw_core::markdown` is a pure function
 *   and gives an imported checkbox no id at all, because `gw_api::export` re-imports its
 *   own output and compares the trees — an id invented per render would differ on every run
 *   and refuse the page forever. A `null` default costs nothing on the wire either:
 *   y-tiptap skips a null attribute when it creates an element and removes it on write-back,
 *   so a task with no id yet writes no `id` key.
 * - **`keepOnSplit: false`**, the same as `checked`. Pressing Enter at the end of a task
 *   line makes a NEW task; carrying the id across would give two blocks one identity, and
 *   reconciliation would have to pick a winner between them.
 * - **`rendered: false`.** The id is database identity, not markup. It stays in the schema
 *   (which is all the CRDT needs) and out of the editor's DOM, so it cannot collide with a
 *   heading's anchor id and cannot be smuggled in by pasted HTML claiming to be a task that
 *   already exists.
 */
const Task = TaskItem.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      id: { default: null, keepOnSplit: false, rendered: false }
    };
  }
});

/**
 * A file placed in the prose (D-15): `gw_core::BlockKind::Attachment`.
 *
 * # Two attributes, and no third
 *
 * `filename` and `alt`, which is exactly what `gw_core::markdown` writes and exactly what
 * `gw_api::export` compares. Both hazards the module docs above describe apply here at once
 * and in opposite directions:
 *
 * - **Undeclared is deleted.** `filename` is half of the address a download is authorised
 *   through (D-16) and the only thing in the block that says which file it is. An attribute
 *   this schema does not declare is removed from the Y.Doc on the first edit that touches the
 *   node, so a placement would quietly become a picture of nothing — with no way left to say
 *   which picture it had been.
 * - **Over-declared is minted.** There is deliberately no reduction for a placement in
 *   `gw_api::export` (see `a_placement_is_compared_with_its_attributes_whole_…` in
 *   `crates/gw-api/tests/export.rs`): its attributes are compared whole, so a third one
 *   declared here would be filled in with its default by ProseMirror's `computeAttrs`,
 *   written onto the wire by `marksToAttributes`, copied into `Block::attrs`, and would then
 *   refuse the page on export — permanently, and on the owner's backup path. That is exactly
 *   what stock `Link`'s four extra attributes really did.
 *
 * `alt` defaults to `''` rather than `null`, because the importer writes it even when it is
 * empty and the two sides have to agree: a `null` default would put `alt: null` on every
 * placement the editor touched, against the `alt: ""` markdown re-imports as.
 *
 * `rendered: false` on neither, unlike a task's `id`: both are document content rather than
 * database identity, and both belong in the editor's own DOM so the author can see which
 * file is placed.
 *
 * # Its own group, so it can only stand where the importer will read one back
 *
 * `group: 'attachment'` and NOT `'block'`, with `Doc` below widening only the document's
 * content expression to admit it. That is not tidiness — it is the schema half of a rule
 * whose other half is in `gw_core::markdown`, and the two must agree exactly:
 *
 * - A placement is written as an image standing alone in its own paragraph, and the importer
 *   reads one back **only at the top level of the document**. A placement anywhere else
 *   exports to markdown that re-imports as a paragraph of text, so `render_file` refuses the
 *   page and `export` fails the whole run on the first refusal.
 * - `listItem`'s content expression is `paragraph block*`, so a placement as an item's first
 *   child is a node ProseMirror cannot build — and `createNodeFromYElement` answers that by
 *   **deleting the element from the CRDT**, which is the silent destruction the module docs
 *   above exist to prevent.
 * - A `tableCell` in markdown is one paragraph and nothing else, so a placement in one is a
 *   page the exporter refuses outright.
 *
 * With `group: 'block'` the editor would happily let somebody drag a picture into a list item
 * or a table cell and produce any of those. With its own group the schema simply will not
 * hold it, which is the only kind of prevention that does not depend on remembering.
 *
 * # `atom`, `draggable`, and no marks
 *
 * `atom: true` because it has no content to edit — everything about it is in its attributes —
 * and `content` is therefore left unset. `draggable: true` so it can be moved without being
 * cut and re-inserted. `marks: ''` because `gw-collab` writes a leaf's marks as Yjs *text*
 * formatting attributes (`doc.rs::mark_key_of`) and has nowhere to put a mark on an element:
 * a bolded placement would sync, publish, and lose the mark in silence.
 */
const Attachment = Node.create({
  name: 'attachment',
  group: 'attachment',
  atom: true,
  draggable: true,
  marks: '',
  addAttributes: () => ({
    filename: {
      default: '',
      parseHTML: (element: HTMLElement) => element.getAttribute('data-filename') ?? ''
    },
    alt: { default: '', parseHTML: (element: HTMLElement) => element.getAttribute('data-alt') ?? '' }
  }),
  // `data-*` rather than `src`/`alt` on a real `<img>`, and that is the safe choice rather
  // than the lazy one. The editor has no permission-checked address to put in a `src` — the
  // one the reader uses is built by the API for a list this component never fetched — and a
  // node view that guessed one would be this interface assembling an address, which D-16 says
  // it may not. So the editor shows WHICH file is placed, in words, and the reader shows the
  // picture. `parseHTML` matches the same attribute, so copying a placement inside the editor
  // keeps it.
  parseHTML: () => [{ tag: 'figure[data-attachment]' }],
  renderHTML: ({ HTMLAttributes }) => [
    'figure',
    {
      'data-attachment': '',
      'data-filename': HTMLAttributes.filename,
      'data-alt': HTMLAttributes.alt,
      class: 'gw-ed-datei'
    },
    ['figcaption', {}, `📎 ${HTMLAttributes.filename}${HTMLAttributes.alt ? ` — ${HTMLAttributes.alt}` : ''}`]
  ]
});

/**
 * TipTap's `Document`, widened by exactly one group so a placement has somewhere to go.
 *
 * `content: 'block+'` is the stock expression, and `attachment` is deliberately not in the
 * `block` group — see `Attachment` above for the three ways that would lose or refuse a
 * page. This is the *only* place it is admitted, and `StarterKit.configure({ document: false })`
 * below is what stops the stock one being registered alongside it.
 *
 * `@tiptap/extension-document` is not named in `package.json`. It arrives as
 * `@tiptap/starter-kit`'s own dependency, pinned by it to the same exact 3.30.0 (checked in
 * `package-lock.json`), exactly as the six marks imported above do.
 */
const Doc = Document.extend({ content: '(block|attachment)+' });

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
    // `Doc` above replaces it, widened to admit a placement. Everything else about the stock
    // Document is kept by extending it rather than writing a new one.
    Doc,
    StarterKit.configure({
      document: false,
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
    // The checklist. `StarterKit` does not bundle these — it registers `BulletList`,
    // `OrderedList`, `ListItem` and `ListKeymap` out of `@tiptap/extension-list` and leaves
    // `TaskList`/`TaskItem` alone — so they are named here, out of the same package, which
    // is already installed as `StarterKit`'s own dependency at the same pinned 3.30.0.
    //
    // Their node names are already `taskList` and `taskItem`, which is the whole reason
    // `gw_core::BlockKind` spells the kinds that way: this enum mirrors the editor so that
    // nothing has to be translated between them. No `extend({ name })` here, unlike
    // `Bold`/`Italic` — but that is a fact to be checked rather than assumed, and
    // `extensions.test.ts` pins both names against the server's list.
    TaskList,
    // `nested: true` widens `taskItem`'s content expression from `paragraph+` to
    // `paragraph block*`, and it is not optional. `gw_core::markdown` imports
    // `- [ ] a` / `  - [x] b` as a `taskList` INSIDE a `taskItem`, and a schema that cannot
    // express that does not merely refuse it: `createNodeFromYElement` catches the
    // `RangeError` from `schema.node(…)` and deletes the element from the Y.Doc, which is
    // the same silent destruction an unknown tag causes. A checklist with a nested
    // checklist is one keystroke away in the editor and one line away in imported markdown.
    //
    // `checked` is declared by the stock extension already — verified in the installed
    // `@tiptap/extension-list@3.30.0`, where `addAttributes` returns exactly `{ checked }`
    // and nothing else. That matters in both directions and is pinned by a test either
    // way: undeclared, `updateYFragment` would delete it from the CRDT and every box would
    // quietly untick itself one edited item at a time (the shape of the near-miss on table
    // column alignment); over-declared, the extra keys would be minted into `Block::attrs`
    // and `gw-api::export` would refuse the page, the way stock `Link`'s four extra
    // attributes really did (see `Anchor` above).
    //
    // The stock declaration is exactly one attribute short, and `Task` above adds it: a
    // task's `id`, which is what the board holds its record by. See its doc comment.
    Task.configure({ nested: true }),
    // A file placed in the prose. Its own group, admitted only by `Doc` above — see its doc
    // comment for why `block` would be three different kinds of data loss.
    Attachment,
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
