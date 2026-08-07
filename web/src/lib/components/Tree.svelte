<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import Self from './Tree.svelte';

  let { nodes, current }: { nodes: TreeNode[]; current?: string } = $props();
</script>

{#if nodes.length}
  <ul>
    {#each nodes as node (node.path)}
      <li>
        <a href={node.path} aria-current={node.path === current ? 'page' : undefined}>
          {node.title}
        </a>
        <Self nodes={node.children} {current} />
      </li>
    {/each}
  </ul>
{/if}

<style>
  ul { list-style: none; margin: 0; padding-left: 1rem; }
  a[aria-current='page'] { font-weight: 600; }
</style>
