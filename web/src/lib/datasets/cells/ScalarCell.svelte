<script lang="ts">
  /** text, number, date and url: one input, the kind picks its type. Value is bound as text. */
  import type { Field } from '../table';

  let {
    field,
    value = $bindable(''),
    error = null
  }: { field: Field; value: string; error?: string | null } = $props();

  const invalid = $derived(error ? 'true' : undefined);
</script>

{#if field.kind === 'number'}
  <input
    type="number"
    step="any"
    name={field.key}
    bind:value={() => (value === '' ? null : Number(value)), (v) => (value = v === null || v === undefined ? '' : String(v))}
    aria-invalid={invalid}
  />
{:else if field.kind === 'date'}
  <input type="date" name={field.key} bind:value aria-invalid={invalid} />
{:else if field.kind === 'url'}
  <input type="url" name={field.key} bind:value aria-invalid={invalid} />
{:else}
  <input type="text" name={field.key} bind:value aria-invalid={invalid} />
{/if}
{#if error}<span class="zell-fehler" role="alert">{error}</span>{/if}
