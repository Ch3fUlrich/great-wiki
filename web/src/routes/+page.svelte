<!--
  The start page, which is also the NEW TAB page.

  »Neuer Reiter« in the tab strip opens this address — the same thing a browser's own new
  tab does, and the reason the strip needs no picker and no menu: you land somewhere that
  lists everything, and you navigate from there. That makes this page's job navigation
  rather than welcome, so it lists the whole tree and the three whole-wiki views, and it
  uses the width it is given instead of a reading column it has no prose for.

  EVERY LINK CARRIES THE WORKSPACE. Without that, opening a second tab and then using it
  would close the first one on the very first click. `$lib/tabs` does the arithmetic; the
  set is empty and the links are plain addresses whenever only one tab is open.

  The list is `aria-label="Alle Seiten"`, deliberately NOT "Seitenbaum": the shell draws
  the Seitenbaum on every view, and two landmarks with one name is one too many. This is a
  directory of the same tree, not a second copy of the navigation.
-->
<script lang="ts">
  import Tree from '$lib/components/Tree.svelte';
  import { navigateHref, resolveTabs } from '$lib/tabs';

  let { data } = $props();

  const workspace = $derived(resolveTabs(data.tabHrefs ?? [], data.hier ?? '/'));

  function gehZu(target: string): string {
    return navigateHref(target, workspace.hrefs, workspace.active);
  }

  const bereiche = [
    { href: '/aufgaben', titel: 'Aufgaben', was: 'Jede Aufgabe, die Sie sehen dürfen.' },
    { href: '/projekte', titel: 'Projekte', was: 'Welche Projekte es gibt, und ein neues.' },
    { href: '/graph', titel: 'Graph', was: 'Die Seiten und die Verweise zwischen ihnen.' }
  ];
</script>

<svelte:head><title>great-wiki</title></svelte:head>

<main id="content" class="home">
  <h1>great&#8209;wiki</h1>
  <p class="lead">
    Eine selbst gehostete Wissensplattform. Wählen Sie eine Seite oder einen Bereich — es
    erscheint nur, was Sie auch lesen dürfen.
  </p>

  <nav class="bereiche" aria-label="Bereiche">
    {#each bereiche as bereich (bereich.href)}
      <a class="bereich" href={gehZu(bereich.href)}>
        <span class="bereich-titel">{bereich.titel}</span>
        <span class="bereich-was">{bereich.was}</span>
      </a>
    {/each}
  </nav>

  {#if (data.tree ?? []).length}
    <section aria-labelledby="gw-alle">
      <h2 id="gw-alle">Alle Seiten</h2>
      <!-- Set in columns rather than one tall list. On a desktop screen a single column of
           page titles is a metre of scrolling down the left edge and nothing else; the
           whole point of the shell is to stop wasting the room. `break-inside: avoid` on
           the branches keeps a page and its children in one column together. -->
      <nav class="alle" aria-label="Alle Seiten">
        <Tree nodes={data.tree} hrefFor={gehZu} />
      </nav>
    </section>
  {:else}
    <!-- A wiki with no pages and an unreachable API arrive here identically, and this
         wording is true of both: it says what is here, not what exists. -->
    <p class="leer">Noch keine Seiten vorhanden.</p>
  {/if}
</main>

<style>
  .home {
    padding: var(--space-12) var(--space-6);
  }

  .home > * + * {
    margin-block-start: var(--space-8);
  }

  h1 {
    font-size: var(--text-4xl);
    line-height: var(--leading-tight);
    letter-spacing: -0.02em;
    font-weight: 650;
  }

  h2 {
    font-size: var(--text-xs);
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-faint);
    margin-block-end: var(--space-3);
  }

  /* The one piece of running text on the page, and the only thing held to the measure. */
  .lead {
    margin-block: var(--space-3) 0;
    color: var(--ink-muted);
    font-size: var(--text-lg);
    max-inline-size: var(--measure);
  }

  .bereiche {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr));
    gap: var(--space-4);
    max-inline-size: 72rem;
  }

  .bereich {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-raised);
    text-decoration: none;
  }

  .bereich:hover,
  .bereich:focus-visible {
    border-color: var(--border-strong);
    background: var(--accent-soft);
  }

  .bereich-titel {
    color: var(--accent);
    font-weight: 650;
  }

  .bereich-was {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  .alle {
    columns: 18rem auto;
    column-gap: var(--space-8);
    font-size: var(--text-sm);
  }

  .alle :global(li) {
    break-inside: avoid;
  }

  .leer {
    color: var(--ink-muted);
  }

  @media (max-width: 48rem) {
    .home {
      padding: var(--space-8) var(--space-4);
    }
  }
</style>
