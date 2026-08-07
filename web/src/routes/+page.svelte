<script lang="ts">
  import Tree from '$lib/components/Tree.svelte';
  let { data } = $props();
</script>

<svelte:head><title>great-wiki</title></svelte:head>

<main id="content" class="home">
  <h1>great&#8209;wiki</h1>
  <p class="lead">
    Eine selbst gehostete Wissensplattform. Wähle eine Seite, um zu beginnen.
  </p>

  {#if data.tree.length}
    <nav aria-label="Seitenbaum"><Tree nodes={data.tree} /></nav>
  {:else}
    <p>Noch keine Seiten vorhanden.</p>
  {/if}
</main>

<style>
  .home {
    max-width: var(--measure);
    margin-inline: auto;
    padding: var(--space-12) var(--space-6);
  }

  h1 {
    font-size: var(--text-4xl);
    line-height: var(--leading-tight);
    letter-spacing: -0.02em;
    font-weight: 650;
  }

  .lead {
    margin-block: var(--space-3) var(--space-8);
    color: var(--ink-muted);
    font-size: var(--text-lg);
  }
</style>
