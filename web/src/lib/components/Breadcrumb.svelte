<!--
  Where this page sits, from the root down.

  Readers arrive by direct URL — a link in a chat, a bookmark, a search result — and the
  corpus is four levels deep in places. Without this, "Heikler Text" is a page with a title
  and no context: nothing on it says it lives under Import/Export, which lives under
  Rundgang, and the sidebar tree only says so if you can find the highlighted row in it.

  Every crumb comes from `/api/tree`, which is filtered in the retriever to what this
  caller may read. That is the whole reason it is not assembled from `doc.path`: a path has
  segments, not titles, and inventing "Import Export" from `import-export` would put a
  guess where a fact belongs — in the one case where the ancestors are the ones the reader
  was deliberately not shown.

  The last crumb is still a link, per the ARIA breadcrumb pattern, and carries
  `aria-current="page"`. A screen-reader user gets told which entry is where they already
  are; a mouse user gets a target that reloads the page, which is harmless and is what the
  pattern's own example does.
-->
<script lang="ts">
  import type { Crumb } from '$lib/pagemeta';

  interface Props {
    crumbs: Crumb[];
    /**
     * What a crumb links to, when that is not simply the page's path.
     *
     * The reader page passes one so a crumb keeps the open workspace: following it
     * navigates the ACTIVE tab instead of collapsing the strip to one. Optional and the
     * identity by default, so this component is unchanged wherever no workspace surrounds
     * it — and so every test of it stays a test of plain page addresses.
     */
    hrefFor?: (path: string) => string;
  }

  let { crumbs, hrefFor }: Props = $props();

  const at = (path: string) => (hrefFor ? hrefFor(path) : path);
</script>

<nav class="crumbs" aria-label="Pfad">
  <ol>
    <li><a href={at('/')}>Start</a></li>
    {#each crumbs as crumb, i (crumb.path)}
      <li>
        <a href={at(crumb.path)} aria-current={i === crumbs.length - 1 ? 'page' : undefined}>
          {crumb.title}
        </a>
      </li>
    {/each}
  </ol>
</nav>

<style>
  /* `@layer components` is the plugin contract (ADR 0005): application CSS is layered,
     plugin CSS arrives unlayered, and an unlayered normal declaration beats every layered
     one regardless of specificity. `components` is also the last layer app.css declares,
     which is what lets these rules win over `@layer content`'s `.prose ul` without raising
     specificity — this navigation sits inside the page column, next to prose. */
  @layer components {
    .crumbs {
      font-size: var(--text-sm);
    }

    ol {
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: var(--space-1) var(--space-2);
      list-style: none;
      margin: 0;
      padding: 0;
    }

    /* The separator is decoration, so it is generated rather than written into the
       markup — and the second `content` gives it empty alternative text, which keeps a
       screen reader from reading "rechtes Anführungszeichen" between every crumb. The
       first declaration is the fallback for an engine that does not support the `/ ''`
       syntax; dropping it would make the separator vanish there entirely. */
    li + li::before {
      content: '›';
      content: '›' / '';
      margin-inline-end: var(--space-2);
      color: var(--ink-faint);
    }

    /* Muted, not accent. These are links, but they are chrome above the title rather than
       part of the reading path, and a row of accent-blue above every heading competes with
       the heading. The underline carries the affordance instead — the same trade the tree
       makes in the other direction, where a dense list of links would be noisy underlined
       and needs the hue. */
    a {
      color: var(--ink-muted);
      text-decoration: underline;
      text-decoration-color: var(--border-strong);
    }

    a:hover,
    a:focus-visible {
      color: var(--ink);
      text-decoration-color: currentcolor;
    }

    /* Not colour alone: the page you are on is the one in the reading ink. */
    a[aria-current='page'] {
      color: var(--ink);
      font-weight: 650;
      text-decoration: none;
    }
  }
</style>
