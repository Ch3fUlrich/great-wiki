<!--
  The block controls.

  # Why there is nothing here for bold, italic or a link

  Because the system cannot store them. `gw_core::Block` has `kind`, `attrs`, `content` and
  `text`, and no field for inline formatting — so a bold word lives in the CRDT and is
  dropped by `CollabDoc::to_block` at the next publish. `extensions.ts` leaves the marks out
  of the schema entirely for that reason, and this toolbar simply has nothing to offer for
  them: a control whose effect disappears at the next save is worse than a missing one,
  because it is only discovered after the text has been written.

  Every control below maps one-to-one onto a `BlockKind` the server can store, so pressing
  any of them is lossless.

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

  interface Props {
    /** `null` until the session is live and the surface exists. */
    editor: Editor | null;
    /** Whether a command could reach anywhere. False in every dead-end session state. */
    enabled: boolean;
  }

  let { editor, enabled }: Props = $props();

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
      short: 'Code',
      on: (e: Editor) => e.isActive('codeBlock'),
      run: (e: Editor) => e.chain().focus().toggleCodeBlock().run()
    }
  ] as const;

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
