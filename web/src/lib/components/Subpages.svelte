<!--
  The children of this page, as links.

  This exists because of a specific complaint rather than a general wish for completeness.
  Several pages in the plan are pure containers — a heading, one sentence, and a back-link
  — and a reader landing on one sees what looks like a page somebody forgot to write. The
  subpages ARE its content; they were only ever visible in the sidebar tree, which on a
  phone has been pushed to the very bottom of the document.

  Rendered after the body, not before it: on a container page the body is a line or two, so
  the list is immediately in view anyway, while on a full page a list of children ahead of
  the first paragraph would push the actual writing off the screen.

  Every entry comes from `/api/tree`, which is filtered in the retriever to what this
  caller may read (AGENTS.md rule 2). A child the reader may not open is not in the tree,
  so it cannot be listed here — including its title, which would be a disclosure on its own.
-->
<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import { subpageCount } from '$lib/pagemeta';

  /** Named `nodes` rather than `children`: in Svelte 5 `children` is the default snippet,
      and a prop of that name is a collision waiting to be debugged by somebody else. */
  let { nodes }: { nodes: TreeNode[] } = $props();
</script>

{#if nodes.length > 0}
  <nav class="subpages" aria-labelledby="gw-subpages">
    <h2 id="gw-subpages">Unterseiten</h2>
    <ul>
      {#each nodes as child (child.path)}
        <li>
          <a href={child.path}>{child.title}</a>
          <!-- The tree already knows how deep this goes, so saying it costs nothing and
               saves a click into a page whose only content is more pages. -->
          {#if child.children.length > 0}
            <span class="count">{subpageCount(child.children.length)}</span>
          {/if}
        </li>
      {/each}
    </ul>
  </nav>
{/if}

<style>
  /* `@layer components`, the plugin contract (ADR 0005). See Breadcrumb.svelte.
     This section deliberately sits OUTSIDE `.prose` — the route puts `.prose` on the
     article rather than on `<main>` — so `.prose ul` never reaches it. Were that ever
     undone, `components` coming after `content` in app.css's order is what would keep
     the indent off, without `!important` and without inflating a selector. */
  @layer components {
    .subpages {
      border-block-start: 1px solid var(--border);
      padding-block-start: var(--space-4);
    }

    h2 {
      margin: 0 0 var(--space-3);
      color: var(--ink-faint);
      font-size: var(--text-xs);
      font-weight: 650;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    ul {
      list-style: none;
      margin: 0;
      padding: 0;
      /* Two columns where there is room: these are short titles, and a single column of
         eight of them is a lot of vertical space for a list you scan rather than read. */
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
      gap: var(--space-1) var(--space-4);
    }

    li {
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: var(--space-2);
      padding-block: var(--space-1);
    }

    /* The accent, like the tree — these are the reason the section exists, and muted ink
       would make a container page's only real content read as a list of labels. That
       exact mistake was made once already in the tree and in the outline. */
    a {
      color: var(--accent);
      text-decoration: none;
    }

    a:hover,
    a:focus-visible {
      text-decoration: underline;
      text-underline-offset: 0.15em;
    }

    .count {
      color: var(--ink-faint);
      font-size: var(--text-xs);
      white-space: nowrap;
    }
  }
</style>
