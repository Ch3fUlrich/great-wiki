<!--
  Themen: every topic you may see, and the one place you would go to ask what topics exist.

  D-4 kept topics out of the graph and stated the consequence as a requirement rather than a
  nicety: topics are invisible there, so browsing by topic needs its own view — *"a topic page
  listing its documents. That is the ONLY way topics are reachable."* This is the front door
  to that, linked from the header beside »Projekte«, because a page nothing links to is a page
  nobody finds.

  **There is no `+page.server.ts` beside this file, deliberately.** The owner put topic
  browsing here *and* in the shell's sidebar, and named the cost D-12 named for the board: two
  places that must agree. They agree by being **one query rendered twice** —
  `+layout.server.ts` asks `GET /api/topics` once per render, this page and the sidebar are
  two mounts of `TopicTree.svelte` over that one answer, and the suggestion list beside a
  page's chips is a third rendering of it. A loader here would be a second answer to "which
  topics exist", and because a topic's own NAME is the disclosure (ADR 0011), a second answer
  is also a second chance to leak one — and the second one is always the one that forgets the
  filter.

  **Nothing here filters anything, and the one thing it could add is a number.** A total, an
  "N ausgeblendet", would be a fact about pages the reader may not read, which is precisely
  what the filtering cannot take back. There is none, and `page.test.ts` asserts its absence
  rather than trusting this comment. The counts that ARE here are the length of the list this
  reader would be handed — the one number ADR 0011 licenses.

  **One hazard, the same one `/projekte` and `/aufgaben` write down.** `/themen` is a literal
  segment and SvelteKit prefers it over `[...path]`, so a wiki page that ever lives at
  `/themen` is shadowed by this route. Nothing in `content-example` is named that. If a page
  ever is, this route's address has to move — not the page.
-->
<script lang="ts">
  import TopicTree from '$lib/components/TopicTree.svelte';
  import { chromeHref } from '$lib/tabs';
  import { TOPICS_PATH } from '$lib/topics';

  let { data } = $props();

  /**
   * Where a topic link from this page goes: the same workspace, and the same sidebar.
   *
   * The sidebar is the other placement, so following a topic from here while the sidebar is
   * showing topics must not snap it back to the page tree — the switcher would then work
   * exactly once. See `chromeHref`.
   */
  function gehZu(target: string): string {
    return chromeHref(target, data.tabHrefs ?? [], data.hier ?? TOPICS_PATH, data.seitenleiste);
  }
</script>

<svelte:head><title>Themen — great-wiki</title></svelte:head>

<main id="content" class="page">
  <h1>Themen</h1>
  <p class="lede">
    Ein Thema ist ein Schlagwort, das eine Seite über sich selbst sagt. Themen schachteln
    sich: <code>Medizin/Darm</code> ist »Darm« innerhalb von »Medizin«, und wer »Medizin«
    öffnet, sieht auch alles, was darunter liegt. Es erscheint nur, was Sie auch lesen dürfen.
  </p>

  <TopicTree
    topics={data.themen ?? []}
    titel="Alle Themen"
    ebene={2}
    fehler={data.themenFehler ?? null}
    hrefFor={gehZu}
  />
</main>

<style>
  /* The tree wants the room; the lede is a sentence and stays at the measure. The same split
     `/aufgaben` makes between its board and its own lede. */
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
  }

  .lede {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.95em;
  }

  @media (max-width: 48rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }
  }
</style>
