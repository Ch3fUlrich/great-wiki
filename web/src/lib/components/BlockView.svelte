<script lang="ts">
  import type { Block } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { plainText } from '$lib/blocks/render';
  import Self from './BlockView.svelte';

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
{:else if block.kind === 'text'}{block.text}{/if}
