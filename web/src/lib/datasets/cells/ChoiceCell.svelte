<script lang="ts">
  /**
   * select (one option id), multi_select (checkboxes, an id array) and tags (free text, comma
   * separated). Option ids are never shown, their labels are; all of it prints as text.
   */
  import type { Field } from '../table';
  import { optionsOf } from './edit';

  let {
    field,
    value = $bindable(''),
    error = null
  }: { field: Field; value: string | string[]; error?: string | null } = $props();

  const options = $derived(optionsOf(field.config));
  const chosen = $derived(Array.isArray(value) ? value : []);

  function toggle(id: string, on: boolean) {
    const rest = chosen.filter((x) => x !== id);
    value = on ? [...rest, id] : rest;
  }
</script>

{#if field.kind === 'select'}
  <select
    name={field.key}
    bind:value={() => (typeof value === 'string' ? value : ''), (v) => (value = v)}
    aria-invalid={error ? 'true' : undefined}
  >
    <option value="">–</option>
    {#each options as o (o.id)}
      <option value={o.id}>{o.label}</option>
    {/each}
  </select>
{:else if field.kind === 'multi_select'}
  <span class="auswahl" role="group" aria-label={field.label}>
    {#each options as o (o.id)}
      <label class="option">
        <input
          type="checkbox"
          name={field.key}
          value={o.id}
          checked={chosen.includes(o.id)}
          onchange={(e) => toggle(o.id, e.currentTarget.checked)}
        />
        {o.label}
      </label>
    {/each}
  </span>
{:else}
  <input
    type="text"
    name={field.key}
    placeholder="Komma-getrennt"
    bind:value={() => (typeof value === 'string' ? value : ''), (v) => (value = v)}
    aria-invalid={error ? 'true' : undefined}
  />
{/if}
{#if error}<span class="zell-fehler" role="alert">{error}</span>{/if}
