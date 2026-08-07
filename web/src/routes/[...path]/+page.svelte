<script lang="ts">
  import BlockView from '$lib/components/BlockView.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import { outline } from '$lib/blocks/render';

  let { data } = $props();
  const headings = $derived(outline(data.body));
</script>

<svelte:head><title>{data.doc.title} — great-wiki</title></svelte:head>

<div class="shell">
  <nav class="sidebar no-print" aria-label="Seitenbaum">
    <Tree nodes={data.tree} current={data.doc.path} />
  </nav>

  <main id="content" class="prose" lang={data.doc.language}>
    <h1>{data.doc.title}</h1>
    <BlockView block={data.body} />
  </main>

  {#if headings.length > 1}
    <nav class="outline no-print" aria-label="Auf dieser Seite">
      <p class="outline-title">Auf dieser Seite</p>
      <ul>
        {#each headings as h (h.id)}
          <li style:padding-inline-start={`${(h.level - 2) * 0.7}rem`}>
            <a href={`#${h.id}`}>{h.text}</a>
          </li>
        {/each}
      </ul>
    </nav>
  {/if}
</div>

<style>
  .shell {
    display: grid;
    gap: var(--space-12);
    padding: var(--space-8) var(--space-6);
    grid-template-columns: minmax(11rem, 15rem) minmax(0, 1fr) minmax(9rem, 13rem);
    max-width: 92rem;
    margin-inline: auto;
    align-items: start;
  }

  /* Both side columns stick, so navigation stays reachable in a long document.
     Offset by the sticky header's height so nothing hides underneath it. */
  .sidebar,
  .outline {
    position: sticky;
    top: calc(var(--space-12) + var(--space-2));
    max-block-size: calc(100vh - var(--space-12) * 2);
    overflow-y: auto;
    font-size: var(--text-sm);
  }

  .outline-title {
    margin-block-end: var(--space-2);
    color: var(--ink-faint);
    font-size: var(--text-xs);
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .outline ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border-inline-start: 1px solid var(--border);
  }

  .outline li {
    padding-block: 2px;
  }

  .outline a {
    display: block;
    padding-inline-start: var(--space-3);
    margin-inline-start: -1px;
    border-inline-start: 2px solid transparent;
    color: var(--ink-muted);
    text-decoration: none;
    line-height: 1.4;
  }

  .outline a:hover {
    color: var(--ink);
    border-inline-start-color: var(--border-strong);
  }

  .prose h1 {
    font-size: var(--text-4xl);
    line-height: var(--leading-tight);
    letter-spacing: -0.02em;
    margin-block-end: var(--space-8);
    text-wrap: balance;
  }

  /* One column below 64rem. The outline moves above the text rather than
     disappearing — on a phone it is the fastest way through a long document. */
  @media (max-width: 64rem) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-8);
      padding: var(--space-6) var(--space-4);
    }

    .sidebar,
    .outline {
      position: static;
      max-block-size: none;
      padding-block: var(--space-4);
      border-block-end: 1px solid var(--border);
    }

    .outline {
      order: -1;
      border-block: 1px solid var(--border);
    }
  }
</style>
