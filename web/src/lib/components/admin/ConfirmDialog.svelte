<!--
  A destructive action, confirmed, naming exactly what will happen.

  Why this is not `$lib/components/Dialog.svelte`, which it otherwise duplicates: that
  wrapper renders `<Dialog.Root>` with no `open` prop, so it can only ever be opened by
  its own trigger. Two of the confirmations here have no trigger to hang on — turning a
  switch off raises one, and the control that started it is the switch itself. A dialog
  that can only open from a button it owns cannot express that, and editing the shared
  wrapper was not this task's to do. Everything else about it is copied deliberately: the
  same tokens, the same `:global` prefixed class names, the same `:not([hidden])` rule.

  `role="alertdialog"` rather than `dialog`, because the body is not a form to fill in —
  it is a statement a screen-reader user needs read out on open, and that is exactly the
  difference between the two roles.
-->
<script lang="ts">
  import { Dialog } from '@ark-ui/svelte/dialog';
  import { Portal } from '@ark-ui/svelte/portal';

  interface Props {
    open: boolean;
    title: string;
    /**
     * What will happen, in words, including the name of the thing it happens to.
     * "Wirklich fortfahren?" is not acceptable here: the whole value of the step is that
     * somebody reads back what they are about to do.
     */
    body: string;
    confirmLabel: string;
    onConfirm: () => void;
    onCancel: () => void;
    busy?: boolean;
  }

  let { open, title, body, confirmLabel, onConfirm, onCancel, busy = false }: Props = $props();
</script>

<Dialog.Root
  {open}
  role="alertdialog"
  onOpenChange={(details) => {
    // Escape, the backdrop and the close control all arrive here. Every one of them
    // means "no", so every one of them has to reach the caller — otherwise the caller's
    // own state says the dialog is open while Ark has already closed it, and the next
    // attempt does nothing at all.
    if (!details.open) onCancel();
  }}
>
  <Portal>
    <Dialog.Backdrop class="gw-adm-dialog-backdrop" />
    <Dialog.Positioner class="gw-adm-dialog-positioner">
      <Dialog.Content class="gw-adm-dialog">
        <Dialog.Title class="gw-adm-dialog-title">{title}</Dialog.Title>
        <Dialog.Description>{body}</Dialog.Description>
        <div class="gw-adm-dialog-footer">
          <Dialog.CloseTrigger class="gw-adm-btn">Abbrechen</Dialog.CloseTrigger>
          <button
            type="button"
            class="gw-adm-btn gw-adm-btn--danger"
            disabled={busy}
            onclick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </Dialog.Content>
    </Dialog.Positioner>
  </Portal>
</Dialog.Root>
