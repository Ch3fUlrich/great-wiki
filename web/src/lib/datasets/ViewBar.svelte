<script lang="ts">
  /**
   * The saved views of a dataset page (M8c C4): the table plus one link per view.
   *
   * Plain links and nothing else — a view is chosen by the address alone, so the bar works
   * before any script arrives, like the table's sorting. A name reaches the page as TEXT.
   * With no saved views there is nothing to choose, and the bar says nothing.
   */
  import { viewHref, type DatasetView } from './views';

  let {
    views,
    active = null
  }: {
    views: DatasetView[];
    active?: string | null;
  } = $props();
</script>

{#if views.length > 0}
  <nav class="ansichten" aria-label="Ansichten">
    <a class:aktiv={active === null} href={viewHref(null)} aria-current={active === null ? 'page' : undefined}>Tabelle</a>
    {#each views as v (v.id)}
      <a
        class:aktiv={active === v.id}
        href={viewHref(v.id)}
        aria-current={active === v.id ? 'page' : undefined}>{v.name}</a
      >
    {/each}
  </nav>
{/if}

<style>
  @layer components {
    nav.ansichten {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-3);
      margin-block: var(--space-4) 0;
    }
    nav.ansichten a.aktiv {
      font-weight: 600;
      text-decoration: none;
    }
  }
</style>
