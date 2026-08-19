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
      short: 'Durchgestrichen',
      on: (e: Editor) => e.isActive('strike'),
      run: (e: Editor) => e.chain().focus().toggleMark('strike').run()
    },
    {
      id: 'link',
      label: 'Link',
      short: 'Link',
      on: (e: Editor) => e.isActive('link'),
      // The one control here that is not a bare toggle: turning a link ON needs a URL from
      // somewhere, and every other control in this table needs nothing beyond "is it on".
      // `window.prompt` rather than a proper dialog is the scope this task actually asked
      // for — a toolbar toggle, not a link-editing UI — and it is revisitable without
      // touching anything else here, since `run` is the only place that would change.
      run: (e: Editor) => {
        if (e.isActive('link')) {
          e.chain().focus().unsetMark('link').run();
          return;
        }
        const href = window.prompt('Adresse des Links (https://…):');
        if (!href) return;
        e.chain().focus().setMark('link', { href }).run();
      }
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
