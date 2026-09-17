<!--
  The block and mark controls.

  # Marks are here now, because a revision can finally keep them

  `gw_core::Block` grew a `marks` field and `gw-collab` writes and reads it, so `extensions.ts`
  now enables exactly the five marks `MarkKind` has — and every one of those five gets a
  control below, alongside the block controls. Every control here maps one-to-one onto
  something the server can store, so pressing any of them is lossless — which is also why
  there is nothing here for underline: no `MarkKind` for it, so a control would be the exact
  control-whose-effect-disappears-at-publish problem this toolbar existed to avoid before
  Task 5, just for a different mark.

  # Ark's ToggleGroup rather than a row of buttons

  These are not commands, they are states: "this block IS a heading of level 2" is exactly
  what `aria-pressed` describes, and toggling it off returns a paragraph. Ark gives the
  roving tabindex, the arrow keys, Home/End and the pressed state; the pressed state is then
  overwritten from the editor on every transaction, so the toolbar can never disagree with
  the caret — which is the failure a toolbar holding its own state always eventually has.
-->
<script lang="ts">
  import { ToggleGroup } from '@ark-ui/svelte/toggle-group';
  import type { Editor } from '@tiptap/core';
  import { safeHref } from '$lib/blocks/render';
  import { kindText, type Attachment } from '$lib/attachments';
  import { normalizeLinkAddress } from './linkAddress';
  import LinkDialog from './LinkDialog.svelte';
import EmbedDialog from './EmbedDialog.svelte';
  import type { PageChoice } from './pagePicker';
  import type { TreeNode } from '$lib/api';

  interface Props {
    /** `null` until the session is live and the surface exists. */
    editor: Editor | null;
    /** Whether a command could reach anywhere. False in every dead-end session state. */
    enabled: boolean;
    /**
     * The document's own stored path, leading slash included — same value `Editor.svelte`
     * has as its `path` prop. What the Link control resolves a relative address against
     * (`normalizeLinkAddress`), so that what gets stored already names the page a click
     * actually lands on rather than leaving that for `gw_store::links::wiki_path` to guess.
     */
    path: string;
    /**
     * What is attached to this page, as `GET /api/attachments/{path}` answered it — the only
     * things that can be placed in the text.
     *
     * **Handed down from the page rather than fetched here**, so there is one answer to
     * "what does this page carry" and one authorisation behind it; a second request would be
     * a second retrieval path for something already read, which is what AGENTS.md rule 2
     * warns about even where the answer would agree.
     *
     * **The list is also the whole of the control.** D-15 makes the `Anhänge` list the
     * authority on what is attached and the inline block a reference to it, so placing a
     * file is *choosing one from the list* — never typing a name, which would let somebody
     * write a reference to a file that is not there without ever being told.
     */
    anhaenge?: Attachment[];
    /**
     * The pages this caller may read, as the shell already asked for them — what the link
     * dialog's picker offers.
     *
     * **Handed down rather than fetched**, for `anhaenge`'s reason and one that is sharper
     * here: an unfiltered page listing is a whole-corpus existence-and-title oracle for
     * exactly the person in the threat model, somebody with write on one page who wants to
     * know whether `/darm/befund-mueller` exists. `GET /api/tree` is `Store::tree_for`,
     * already filtered per document through the same `can()` a page read goes through, and
     * building the picker on it means there is no second listing to filter differently.
     */
    seiten?: TreeNode[];
  }

  let { editor, enabled, path, anhaenge = [], seiten = [] }: Props = $props();

  /** Whether the link dialog is open. */
  let linkDialog = $state(false);

  /** Whether the embed dialog is open. */
  let embedDialog = $state(false);

  /**
   * Put a live view of another page where the caret is (D-27).
   *
   * A command rather than a toggle, exactly as `place` below is, and for the same reason:
   * "this block IS a heading" is a state a toggle can describe and "quote that page here" is
   * not. So it sits outside the ToggleGroup and outside `CONTROLS`.
   *
   * What is stored is the target's **identity** and, for a section, the heading's **stable
   * id** — never a path and never a slug. The dialog already has both: the picker's `id`
   * comes from the tree, and the anchor comes from the target's own body. Nothing here
   * resolves anything, for `linkToPage`'s reason: a client that turned an address into an id
   * would be a second answer to a permission question.
   *
   * The label defaults to the page's CURRENT title — the same snapshot `linkToPage` takes of
   * a link's text, and the same argument: the label is document content and has to round-trip
   * through markdown, while the title in the frame is resolved afresh on every read. A
   * section embed's label names the section, because that is what was quoted.
   */
  function embed(e: Editor, wahl: { seite: PageChoice; abschnitt: { text: string; anchor: string } | null }) {
    e.chain()
      .focus()
      .insertContent({
        type: 'embed',
        attrs: {
          doc: wahl.seite.id,
          heading: wahl.abschnitt?.anchor ?? null,
          label: wahl.abschnitt?.text ?? wahl.seite.title
        }
      })
      .run();
  }

  /**
   * Store the chosen page's IDENTITY, not its address (D-5).
   *
   * `setMark` rather than TipTap's own `setLink`: that command reads `href` out of what it is
   * handed and refuses anything its URI check does not like, and a document reference has no
   * `href` at all. `preventAutolink` is set exactly as `setLink` sets it — without it the Link
   * extension's autolink plugin can rewrite what was just written as soon as the next
   * character is typed.
   *
   * With nothing selected there is no text for the mark to sit on, so the page's CURRENT
   * title is inserted as the link text. That is D-5's "the link text follows the title",
   * realised at the one moment it can be: the text is document content and has to round-trip
   * through markdown, so it is a snapshot of the title, while the ADDRESS the reader lands on
   * is resolved afresh on every read and does follow the page.
   */
  function linkToPage(e: Editor, seite: PageChoice) {
    if (e.state.selection.empty) {
      e.chain()
        .focus()
        .insertContent({
          type: 'text',
          text: seite.title,
          marks: [{ type: 'link', attrs: { doc: seite.id } }]
        })
        .setMeta('preventAutolink', true)
        .run();
      return;
    }
    e.chain()
      .focus()
      .extendMarkRange('link')
      .setMark('link', { doc: seite.id })
      .setMeta('preventAutolink', true)
      .run();
  }

  /**
   * Store what was typed, as an address.
   *
   * Normalised at the one moment this code still knows two things `gw_store::links::wiki_path`
   * deliberately does not: the browser's own origin, and the path of the page this link is
   * being written on (`linkAddress.ts`). A same-origin absolute address (paste-the-address-bar)
   * becomes its path; a relative one is resolved against THIS page rather than left for the
   * server to root-anchor against the site root, which named the wrong page for anything
   * without a leading slash.
   *
   * It is NOT resolved to a document id here. `Store::resolve_references` does that on publish,
   * against the author's own permissions, and a second resolver in the client would be a second
   * answer to a permission question.
   */
  function linkToAddress(e: Editor, typed: string) {
    const normalized = normalizeLinkAddress(location.origin, path, typed);
    // The renderer refuses to build an `<a>` for anything but http/https/mailto —
    // `javascript:` in an href is stored XSS against every reader, and `BlockView` is where
    // that is stopped for ALL writers, not just this one. Checking the same rule here as
    // well is not the security boundary; it is the difference between being told and
    // watching a link silently come out as plain text on the published page.
    const href = safeHref(normalized);
    if (href === null) {
      window.alert(
        'Diese Adresse wird nicht verlinkt. Erlaubt sind nur Adressen, die mit ' +
          'http://, https:// oder mailto: beginnen.'
      );
      return;
    }
    e.chain().focus().setLink({ href }).run();
  }

  /**
   * One entry per control: what it is called, whether it is on, and what pressing it does.
   *
   * A table rather than markup per button, so that "the toolbar offers exactly these" is
   * one list somebody can read against `BlockKind` — which is the check that matters, and
   * the one that is impossible to do against eight near-identical blocks of markup.
   */
  const CONTROLS = [
    {
      id: 'h2',
      label: 'Überschrift 2',
      short: 'H2',
      on: (e: Editor) => e.isActive('heading', { level: 2 }),
      run: (e: Editor) => e.chain().focus().toggleHeading({ level: 2 }).run()
    },
    {
      id: 'h3',
      label: 'Überschrift 3',
      short: 'H3',
      on: (e: Editor) => e.isActive('heading', { level: 3 }),
      run: (e: Editor) => e.chain().focus().toggleHeading({ level: 3 }).run()
    },
    {
      id: 'h4',
      label: 'Überschrift 4',
      short: 'H4',
      on: (e: Editor) => e.isActive('heading', { level: 4 }),
      run: (e: Editor) => e.chain().focus().toggleHeading({ level: 4 }).run()
    },
    {
      id: 'bulletList',
      label: 'Aufzählung',
      short: '• Liste',
      on: (e: Editor) => e.isActive('bulletList'),
      run: (e: Editor) => e.chain().focus().toggleBulletList().run()
    },
    {
      id: 'orderedList',
      label: 'Nummerierte Liste',
      short: '1. Liste',
      on: (e: Editor) => e.isActive('orderedList'),
      run: (e: Editor) => e.chain().focus().toggleOrderedList().run()
    },
    {
      id: 'blockquote',
      label: 'Zitat',
      short: 'Zitat',
      on: (e: Editor) => e.isActive('blockquote'),
      run: (e: Editor) => e.chain().focus().toggleBlockquote().run()
    },
    {
      id: 'codeBlock',
      label: 'Codeblock',
      // Spelt out rather than shortened to "Code": the `code` MARK control sits four
      // buttons along and its own short label is "Code", so two adjacent buttons read the
      // same word and only a tooltip told them apart. The person using this is not a
      // programmer, and "the one that makes a whole block" versus "the one that marks a
      // word" is not a distinction a hover title should be carrying alone.
      short: 'Codeblock',
      on: (e: Editor) => e.isActive('codeBlock'),
      run: (e: Editor) => e.chain().focus().toggleCodeBlock().run()
    },
    // The five marks `extensions.ts` enables, addressed by the same name everywhere: the
    // Yjs attribute key `gw-collab` reads, the ProseMirror mark type name (`Bold`/`Italic`
    // are renamed to these in `extensions.ts` for exactly this reason), and the id here.
    {
      id: 'strong',
      label: 'Fett',
      short: 'Fett',
      on: (e: Editor) => e.isActive('strong'),
      run: (e: Editor) => e.chain().focus().toggleMark('strong').run()
    },
    {
      id: 'em',
      label: 'Kursiv',
      short: 'Kursiv',
      on: (e: Editor) => e.isActive('em'),
      run: (e: Editor) => e.chain().focus().toggleMark('em').run()
    },
    {
      id: 'code',
      label: 'Code',
      short: 'Code',
      on: (e: Editor) => e.isActive('code'),
      run: (e: Editor) => e.chain().focus().toggleMark('code').run()
    },
    {
      id: 'strike',
      label: 'Durchgestrichen',
      // Shorter than the full term, which was wider than every block control's own label
      // and pushed the row into a second line on a narrow window. The accessible name stays
      // "Durchgestrichen" and contains this word, so WCAG's label-in-name still holds.
      short: 'Gestrichen',
      on: (e: Editor) => e.isActive('strike'),
      run: (e: Editor) => e.chain().focus().toggleMark('strike').run()
    },
    {
      id: 'link',
      label: 'Link',
      short: 'Link',
      on: (e: Editor) => e.isActive('link'),
      // The one control here that is not a bare toggle: turning a link ON needs a
      // destination from somewhere, and every other control in this table needs nothing
      // beyond "is it on". It opens `LinkDialog`, which offers both destinations this system
      // has — a page of this wiki, stored by IDENTITY (D-5), and an address — because they
      // are two different acts and only one of them survives the target being moved.
      //
      // The dialog opens whether or not the caret is already in a link, and OFFERS the
      // removal rather than performing it. Pressing a pressed toggle and having it stay
      // pressed would be a lie; `apply` below puts the button back where the editor says it
      // is, one microtask later, for exactly that reason.
      run: (e: Editor) => {
        void e;
        linkDialog = true;
      }
    }
  ] as const;

  /**
   * Put a file into the text where the caret is (D-15).
   *
   * A command rather than a toggle, which is why it is not in `CONTROLS` above and why the
   * buttons sit outside the ToggleGroup: "this block IS a heading" is a state a toggle can
   * describe, and "insert a picture here" is not one.
   *
   * The description is asked for with `window.prompt`, exactly as the Link control asks for
   * an address, and for the same reason and with the same honesty: this is a toolbar, not a
   * media dialogue, and `place` is the only thing that would change the day somebody builds
   * one. Cancelling inserts nothing. An empty answer inserts a placement with an empty
   * description, which is a real state the importer also produces (`![](anhang:a.png)`) —
   * the reader falls back to the filename rather than leaving the picture unnamed.
   */
  function place(e: Editor, anhang: Attachment) {
    const typed = window.prompt(
      `Wie würden Sie »${anhang.filename}« jemandem beschreiben, der die Datei nicht sehen ` +
        'kann? Der Text steht später als Alternativtext am Bild. (Kann leer bleiben.)',
      ''
    );
    if (typed === null) return;
    e.chain()
      .focus()
      .insertContent({
        type: 'attachment',
        attrs: { filename: anhang.filename, alt: typed.trim() }
      })
      .run();
  }

  let active = $state<string[]>([]);

  /** Read the pressed set out of the editor, which is the only thing that knows it. */
  function readActive(e: Editor) {
    active = CONTROLS.filter((control) => control.on(e)).map((control) => control.id);
  }

  // Subscribed rather than polled, and re-subscribed when the editor is finally built.
  $effect(() => {
    const e = editor;
    if (!e) {
      active = [];
      return;
    }
    const update = () => readActive(e);
    update();
    e.on('transaction', update);
    return () => {
      e.off('transaction', update);
    };
  });

  /**
   * Ark reports the whole new set, not which button was pressed, so the difference against
   * what the editor last said is the press. There is always exactly one — Ark changes one
   * item per interaction — and running a command for anything else would fight the editor.
   */
  function apply(next: string[]) {
    const e = editor;
    if (!e) return;
    const previous = CONTROLS.filter((control) => control.on(e)).map((control) => control.id);
    const changed = CONTROLS.find(
      (control) => next.includes(control.id) !== previous.includes(control.id)
    );
    changed?.run(e);

    // A press that changes nothing must not leave the button looking pressed. `run` can
    // decline — the Link control's prompt is cancelled, or the address is refused — and a
    // command that dispatches no transaction fires no `transaction` event, so the
    // subscription above never corrects anything; the header's promise that the toolbar
    // cannot disagree with the caret was exactly one `return` short of true.
    //
    // A microtask, not a straight call, and that is not defensive: Ark's own root runs
    // `onValueChange(details) { props.onValueChange?.(details); if (value !== undefined)
    // value = details.value }` (read out of the installed
    // `@ark-ui/svelte/dist/components/toggle-group/toggle-group-root.svelte`), so it assigns
    // the press it THINKS happened through `bind:value` the instant this function returns.
    // Correcting `active` here would be overwritten a line later. One microtask on, that
    // assignment has happened, and `value` is a controlled prop of the underlying machine —
    // so writing the editor's truth into it moves the buttons.
    queueMicrotask(() => {
      if (editor === e) readActive(e);
    });
  }
</script>

<!-- `role="toolbar"` on the wrapper and the group inside it: Ark's own root is a `group`
     (it is a set of toggles), and the toolbar role is what tells a screen reader that the
     arrow keys move within it rather than out of it. Zag's own item keymap looks for a
     `[role=toolbar]` ancestor, so this is the arrangement it is built for. -->
<div role="toolbar" aria-label="Textbausteine" aria-controls="gw-ed-surface" class="gw-ed-tools">
  <ToggleGroup.Root
    multiple
    bind:value={active}
    onValueChange={(details) => apply(details.value)}
    class="gw-ed-toolgroup"
  >
    {#each CONTROLS as control (control.id)}
      <ToggleGroup.Item
        value={control.id}
        disabled={!enabled}
        aria-label={control.label}
        title={control.label}
        class="gw-ed-tool"
      >
        {control.short}
      </ToggleGroup.Item>
    {/each}
  </ToggleGroup.Root>
</div>

<!-- Rendered beside the toolbar rather than inside it: a `<dialog>` inside a `role="toolbar"`
     would put a whole form into a roving-tabindex set of toggles. It is in the DOM at all
     times and shown with `showModal()`, which is what supplies the focus trap and Escape. -->
<EmbedDialog
  bind:offen={embedDialog}
  {seiten}
  onEinbetten={(wahl) => editor && embed(editor, wahl)}
/>

<LinkDialog
  bind:offen={linkDialog}
  {seiten}
  verknuepft={active.includes('link')}
  onSeite={(seite) => editor && linkToPage(editor, seite)}
  onAdresse={(adresse) => editor && linkToAddress(editor, adresse)}
  onEntfernen={() => editor?.chain().focus().unsetLink().run()}
/>

<!-- The files this page carries, each one a button that puts it where the caret is.
     Deliberately NOT inside the toolbar above: those are toggles under Ark's roving
     tabindex, these are commands, and a screen reader arrowing through a set of states
     should not land on an action.

     Rendered only when there is something to place. A row saying »Keine Dateien« on every
     page in the wiki is furniture paid for by everybody who never attached one — and the
     `Anhänge` section under the page is where a file is added, which is also where somebody
     who came looking for this will already be.

     The names are the files' own, from the list the page read. Nothing here builds an
     address and nothing here can name a file that is not attached: a placement is a
     reference to a row in that list (D-15), and choosing from it is what keeps the two in
     step. -->
<!-- Quoting another page, beside the file buttons and outside the toolbar: a command, not a
     toggle, so a screen reader arrowing through a set of states never lands on an action.

     Always offered, unlike the file row, because every wiki has other pages and none of them
     has to be prepared first — and because the dialog is where the page is chosen, so there
     is nothing to list here that could be empty. -->
<div class="gw-ed-dateien">
  <p id="gw-ed-einbetten-label">Andere Seite</p>
  <div class="gw-ed-dateiliste" role="group" aria-labelledby="gw-ed-einbetten-label">
    <button
      type="button"
      class="gw-ed-datei-btn"
      disabled={!enabled}
      title="Eine andere Seite oder einen ihrer Abschnitte hier einbetten"
      onclick={() => (embedDialog = true)}
    >
      Seite einbetten
    </button>
  </div>
</div>

{#if anhaenge.length > 0}
  <div class="gw-ed-dateien">
    <p id="gw-ed-dateien-label">Datei einfügen</p>
    <div class="gw-ed-dateiliste" role="group" aria-labelledby="gw-ed-dateien-label">
      {#each anhaenge as anhang (anhang.filename)}
        <button
          type="button"
          class="gw-ed-datei-btn"
          disabled={!enabled}
          title={`${anhang.filename} — ${kindText(anhang.media_type)}`}
          onclick={() => editor && place(editor, anhang)}
        >
          {anhang.filename}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  /* `:global`, because Ark renders these elements itself and Svelte's scoping attribute
     never reaches them — the same reason `Dialog.svelte` gives. */
  :global(.gw-ed-toolgroup) {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  :global(.gw-ed-tool) {
    font: inherit;
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink-muted);
    cursor: pointer;
  }

  :global(.gw-ed-tool:hover:not(:disabled)) {
    background: var(--bg-sunken);
    color: var(--ink);
  }

  :global(.gw-ed-tool:disabled) {
    opacity: 0.55;
    cursor: default;
  }

  :global(.gw-ed-dateien) {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-2);
  }

  :global(.gw-ed-dateien > p) {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  :global(.gw-ed-dateiliste) {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  /* The same shape as a block control, because it sits beside them and does the same kind of
     thing — but never in the pressed state, because it has none: it is an act, not a state. */
  :global(.gw-ed-datei-btn) {
    font: inherit;
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--accent);
    cursor: pointer;
    max-inline-size: 20rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(.gw-ed-datei-btn:hover:not(:disabled)) {
    background: var(--accent-soft);
  }

  :global(.gw-ed-datei-btn:disabled) {
    opacity: 0.55;
    cursor: default;
    color: var(--ink-muted);
  }

  /* Pressed reads as pressed by TWO means, not by tint alone: the accent border and the
     weight change survive a monochrome display and a colour-vision difference, and
     `aria-pressed` (which Ark sets) carries it to a screen reader. */
  :global(.gw-ed-tool[data-state='on']) {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--ink);
    font-weight: 650;
  }
</style>
