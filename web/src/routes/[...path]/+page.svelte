<script lang="ts">
  import BlockView from '$lib/components/BlockView.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import { outline } from '$lib/blocks/render';

  let { data } = $props();
  const headings = $derived(outline(data.body));
</script>

<svelte:head><title>{data.doc.title} — great-wiki</title></svelte:head>

<div class="shell">
  <nav aria-label="Site"><Tree nodes={data.tree} current={data.doc.path} /></nav>
  <main id="content" lang={data.doc.language}>
    <h1>{data.doc.title}</h1>
    <BlockView block={data.body} />
  </main>
  {#if headings.length > 1}
    <nav aria-label="On this page">
      <ul>
        {#each headings as h (h.id)}
          <li style:padding-left={`${(h.level - 1) * 0.75}rem`}>
            <a href={`#${h.id}`}>{h.text}</a>
          </li>
        {/each}
      </ul>
    </nav>
  {/if}
</div>

<style>
  .shell {
    display: grid; gap: 2rem; padding: 1.5rem;
    grid-template-columns: minmax(12rem, 16rem) minmax(0, 1fr) minmax(10rem, 14rem);
    max-width: 90rem; margin: 0 auto;
  }
  /* Single column on narrow viewports; the page body must never scroll horizontally. */
  @media (max-width: 60rem) { .shell { grid-template-columns: 1fr; } }
  main :global(pre) { overflow-x: auto; background: var(--panel); padding: 1rem; border-radius: 6px; }
  nav ul { list-style: none; margin: 0; padding: 0; }
</style>
