<script lang="ts">
  /** bool: three states so that "no value" (clears the cell) stays reachable. */
  import type { Field } from '../table';

  let {
    field,
    value = $bindable(''),
    error = null
  }: { field: Field; value: string; error?: string | null } = $props();
</script>

<select name={field.key} bind:value aria-invalid={error ? 'true' : undefined}>
  <option value="">–</option>
  <option value="true">Ja</option>
  <option value="false">Nein</option>
</select>
{#if error}<span class="zell-fehler" role="alert">{error}</span>{/if}
