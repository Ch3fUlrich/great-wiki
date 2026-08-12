<script lang="ts">
  import BlockView from '$lib/components/BlockView.svelte';
  import Breadcrumb from '$lib/components/Breadcrumb.svelte';
  import PageMeta from '$lib/components/PageMeta.svelte';
  import Subpages from '$lib/components/Subpages.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import { outline } from '$lib/blocks/render';
  import { breadcrumb, childrenOf } from '$lib/pagemeta';

  let { data } = $props();
  const headings = $derived(outline(data.body));

  // Derived here rather than in the loader on purpose: `$derived` runs during server
  // rendering too, so the markup is complete in the first response, and the tree is
  // already in the payload — computing these server-side would ship the same titles twice.
  const crumbs = $derived(breadcrumb(data.tree, data.doc));
  const subpages = $derived(childrenOf(data.tree, data.doc.path));
</script>

<svelte:head><title>{data.doc.title} — great-wiki</title></svelte:head>

<div class="shell">
  <nav class="sidebar no-print" aria-label="Seitenbaum">
    <Tree nodes={data.tree} current={data.doc.path} />
  </nav>

  <!-- `.prose` moved off `<main>` and onto the article, and `lang` with it. Both were
       right while `<main>` held nothing but the document, and both became wrong the
       moment it grew chrome around one.

       `.prose` is scoped to rendered document content so its rules never reach the
       interface, and one of those rules cannot be overridden by a component at all: the
       print block at the end of app.css is UNLAYERED on purpose, so
       `.prose a::after { content: ' (' attr(href) ')' }` outranks every layered rule
       regardless of specificity. With `.prose` on `<main>`, a printed page would have had
       its own URL spelled out after every crumb and every subpage link.

       `lang` on `<main>` claimed the German metadata panel was written in the document's
       language. On the 29 English pages of the corpus a screen reader would have
       announced "Sichtbarkeit" with English phonemes; the document's language belongs on
       the document. -->
  <main id="content" class="page">
    <Breadcrumb {crumbs} />
    <h1>{data.doc.title}</h1>
    <PageMeta
      visibility={data.doc.visibility}
      language={data.doc.language}
      docType={data.doc.doc_type}
    />

    <article class="prose" lang={data.doc.language}>
      <BlockView block={data.body} />
    </article>

    <Subpages nodes={subpages} />
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
    gap: var(--space-8) var(--space-12);
    padding: var(--space-8) var(--space-6);
    /* The centre column is sized to the measure rather than to leftover space.
       With `1fr` the column stretched to the viewport while the text inside stayed
       capped at 68 characters, so on a wide screen the prose hugged the left edge
       with a band of dead space beside it — which reads as a bug, because it is one. */
    grid-template-columns:
      minmax(11rem, 15rem)
      minmax(0, var(--measure))
      minmax(9rem, 13rem);
    justify-content: center;
    max-width: 100rem;
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
    /* The accent, not muted body ink. These were --ink-muted, which is the colour of
       de-emphasised TEXT, so the on-this-page list read as a set of labels rather than
       as jumps you can take. */
    color: var(--accent);
    text-decoration: none;
    line-height: 1.35;
    /* A wrapped entry keeps its indent on the second line. Without this the
       continuation returns to the left rail and the nesting stops reading. */
    text-indent: 0;
    hanging-punctuation: none;
  }

  .outline a:hover,
  .outline a:focus-visible {
    text-decoration: underline;
    text-underline-offset: 0.15em;
    border-inline-start-color: var(--accent);
  }

  /* The page column: chrome, then the document, then the children.
     `.prose > * + *` in `@layer content` no longer reaches any of this — it applies
     inside the article — so the rhythm between the parts is stated here, and stated
     unevenly on purpose. The breadcrumb belongs to the title, so the gap above the
     heading is small; the subpage list is a different thing from the document, so the
     gap above it is the largest on the page. */
  .page > * + * {
    margin-block-start: var(--space-6);
  }

  .page > h1 {
    font-size: var(--text-4xl);
    line-height: var(--leading-tight);
    letter-spacing: -0.02em;
    margin-block-start: var(--space-3);
    text-wrap: balance;
  }

  .page > :global(.subpages) {
    margin-block-start: var(--space-12);
  }

  /* One column below 64rem.
     Ordering matters more than it looks. Putting both navigations above the text
     means scrolling past two blocks of links to reach the article — on a phone that
     is most of a screen of nothing you came for. So: the outline first, because it
     is short and is the fastest way through a long document; then the article; then
     the site tree, which you only want when you are leaving this page anyway. */
  @media (max-width: 64rem) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-6);
      padding: var(--space-6) var(--space-4);
    }

    .sidebar,
    .outline {
      position: static;
      max-block-size: none;
    }

    .outline {
      order: -1;
      padding-block-end: var(--space-4);
      border-block-end: 1px solid var(--border);
    }

    .sidebar {
      order: 1;
      padding-block-start: var(--space-6);
      border-block-start: 1px solid var(--border);
    }
  }
</style>
