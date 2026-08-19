<!--
  The pages that link TO this one — the graph's edges read backwards, from `/api/links/
  backlinks/{path}` (Task 8), which is `Store::backlinks_for` (Task 7) over HTTP.

  Every entry is already filtered to what THIS caller may read: the store omits a backlink
  from a page the caller cannot see rather than counting it or naming it, so there is
  nothing left for this component to filter and nothing it could disclose by filtering
  wrongly. See `gw-store/src/links.rs` and `gw-api/src/routes/links.rs`.

  Renders NOTHING AT ALL when the list is empty, exactly like Subpages.svelte and for the
  same reason: most pages in this corpus have no backlinks yet, and an empty "Verweist
  hierher" heading with a rule above it would be furniture paid for by every page that has
  nothing to show.
-->
<script lang="ts">
  import type { Backlink } from '$lib/api';

  let { backlinks }: { backlinks: Backlink[] } = $props();
</script>

{#if backlinks.length > 0}
  <nav class="backlinks" aria-labelledby="gw-backlinks">
    <h2 id="gw-backlinks">Verweist hierher</h2>
    <ul>
      {#each backlinks as link (link.path)}
        <li><a href={link.path}>{link.title}</a></li>
      {/each}
    </ul>
  </nav>
{/if}

<style>
  /* `@layer components`, the plugin contract (ADR 0005). See Breadcrumb.svelte and
     Subpages.svelte, whose structure this mirrors on purpose — this is the same kind of
     panel, sitting in the same place in the page. */
  @layer components {
    .backlinks {
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
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
      gap: var(--space-1) var(--space-4);
    }

    li {
      padding-block: var(--space-1);
    }

    a {
      color: var(--accent);
      text-decoration: none;
    }

    a:hover,
    a:focus-visible {
      text-decoration: underline;
      text-underline-offset: 0.15em;
    }
  }
</style>
