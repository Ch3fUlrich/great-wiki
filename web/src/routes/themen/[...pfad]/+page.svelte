<!--
  One topic: the pages under it, and the topics inside it.

  D-4 kept topics out of the graph and named the consequence — this page is the ONLY way a
  topic is reachable — which sets the bar for it: everything the API listed has to be here and
  reachable from here, including the topics inside this one, or the reader hits a dead end
  with no other route.

  **Opening a topic shows everything inside it.** That is the store's decision, recorded in
  `gw-store/src/topics.rs` with its reasoning, and this page states it in words rather than
  leaving the reader to work out why a page filed under `Rundgang/Tabellen` is on the
  `Rundgang` list. It is not re-derived here and it is not optional.

  **Nothing here filters, and the one thing it could add is a number about what was hidden.**
  There is none. The counts beside the subtopics are the lengths of the lists this reader
  would be handed — the one number ADR 0011 licenses.

  **Nothing here needs a script.** It is a heading, a trail of links, a list of links, and the
  same topic tree the index and the sidebar render.
-->
<script lang="ts">
  import TopicTree from '$lib/components/TopicTree.svelte';
  import { chromeHref } from '$lib/tabs';
  import { ancestryOf, topicHref, TOPICS_PATH } from '$lib/topics';

  let { data } = $props();

  const thema = $derived(data.thema);
  /** The topics above this one, assembled from the two spellings the API answered with. */
  const oben = $derived(ancestryOf(thema.topic));

  /** Where a link on this page goes: the same workspace, the same sidebar. See `chromeHref`. */
  function gehZu(target: string): string {
    return chromeHref(target, data.tabHrefs ?? [], data.hier ?? TOPICS_PATH, data.seitenleiste);
  }
</script>

<svelte:head><title>{thema.topic.display_path} — Themen — great-wiki</title></svelte:head>

<main id="content" class="page">
  {#if oben.length > 0}
    <!-- Rendered only when there IS something above: an empty trail with a lone »Themen« in
         it would be a control that looks like a hierarchy and describes none. The index is
         reachable from the header on every page anyway. -->
    <nav class="pfad" aria-label="Übergeordnete Themen">
      <ol>
        {#each oben as schritt (schritt.path)}
          <li><a href={gehZu(topicHref(schritt))}>{schritt.name}</a></li>
        {/each}
      </ol>
    </nav>
  {/if}

  <h1>{thema.topic.name}</h1>

  <p class="lede">
    Seiten unter »{thema.topic.display_path}« — auch die, die in einem Thema darunter stehen.
    Es erscheint nur, was Sie auch lesen dürfen.
  </p>

  {#if thema.documents.length === 0}
    <!-- Unreachable through the API, which answers 404 for a topic with nothing readable
         under it rather than an empty listing — see ADR 0011, where that conflation IS the
         decision. Rendered anyway, because a page that would show a bare heading if the
         answer ever changed shape is a page that says nothing about what happened. -->
    <p class="leer">Unter diesem Thema steht hier keine Seite.</p>
  {:else}
    <ul class="seiten">
      {#each thema.documents as dokument (dokument.path)}
        <li>
          <a href={gehZu(dokument.path)}>{dokument.title}</a>
          <span class="pfad-klein">{dokument.path}</span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if thema.children.length > 0}
    <!-- The SAME component the index and the sidebar render, over the children this topic's
         own answer carried. One rendering of a topic list, in all three places. -->
    <TopicTree
      topics={thema.children}
      titel="Themen darin"
      ebene={2}
      fehler={null}
      hrefFor={gehZu}
    />
  {/if}
</main>

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
    letter-spacing: -0.02em;
    /* Tight to the trail above it: the trail belongs to the title, not to the page. */
    margin-block-start: var(--space-2);
  }

  .lede {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  /* --- The trail up ------------------------------------------------------------------- */

  .pfad ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1);
    font-size: var(--text-sm);
  }

  /* The separator is decoration and is generated rather than written into the markup: a
     reader of the list hears the names, not a row of slashes. */
  .pfad li + li::before {
    content: '/';
    color: var(--ink-faint);
    margin-inline-end: var(--space-1);
  }

  .pfad a {
    color: var(--ink-muted);
    text-decoration: none;
  }

  .pfad a:hover,
  .pfad a:focus-visible {
    color: var(--ink);
    text-decoration: underline;
    text-underline-offset: 0.15em;
  }

  /* --- The pages under it -------------------------------------------------------------- */

  .seiten {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr));
    gap: var(--space-3) var(--space-6);
  }

  .seiten a {
    color: var(--accent);
    text-decoration: none;
  }

  .seiten a:hover,
  .seiten a:focus-visible {
    text-decoration: underline;
    text-underline-offset: 0.15em;
  }

  /* The address under the title, because two pages in this wiki can share a title and the
     path is what tells them apart. */
  .pfad-klein {
    display: block;
    color: var(--ink-faint);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }

  .leer {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-inline-start-width: 3px;
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  @media (max-width: 48rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }
  }
</style>
