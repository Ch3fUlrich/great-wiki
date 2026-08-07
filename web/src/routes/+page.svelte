<script lang="ts">
  import Tree from '$lib/components/Tree.svelte';
  let { data } = $props();
</script>

<svelte:head><title>great-wiki</title></svelte:head>
<main id="content" class="wrap">
  <h1>great-wiki</h1>
  {#if data.tree.length}
    <Tree nodes={data.tree} />
  {:else}
    <p>No pages yet.</p>
  {/if}
</main>

<style>.wrap { max-width: 60rem; margin: 0 auto; padding: 1.5rem; }</style>
