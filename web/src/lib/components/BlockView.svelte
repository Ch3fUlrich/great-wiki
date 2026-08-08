<script lang="ts">
  import type { Block } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { plainText } from '$lib/blocks/render';
  import Self from './BlockView.svelte';

  let { block }: { block: Block } = $props();

  /// A row belongs in the `thead` when it holds header cells. The kind of the cell decides
  /// it, so no row needs to know its position in the table.
  const isHeaderRow = (row: Block) => (row.content ?? []).some((c) => c.kind === 'tableHeader');

  /// Split the leading run of header rows off the rest. Only the *leading* run: a header
  /// row further down (a repeated head in a long table) stays where the author put it,
  /// because moving it would reorder the document.
  function sections(rows: Block[]): { head: Block[]; body: Block[] } {
    const first = rows.findIndex((row) => !isHeaderRow(row));
    return first < 0 ? { head: rows, body: [] } : { head: rows.slice(0, first), body: rows.slice(first) };
  }

  /// The column's alignment, or `undefined` where the table states none — Svelte then
  /// writes no `text-align` at all and the stylesheet's own default stands.
  const align = (cell: Block) => {
    const value = cell.attrs?.align;
    return value === 'left' || value === 'center' || value === 'right' ? value : undefined;
  };
</script>

<!-- Only known kinds render. An unknown block is skipped rather than emitted raw, which
     is why there is no sanitisation step here: no untrusted HTML is ever constructed. -->
{#if block.kind === 'doc'}
  {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
{:else if block.kind === 'paragraph'}
  <p>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</p>
{:else if block.kind === 'heading'}
  {@const level = Math.min(6, Math.max(1, Number(block.attrs?.level ?? 1)))}
  {@const id = slugify(plainText(block))}
  <svelte:element this={`h${level}`} {id}>
    {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
  </svelte:element>
{:else if block.kind === 'bulletList'}
  <ul>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ul>
{:else if block.kind === 'orderedList'}
  <ol>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ol>
{:else if block.kind === 'listItem'}
  <li>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</li>
{:else if block.kind === 'blockquote'}
  <blockquote>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</blockquote>
{:else if block.kind === 'codeBlock'}
  <pre><code>{plainText(block)}</code></pre>
{:else if block.kind === 'table'}
  {@const rows = sections(block.content ?? [])}
  <!-- A wide table scrolls inside this box rather than pushing the page sideways — a
       body that scrolls horizontally is what makes a document unreadable on a phone. A
       scrollable box is only reachable without a mouse if it is focusable, and only
       announced if it is named, so it is a named region with a tab stop. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- The rule guards against tab stops that lead nowhere. This one leads somewhere: a
       scrollable box that cannot be focused cannot be scrolled without a mouse (WCAG
       2.1.1), and `region` + a name is the pattern that announces it as one. -->
  <div class="table-scroll" role="region" aria-label="Tabelle" tabindex="0">
    <table>
      {#if rows.head.length > 0}
        <thead>{#each rows.head as row, i (i)}<Self block={row} />{/each}</thead>
      {/if}
      {#if rows.body.length > 0}
        <tbody>{#each rows.body as row, i (i)}<Self block={row} />{/each}</tbody>
      {/if}
    </table>
  </div>
{:else if block.kind === 'tableRow'}
  <tr>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</tr>
{:else if block.kind === 'tableHeader'}
  <th scope="col" style:text-align={align(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</th
  >
{:else if block.kind === 'tableCell'}
  <td style:text-align={align(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</td
  >
{:else if block.kind === 'text'}{block.text}{/if}

<style>
  .table-scroll {
    overflow-x: auto;
    /* The focus ring belongs around the whole scroll box, not inside it. */
    border-radius: var(--radius-sm);
  }
</style>
