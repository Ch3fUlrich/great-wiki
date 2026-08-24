<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import Self from './Tree.svelte';

  interface Props {
    nodes: TreeNode[];
    current?: string;
    /**
     * What address a page's entry links to, when that is not simply the page's path.
     *
     * The shell passes one so a tree link keeps the open workspace: following it navigates
     * the ACTIVE tab rather than closing every other one. Optional, and the identity by
     * default, so every other use of this component — and every existing test — is
     * untouched, and so a tree rendered anywhere with no workspace around it stays exactly
     * the list of plain page addresses it always was.
     *
     * `aria-current` still compares PATHS, never these addresses: which page you are on is
     * a fact about the page, not about how the link to it was spelled.
     */
    hrefFor?: (path: string) => string;
  }

  let { nodes, current, hrefFor }: Props = $props();
</script>

{#if nodes.length}
  <ul>
    {#each nodes as node (node.path)}
      <li>
        <a
          href={hrefFor ? hrefFor(node.path) : node.path}
          aria-current={node.path === current ? 'page' : undefined}
        >
          {node.title}
        </a>
        <Self nodes={node.children} {current} {hrefFor} />
      </li>
    {/each}
  </ul>
{/if}

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Nested levels get a guide line, so depth is visible without counting indents. */
  :global(li) ul {
    margin-inline-start: var(--space-3);
    padding-inline-start: var(--space-3);
    border-inline-start: 1px solid var(--border);
  }

  /* Navigation links carry the accent colour, not muted body ink.
   *
   * They were `--ink-muted`, which is the colour of de-emphasised TEXT — so a column of
   * page titles read as a list of labels rather than as things you can click, and looked
   * fainter than the prose beside it. Underlines are still off here, because in a dense
   * vertical list they add more noise than affordance; the colour plus the hover
   * background carries it, and the hover adds an underline for anyone who reads shape
   * before hue. */
  a {
    display: block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    color: var(--accent);
    text-decoration: none;
    line-height: 1.4;
  }

  a:hover {
    background: var(--bg-sunken);
    text-decoration: underline;
    text-underline-offset: 0.15em;
  }

  /* The current page is marked by weight and a background, not by colour alone —
     colour alone fails for anyone who cannot distinguish these two hues. */
  a[aria-current='page'] {
    background: var(--accent-soft);
    color: var(--ink);
    font-weight: 650;
  }
</style>
