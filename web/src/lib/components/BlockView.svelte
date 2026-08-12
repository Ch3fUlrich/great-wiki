<script lang="ts">
  import type { Block } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { plainText } from '$lib/blocks/render';
  import { alignOf } from '$lib/blocks/table';
  import Self from './BlockView.svelte';
  import TableView from './TableView.svelte';

  let { block }: { block: Block } = $props();
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
  <!-- TableView owns the scroll box, the sticky header and — once it has mounted in a
       browser — sorting and filtering. It renders no cell content itself: `nested` hands
       the recursion below straight back to it, so there is exactly one block renderer and
       no import cycle between the two components. -->
  <TableView {block} child={nested} />
{:else if block.kind === 'tableRow'}
  <tr>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</tr>
{:else if block.kind === 'tableHeader'}
  <th scope="col" style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</th
  >
{:else if block.kind === 'tableCell'}
  <td style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</td
  >
{:else if block.kind === 'text'}{block.text}{/if}

{#snippet nested(child: Block)}<Self block={child} />{/snippet}
