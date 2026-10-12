<script lang="ts">
  /**
   * The one view a dataset page (or an embed of one) draws: its picked view rendered by kind —
   * a form, a board, a calendar, a gallery, and the table for everything else.
   *
   * Extracted from `[...path]/+page.svelte` so the reader page and an embedded block draw the
   * SAME view from the SAME props, rather than a second copy of the switch drifting from the
   * first. It renders the branch and nothing else: the caller owns the `ViewBar`, the frame
   * around it, and whether it may write. Purely presentational — it fetches nothing, and a
   * failed row read arrives already stated in `dataset.error` for the view that shows it.
   */
  import CalendarView from './CalendarView.svelte';
  import GalleryView from './GalleryView.svelte';
  import BoardView from './BoardView.svelte';
  import FormView from './FormView.svelte';
  import TableView from './TableView.svelte';
  import { boardGroupKey, calendarConfig, galleryConfig } from './views';
  import { formKeyOf } from './form';
  import type { DatasetLoad } from './load';

  let {
    path,
    dataset,
    mayWrite = false,
    month = null,
    embedded = false
  }: {
    path: string;
    dataset: DatasetLoad;
    mayWrite?: boolean;
    /** The month the address names, for a calendar view. Ignored by every other kind. */
    month?: string | null;
    /** Whether this view is drawn inside somebody else's page (M8d D2W). */
    embedded?: boolean;
  } = $props();

  const view = $derived(dataset.view);
  const formularAnsicht = $derived(formKeyOf(view) !== null ? view : null);
  const kalenderAnsicht = $derived(calendarConfig(view) !== null ? view : null);
  const gallerieAnsicht = $derived(galleryConfig(view) !== null ? view : null);
</script>

{#if formularAnsicht !== null}
  <FormView {path} fields={dataset.fields} view={formularAnsicht} {mayWrite} />
{:else if boardGroupKey(view) !== null}
  <BoardView
    {path}
    fields={dataset.fields}
    page={dataset.page}
    view={view}
    error={dataset.error}
    {mayWrite}
  />
{:else if kalenderAnsicht !== null}
  <CalendarView
    {path}
    fields={dataset.fields}
    page={dataset.page}
    view={kalenderAnsicht}
    {month}
    error={dataset.error}
    {mayWrite}
  />
{:else if gallerieAnsicht !== null}
  <GalleryView
    {path}
    fields={dataset.fields}
    page={dataset.page}
    view={gallerieAnsicht}
    attachments={dataset.attachments}
    error={dataset.error}
  />
{:else}
  <TableView
    {path}
    fields={dataset.fields}
    page={dataset.page}
    error={dataset.error}
    sort={dataset.sort}
    filter={dataset.filter}
    labels={dataset.labels}
    {mayWrite}
    {embedded}
  />
{/if}
