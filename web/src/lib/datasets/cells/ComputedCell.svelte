<script lang="ts">
  /**
   * The read-only display of a computed column (formula, rollup). The value arrives exactly as
   * the server evaluated it — a number, string, bool, list or nothing — or as an error object
   * whose text is the full `#ERR(reason)` cell. All of it is printed as TEXT, never as markup.
   */
  import { cellOf, type Field } from '../table';

  let { field, value = null }: { field: Field; value?: unknown } = $props();

  const cell = $derived(cellOf(field, value));
  const text = $derived(
    cell.kind === 'chips' ? cell.items.join(', ') : cell.kind === 'error' ? cell.reason : cell.text
  );
</script>

{#if cell.kind === 'error'}
  <span class="err">{text}</span>
{:else}
  <span>{text}</span>
{/if}
