<script lang="ts">
  /**
   * The editor input of a relation cell: the target dataset's rows are fetched ONCE per mount
   * (tests inject `fetchRows`), and the picker offers exactly those rows — a single select, or
   * checkboxes when the column is `multiple`. Picks emit row ids.
   *
   * A selected id whose row is gone stays listed as a placeholder rather than vanishing, and a
   * failed fetch says so in German and keeps the current selection: the cell is never quietly
   * emptied by a network problem.
   */
  import { onMount } from 'svelte';
  import type { Field, Row } from '../table';
  import type { Raw } from './edit';
  import {
    RELATION_FETCH_FAILURE,
    fetchTargetRows,
    pickerOptions,
    relationConfig,
    toggleId
  } from './relation';

  let {
    field,
    value = $bindable(''),
    error = null,
    fetchRows = (targetPath: string) => fetchTargetRows(fetch, targetPath)
  }: {
    field: Field;
    value: Raw;
    error?: string | null;
    fetchRows?: (targetPath: string) => Promise<Row[]>;
  } = $props();

  const cfg = $derived(relationConfig(field.config));
  const multiple = $derived(cfg?.multiple ?? false);
  const selected = $derived(Array.isArray(value) ? value : value === '' ? [] : [String(value)]);

  let targetRows = $state<Row[]>([]);
  let failed = $state(false);

  onMount(async () => {
    const c = cfg;
    if (!c) return;
    try {
      targetRows = await fetchRows(c.targetPath);
    } catch {
      failed = true;
    }
  });

  const options = $derived(pickerOptions(targetRows, selected));

  function toggle(id: string, on: boolean) {
    value = toggleId(selected, id, on);
  }
</script>

{#if multiple}
  <span class="auswahl" role="group" aria-label={field.label}>
    {#each options as o (o.id)}
      <label class="option">
        <input
          type="checkbox"
          name={field.key}
          value={o.id}
          checked={selected.includes(o.id)}
          onchange={(e) => toggle(o.id, e.currentTarget.checked)}
        />
        {o.label}
      </label>
    {/each}
  </span>
{:else}
  <select
    name={field.key}
    bind:value={() => (typeof value === 'string' ? value : (value[0] ?? '')), (v) => (value = v)}
    aria-invalid={error ? 'true' : undefined}
  >
    <option value="">–</option>
    {#each options as o (o.id)}
      <option value={o.id}>{o.label}</option>
    {/each}
  </select>
{/if}
{#if failed}<span class="zell-fehler" role="alert">{RELATION_FETCH_FAILURE}</span>{/if}
{#if error}<span class="zell-fehler" role="alert">{error}</span>{/if}
