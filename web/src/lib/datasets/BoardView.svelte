<script lang="ts">
  /**
   * A dataset page's saved BOARD view (M8c C4): the generic board over the same rows the
   * table shows, grouped by the view's select field, a drag being one row PATCH through the
   * same endpoint the row editor uses.
   *
   * A failed read, a missing page or a `group_by` the dataset cannot group by is STATED in a
   * role=alert — never drawn as an empty board. Card titles come from the source and reach
   * the page as text.
   */
  import Board from '../boards/Board.svelte';
  import { DatasetRowSource } from '../boards/DatasetRowSource';
  import type { RowSource } from '../boards/RowSource';
  import { apiSendBrowser } from './browser';
  import type { Field, RowPage } from './table';
  import { boardGroupKey, type DatasetView } from './views';

  let {
    path,
    fields,
    page,
    view,
    mayWrite = false,
    error = null
  }: {
    path: string;
    fields: Field[];
    page: RowPage | null;
    view: DatasetView | null;
    mayWrite?: boolean;
    error?: string | null;
  } = $props();

  const built = $derived.by((): { source: RowSource } | { message: string } => {
    if (error) return { message: error };
    if (!page) return { message: 'Die Zeilen des Boards konnten nicht geladen werden.' };
    const groupKey = boardGroupKey(view);
    if (!groupKey) return { message: 'Diese Ansicht gruppiert nach keinem Feld.' };
    try {
      return { source: DatasetRowSource(fields, page.rows, groupKey, apiSendBrowser, path, mayWrite) };
    } catch (e) {
      return { message: e instanceof Error ? e.message : 'Das Board konnte nicht aufgebaut werden.' };
    }
  });
</script>

{#if 'message' in built}
  <p class="board-fehler" role="alert">{built.message}</p>
{:else}
  <Board source={built.source} />
{/if}

<style>
  @layer components {
    .board-fehler {
      color: var(--danger, inherit);
      margin-block: var(--space-4);
    }
  }
</style>
