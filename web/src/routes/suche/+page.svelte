<!--
  Suche: pages, topics and tasks matching some words.

  The snippet is rendered from segments, never from HTML: a `hit` segment becomes a `<mark>`
  and every other run is text, so a page whose body contains markup cannot inject any here.

  Nothing on this page counts what was left out. A group's empty state says that nothing was
  found, which is also what a query matching only withheld pages gets — by design (ADR 0024).
-->
<script lang="ts">
  import {
    groupCountText,
    isEmpty,
    mergeSegments,
    pageHitHref,
    taskHitHref,
    topicHitHref
  } from '$lib/search';

  let { data } = $props();
  const results = $derived(data.results);
  const query = $derived(data.query);
</script>

<svelte:head>
  <title>{query ? `Suche: ${query}` : 'Suche'} – great-wiki</title>
</svelte:head>

<div class="page">
  <h1>Suche</h1>

  {#if !query}
    <p class="lede" id="suche-leer">
      Gib oben einen Suchbegriff ein, um Seiten, Themen und Aufgaben zu finden.
    </p>
  {:else}
    <p class="lede">Ergebnisse für „{query}“</p>
    {#if isEmpty(results)}
      <p class="gesamtleer" id="suche-nichts">Nichts gefunden.</p>
    {/if}

    <section aria-labelledby="suche-seiten">
      <h2 id="suche-seiten">Seiten</h2>
      {#if results.pages.length}
        <p class="anzahl">{groupCountText(results.pages.length)}</p>
        <ul class="treffer">
          {#each results.pages as hit (hit.path)}
            <li>
              <a href={pageHitHref(hit)}>{hit.title}</a>
              {#if hit.snippet.length}
                <p class="snippet">
                  {#each mergeSegments(hit.snippet) as segment}{#if segment.hit}<mark
                        >{segment.text}</mark
                      >{:else}{segment.text}{/if}{/each}
                </p>
              {/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="leer">Keine Seiten gefunden.</p>
      {/if}
    </section>

    <section aria-labelledby="suche-themen">
      <h2 id="suche-themen">Themen</h2>
      {#if results.topics.length}
        <p class="anzahl">{groupCountText(results.topics.length)}</p>
        <ul class="treffer">
          {#each results.topics as hit (hit.path)}
            <li>
              <a href={topicHitHref(hit)}>{hit.display_path}</a>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="leer">Keine Themen gefunden.</p>
      {/if}
    </section>

    <section aria-labelledby="suche-aufgaben">
      <h2 id="suche-aufgaben">Aufgaben</h2>
      {#if results.tasks.length}
        <p class="anzahl">{groupCountText(results.tasks.length)}</p>
        <ul class="treffer">
          {#each results.tasks as hit (hit.id)}
            <li>
              <a href={taskHitHref(hit)}>{hit.title}</a>
              <span class="status">{hit.status}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="leer">Keine Aufgaben gefunden.</p>
      {/if}
    </section>
  {/if}
</div>

<style>
  .page {
    padding: var(--space-8) var(--space-6);
  }

  .page > * + * {
    margin-block-start: var(--space-6);
  }

  h1 {
    font-size: var(--text-3xl);
    line-height: var(--leading-tight);
  }

  h2 {
    font-size: var(--text-xl);
  }

  .lede,
  .leer,
  .anzahl,
  .gesamtleer {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  .treffer {
    list-style: none;
    padding: 0;
    margin-block-start: var(--space-2);
  }

  .treffer > li + li {
    margin-block-start: var(--space-3);
  }

  .treffer a {
    color: var(--accent);
  }

  .treffer a:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .snippet {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  mark {
    background: var(--accent-soft);
    color: var(--ink);
    border-radius: 2px;
    padding-inline: 0.1em;
  }

  .status {
    margin-inline-start: var(--space-2);
    color: var(--ink-muted);
    font-size: var(--text-xs);
  }

  @media (max-width: 40rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }
  }
</style>
