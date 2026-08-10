<!--
  A dialog, themed through great-wiki's tokens.

  The Ark UI spike (D-M2-14, ADR 0005) lands here first, because a dialog is the component
  whose correctness is least visible and most fiddly: focus has to move into it on open and
  back to the trigger on close, Tab must not escape it, Escape must dismiss it, the page
  behind it must be inert to a screen reader, and scroll must be locked without the page
  jumping. Every one of those is easy to leave out and impossible to notice by looking.

  Ark supplies exactly that behaviour and NO appearance — it ships no stylesheet at all —
  which is why ADR 0005 chose it over a styled library. Every rule below reads from the same
  custom properties as the rest of the application, so a theme reaches this component the
  same way it reaches everything else. A component a theme cannot repaint would break the
  plugin contract, which is the whole reason the token layer exists.
-->
<script lang="ts">
  import { Dialog } from '@ark-ui/svelte/dialog';
  import { Portal } from '@ark-ui/svelte/portal';
  import type { Snippet } from 'svelte';

  interface Props {
    /** Shown as the dialog's accessible name. Never omit it: a dialog with no name is
        announced as just "dialog", which tells a screen-reader user nothing. */
    title: string;
    description?: string;
    /** The control that opens it. Rendered inside Ark's trigger so focus returns here. */
    trigger: Snippet;
    children: Snippet;
    /** Actions. Kept separate from the body so the layout can pin them without the caller
        having to know how. */
    footer?: Snippet;
  }

  let { title, description, trigger, children, footer }: Props = $props();
</script>

<Dialog.Root>
  <Dialog.Trigger>
    {@render trigger()}
  </Dialog.Trigger>

  <Portal>
    <Dialog.Backdrop class="gw-dialog-backdrop" />
    <Dialog.Positioner class="gw-dialog-positioner">
      <Dialog.Content class="gw-dialog">
        <Dialog.Title class="gw-dialog-title">{title}</Dialog.Title>
        {#if description}
          <Dialog.Description class="gw-dialog-description">{description}</Dialog.Description>
        {/if}

        <div class="gw-dialog-body">
          {@render children()}
        </div>

        {#if footer}
          <div class="gw-dialog-footer">
            {@render footer()}
          </div>
        {/if}

        <Dialog.CloseTrigger class="gw-dialog-close" aria-label="Schließen">×</Dialog.CloseTrigger>
      </Dialog.Content>
    </Dialog.Positioner>
  </Portal>
</Dialog.Root>

<!--
  Not `<style>` but `:global`, because these elements are rendered by Ark inside a portal
  at the end of <body> — Svelte's scoping attribute never reaches them. Prefixed `gw-`
  rather than scoped, so the names stay predictable for a theme to target.
-->
<style>
  /* `:not([hidden])` on every rule that sets `display` or `position`.
   *
   * Ark closes the dialog by setting the `hidden` attribute and `data-state="closed"`,
   * relying on the user-agent rule `[hidden] { display: none }`. That rule has almost no
   * specificity, so ANY author `display` declaration silently beats it — and the dialog
   * stayed fully visible and interactive after Escape, with `data-state="closed"` sitting
   * right there in the DOM. Nothing threw, nothing logged, and it looked correct until the
   * close was actually exercised.
   *
   * `:not([hidden])` rather than `!important`, deliberately: an important declaration in a
   * cascade layer outranks an unlayered one, so it would be the one thing in the
   * application a plugin could not override.
   *
   * TWO THINGS THIS GUARD WILL DO TO YOU, both found the hard way while building the
   * admin console on Ark:
   *
   * 1. IT RAISES SPECIFICITY. `:not()` takes the weight of its argument, so
   *    `.btn:not([hidden])` is (0,2,0) and silently outranks `.btn--primary` at (0,1,0) —
   *    every modifier stops applying, nothing is invalid, nothing is logged, and it looks
   *    like a missing class. If a guarded rule has modifiers, the modifiers must repeat
   *    the guard.
   *
   * 2. IT CANNOT BE APPLIED MECHANICALLY, because Ark is not consistent about how it
   *    closes things. Tabs content, collapsible and tree branch content, and the content
   *    of combobox, select, menu and tooltip all use the `hidden` attribute — those need
   *    the guard. `Field.ErrorText` is simply not rendered when there is no error, and
   *    guarding it produces a rule that never matches. Check which one you have before
   *    adding the guard, or you get dead CSS in one direction and a control that will not
   *    close in the other. */
  :global(.gw-dialog-backdrop:not([hidden])) {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 45%);
    z-index: 50;
  }

  :global(.gw-dialog-positioner:not([hidden])) {
    position: fixed;
    inset: 0;
    z-index: 51;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-4);
  }

  :global(.gw-dialog:not([hidden])) {
    position: relative;
    background: var(--bg-raised);
    color: var(--ink);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: var(--space-6);
    inline-size: min(32rem, 100%);
    /* A dialog taller than the viewport scrolls inside itself. Without this the
       actions at the bottom become unreachable on a phone. */
    max-block-size: calc(100vh - var(--space-8));
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  :global(.gw-dialog-title) {
    font-size: var(--text-xl);
    font-weight: 650;
    line-height: var(--leading-tight);
    /* Room for the close control, so a long title never runs under it. */
    padding-inline-end: var(--space-8);
  }

  :global(.gw-dialog-description) {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  :global(.gw-dialog-footer) {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
    flex-wrap: wrap;
    margin-block-start: var(--space-2);
  }

  :global(.gw-dialog-close) {
    position: absolute;
    inset-block-start: var(--space-3);
    inset-inline-end: var(--space-3);
    background: none;
    border: 0;
    color: var(--ink-muted);
    font-size: var(--text-xl);
    line-height: 1;
    cursor: pointer;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
  }

  :global(.gw-dialog-close:hover) {
    background: var(--bg-sunken);
    color: var(--ink);
  }
</style>
