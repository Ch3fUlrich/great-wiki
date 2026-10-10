<script lang="ts">
  /**
   * A dataset page's table (ADR 0029).
   *
   * Everything a value says reaches the page as TEXT: `cellOf` describes a cell as data and
   * this file prints it with `{…}`, never as markup, so a cell holding `<script>` shows those
   * characters. Sorting and filtering are plain links and a GET form (the route reloads the
   * first page), so they work with JavaScript switched off; "Mehr laden" and the add-row form
   * need a script and say nothing without one.
   */
  import RowEditor from './RowEditor.svelte';
  import { editableFields } from './rowedit';
  import {
    cellOf,
    describeRowsFailure,
    rowsApiPath,
    sortHref,
    withoutFilterHref,
    type Field,
    type FilterState,
    type Row,
    type RowPage,
    type SortState
  } from './table';

  let {
    path,
    fields,
    page,
    error = null,
    sort = null,
    filter = null,
    mayWrite = false
  }: {
    path: string;
    fields: Field[];
    page: RowPage | null;
    error?: string | null;
    sort?: SortState | null;
    filter?: FilterState | null;
    mayWrite?: boolean;
  } = $props();

  let more = $state<Row[]>([]);
  let cursor = $state<string | null | undefined>(undefined);
  let loadError = $state<string | null>(null);
  let busy = $state(false);
  let editing = $state<string | null>(null);

  // A new first page (sort, filter or reload) discards what was appended to the old one.
  $effect(() => {
    void page;
    more = [];
    cursor = undefined;
    loadError = null;
  });

  const rows = $derived([...(page?.rows ?? []), ...more]);
  const next = $derived(cursor === undefined ? page?.next : (cursor ?? undefined));
  const labelOf = (key: string) => fields.find((f) => f.key === key)?.label ?? key;
  const ariaSort = (key: string) =>
    sort && sort.sort === key ? (sort.desc ? 'descending' : 'ascending') : 'none';
  const addable = $derived(editableFields(fields));

  async function loadMore(event: Event) {
    event.preventDefault();
    if (!next || busy) return;
    busy = true;
    loadError = null;
    try {
      const res = await fetch(rowsApiPath(path, { sort: sort?.sort, desc: sort?.desc, filter, after: next }));
      if (!res.ok) {
        loadError = describeRowsFailure(res.status);
        return;
      }
      const body = (await res.json()) as RowPage;
      more = [...more, ...body.rows];
      cursor = body.next ?? null;
    } catch {
      loadError = describeRowsFailure(0);
    } finally {
      busy = false;
    }
  }
</script>

<section class="datensatz" aria-label="Tabelle">
  {#if error}
    <p class="tabelle-fehler" role="alert">{error}</p>
  {:else}
    <form class="filter" method="GET">
      {#if sort}
        <input type="hidden" name="sort" value={sort.sort} />
        <input type="hidden" name="desc" value={sort.desc ? '1' : '0'} />
      {/if}
      <label>
        Spalte
        <select name="fk">
          {#each fields as f (f.key)}
            <option value={f.key} selected={filter?.key === f.key}>{f.label}</option>
          {/each}
        </select>
      </label>
      <label>
        enthält
        <input type="text" name="fv" value={filter?.value ?? ''} />
      </label>
      <button type="submit">Filtern</button>
      {#if filter}
        <span class="chip filter-chip">
          {labelOf(filter.key)} enthält „{filter.value}“
          <a href={withoutFilterHref(sort)} aria-label="Filter entfernen">×</a>
        </span>
      {/if}
    </form>

    {#if rows.length === 0}
      <p class="leer">{filter ? 'Keine Zeile passt zum Filter.' : 'Noch keine Zeilen.'}</p>
    {/if}

    {#if fields.length > 0}
      <div class="rollen">
        <table>
          <thead>
            <tr>
              {#each fields as f (f.key)}
                <th scope="col" aria-sort={ariaSort(f.key)}>
                  <a href={sortHref(sort, f.key, filter)}>{f.label}</a>
                </th>
              {/each}
              {#if mayWrite}<th scope="col"><span class="sr-only">Aktionen</span></th>{/if}
            </tr>
          </thead>
          <tbody>
            {#each rows as row (row.id)}
              <tr>
                {#each fields as f (f.key)}
                  {@const cell = cellOf(f, row.values?.[f.key])}
                  <td class:end={cell.kind === 'text' && cell.align === 'end'}>
                    {#if cell.kind === 'chips'}
                      {#each cell.items as item}<span class="chip">{item}</span>{/each}
                    {:else if cell.kind === 'link'}
                      <a href={cell.href} rel="noopener noreferrer">{cell.text}</a>
                    {:else if cell.kind === 'error'}
                      <span class="err" title={cell.reason}>#ERR</span>
                      <span class="err-grund">{cell.reason}</span>
                    {:else}
                      {cell.text}
                    {/if}
                  </td>
                {/each}
                {#if mayWrite}
                  <td>
                    <button type="button" class="bearbeiten" onclick={() => (editing = editing === row.id ? null : row.id)}
                      >Bearbeiten</button
                    >
                  </td>
                {/if}
              </tr>
              {#if editing === row.id}
                <tr class="editor-zeile">
                  <td colspan={fields.length + 1}>
                    <RowEditor {path} {fields} {row} onclose={() => (editing = null)} />
                  </td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    {#if next}
      <p>
        <a
          class="mehr"
          href={`?after=${encodeURIComponent(next)}`}
          onclick={loadMore}
          aria-busy={busy}>Mehr laden</a
        >
      </p>
    {/if}
    {#if loadError}<p class="tabelle-fehler" role="alert">{loadError}</p>{/if}

    {#if mayWrite && addable.length > 0}
      <RowEditor {path} {fields} />
    {/if}
  {/if}
</section>

<style>
  @layer components {
    .datensatz {
      margin-block: var(--space-4);
    }
    .rollen {
      overflow-x: auto;
    }
    table {
      border-collapse: collapse;
      inline-size: 100%;
    }
    th,
    td {
      text-align: start;
      padding: var(--space-1) var(--space-3);
      border-block-end: 1px solid var(--border);
    }
    td.end {
      text-align: end;
    }
    .chip {
      display: inline-block;
      padding: 0 var(--space-2);
      margin-inline-end: var(--space-1);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
    }
    .filter {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2) var(--space-3);
      align-items: end;
      margin-block-end: var(--space-3);
    }
    .sr-only {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .err {
      font-weight: 600;
    }
    .tabelle-fehler {
      color: var(--danger, inherit);
    }
  }
</style>
