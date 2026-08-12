<!--
  A rendered table that can be sorted and filtered — as a PROGRESSIVE ENHANCEMENT.

  The contract, which is the whole design and not a detail: the server renders the
  complete table, every row of it, in the order the author wrote, with no control of any
  kind. Only after the component has mounted in a browser — where the controls can
  actually do something — do the sort buttons, the filters and the row count appear. A
  reader with JavaScript disabled therefore gets the document, not an empty shell, not a
  spinner, and above all not a filter box that silently does nothing.

  That is why `enhanced` is flipped in `onMount` rather than derived from `browser` from
  `$app/environment`. `browser` is already true during hydration, so the client's first
  render would disagree with the server's HTML about which branch of the `{#if}` exists,
  which is a hydration mismatch. `onMount` runs after hydration has matched.

  The logic — what "sorted" means for a cell holding `>1200 ppm`, an umlaut or nothing at
  all — is in `$lib/blocks/table.ts`, where it can be tested without a DOM.

  Cell CONTENTS are rendered by the `child` snippet, which the caller (BlockView) supplies
  and which points back at BlockView's own recursion. That is not indirection for its own
  sake: it keeps the block renderer in one place, and it avoids a circular import between
  two components that would otherwise each need the other.
-->
<script lang="ts">
  import { Field } from '@ark-ui/svelte/field';
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import type { Block } from '$lib/blocks/render';
  import {
    ariaSort,
    columns,
    isInteractive,
    rowCountLabel,
    sectionRows,
    sortActionLabel,
    sortAnnouncement,
    sortOrder,
    sortStateFor,
    textGrid,
    visibleOrder,
    type FilterState,
    type SortState
  } from '$lib/blocks/table';
  import './TableView.css';

  interface Props {
    /** The `table` block. */
    block: Block;
    /** Renders one nested block — BlockView's recursion, handed in. */
    child: Snippet<[Block]>;
  }

  let { block, child }: Props = $props();

  const sections = $derived(sectionRows(block.content ?? []));
  const cols = $derived(sections.head.length === 1 ? columns(sections.head[0]) : []);
  const grid = $derived(textGrid(sections.body));
  const eligible = $derived(isInteractive(sections));

  let mounted = $state(false);
  onMount(() => {
    mounted = true;
  });

  /** Controls exist only where they can work AND where they are worth their space. */
  const controls = $derived(mounted && eligible);

  let sort = $state<SortState | null>(null);
  let query = $state('');
  /** One needle per column, sparse: a column never typed into holds nothing. */
  let needles = $state<string[]>([]);

  /**
   * The current row order, as indices into `sections.body`.
   *
   * `null` means document order, which is also what a third click on a header returns to.
   * Holding the ORDER rather than recomputing it from `sort` is what makes a second sort
   * inherit the first one's ties — sorting is applied to the order on screen, not to the
   * document, so "by column A, then by column B within A" is reachable without a
   * multi-sort interface.
   *
   * The length check heals the case where this component is reused for a different table
   * (the `{#each}` in BlockView is keyed by position): a stale permutation would otherwise
   * index rows that no longer exist.
   */
  let order = $state<number[] | null>(null);
  const currentOrder = $derived(
    order && order.length === sections.body.length ? order : sections.body.map((_, i) => i)
  );

  const filter = $derived<FilterState>({
    query,
    columns: cols.map((_, i) => needles[i] ?? '')
  });

  const visible = $derived(
    controls ? visibleOrder(grid, currentOrder, filter) : sections.body.map((_, i) => i)
  );

  const filtering = $derived(query.trim() !== '' || filter.columns.some((n) => n.trim() !== ''));

  /**
   * One live region carrying both facts, because both change on the same interactions and
   * two polite regions firing together are read one after the other with no relation
   * between them.
   */
  const status = $derived(
    [rowCountLabel(visible.length, sections.body.length), sortAnnouncement(sort, cols)]
      .filter(Boolean)
      .join(' · ')
  );

  function toggleSort(column: number) {
    const next = sortStateFor(sort, column);
    sort = next;
    order = next ? sortOrder(grid, currentOrder, next.column, next.direction) : null;
  }

  function resetFilters() {
    query = '';
    needles = [];
  }

  /** Ascending, descending, or "this column can be sorted and is not". */
  const arrow = (column: number) =>
    sort?.column !== column ? '⇅' : sort.direction === 'ascending' ? '▲' : '▼';
</script>

<div class="gw-tbl">
  {#if controls}
    <div class="gw-tbl-bar">
      <Field.Root class="gw-tbl-search">
        <!-- A real `<label>` rather than an `aria-label`: it is what makes clicking the
             text focus the field, and it survives translation tooling that skips
             attributes. Hidden because the placeholder already says it on screen. -->
        <Field.Label class="gw-tbl-vh">Tabelle durchsuchen</Field.Label>
        <Field.Input
          class="gw-tbl-input"
          type="search"
          autocomplete="off"
          placeholder="Tabelle durchsuchen"
          value={query}
          oninput={(event: Event) => (query = (event.currentTarget as HTMLInputElement).value)}
        />
      </Field.Root>

      <button
        type="button"
        class="gw-tbl-reset"
        onclick={resetFilters}
        disabled={!filtering}
      >
        Filter zurücksetzen
      </button>

      <!-- `role="status"` is an implicit `aria-live="polite"`; both are written out
           because the row count is the one thing a filtered table must never be silent
           about, and an assertive announcement would interrupt typing. -->
      <p class="gw-tbl-count" role="status" aria-live="polite">{status}</p>
    </div>
  {/if}

  <!-- A wide table scrolls inside this box rather than pushing the page sideways — a body
       that scrolls horizontally is what makes a document unreadable on a phone. A
       scrollable box is only reachable without a mouse if it is focusable, and only
       announced if it is named, so it is a named region with a tab stop. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- The rule guards against tab stops that lead nowhere. This one leads somewhere: a
       scrollable box that cannot be focused cannot be scrolled without a mouse (WCAG
       2.1.1), and `region` + a name is the pattern that announces it as one. -->
  <div class="gw-tbl-scroll" role="region" aria-label="Tabelle" tabindex="0">
    <table>
      {#if sections.head.length > 0}
        <thead>
          {#if controls}
            <tr>
              {#each cols as col (col.index)}
                <th
                  scope="col"
                  data-align={col.align}
                  style:text-align={col.align}
                  aria-sort={ariaSort(sort, col.index)}
                >
                  <!-- `aria-sort` on the `th` says what the column IS; the hidden span
                       says what pressing the button will DO, which is the half a name
                       alone cannot carry and which changes with every press.

                       The two spans ABUT, with no whitespace between them, deliberately:
                       a newline there is a text node, and the accessible name came out as
                       "Probe , aufsteigend sortieren" — with a space before the comma. -->
                  <button type="button" class="gw-tbl-sort" onclick={() => toggleSort(col.index)}>
                    <span>{col.label}</span><span class="gw-tbl-vh"
                      >, {sortActionLabel(sort, col.index)}</span
                    >
                    <span
                      class="gw-tbl-arrow"
                      data-active={sort?.column === col.index}
                      aria-hidden="true">{arrow(col.index)}</span
                    >
                  </button>

                  <Field.Root class="gw-tbl-filter">
                    <!-- Named after its column, so a screen reader says WHICH column this
                         filters. Eight boxes called "Filter" are eight boxes you have to
                         count your way along. -->
                    <Field.Label class="gw-tbl-vh">{col.label} filtern</Field.Label>
                    <Field.Input
                      class="gw-tbl-input"
                      type="search"
                      autocomplete="off"
                      placeholder="filtern"
                      value={needles[col.index] ?? ''}
                      oninput={(event: Event) =>
                        (needles[col.index] = (event.currentTarget as HTMLInputElement).value)}
                    />
                  </Field.Root>
                </th>
              {/each}
            </tr>
          {:else}
            {#each sections.head as row, i (i)}{@render child(row)}{/each}
          {/if}
        </thead>
      {/if}

      {#if sections.body.length > 0}
        <tbody>
          {#each visible as index (index)}
            {@render child(sections.body[index])}
          {/each}

          {#if controls && visible.length === 0}
            <tr>
              <td class="gw-tbl-empty" colspan={cols.length}>
                Keine Zeile passt zu diesem Filter.
              </td>
            </tr>
          {/if}
        </tbody>
      {/if}
    </table>
  </div>
</div>
