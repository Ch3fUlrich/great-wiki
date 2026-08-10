<!--
  Mounts Dialog.svelte in isolation so the automated behaviour check
  (web/scripts/behaviour.mjs, Group B) has something to open, tab through and Escape —
  none of the real routes use Dialog yet (ADR 0005's Ark UI spike, D-M2-14). Not a page a
  reader should ever reach: see +page.server.ts in this directory for how that is
  enforced, and why the leading underscore alone does not do it.
-->
<script lang="ts">
  import Dialog from '$lib/components/Dialog.svelte';

  let name = $state('');
</script>

<Dialog title="Testdialog">
  {#snippet trigger()}
    Dialog öffnen
  {/snippet}

  <label class="field">
    Name
    <input type="text" bind:value={name} />
  </label>

  {#snippet footer()}
    <button type="button">Fertig</button>
  {/snippet}
</Dialog>
