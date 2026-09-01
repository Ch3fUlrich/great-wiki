<!--
  The topics — **the one component both placements render.**

  The owner put browsing by topic in two places: a page at `/themen`, listing every topic you
  may see, and a switcher in the shell's sidebar, so the page tree and the topics are two ways
  through one corpus. That is the same shape D-12 gave the board and it carries the same cost:
  two places that must agree. The answer is the same answer too — **one query, rendered
  twice**. `GET /api/topics` is asked once, in `+layout.server.ts`; the index and the sidebar
  are two mounts of this file, differing only in the heading, the level it sits at and whether
  a topic is being looked at. The suggestion list beside a page's chips is the third rendering
  of that same array, which is what makes ADR 0011's "a suggestion list is a disclosure
  surface" structurally true here rather than a promise somebody has to keep.

  **Everything here is already filtered, and nothing here filters again.** A topic exists, for
  a given caller, exactly when they may read at least one document under it or under a topic
  inside it — decided in `Store::topics_for`, where it is mutation-tested. The one thing this
  file could add that the filtering cannot take back is a NUMBER about what was left out: a
  total, an "und 3 weitere". There is none, and `TopicTree.test.ts` asserts its absence rather
  than trusting this comment. The count that IS rendered is the length of the very list this
  caller would be handed, which is the one number ADR 0011 licenses.

  **Nothing here needs a script.** It is a list of links.
-->
<script lang="ts">
  import TopicList from './TopicList.svelte';
  import { treeOf, type TopicSummary } from '$lib/topics';

  interface Props {
    /** The index, exactly as the endpoint answered it. Not re-sorted, not re-filtered. */
    topics: TopicSummary[];
    /** The heading, and the level it sits at. See the placement notes above. */
    titel: string;
    ebene: 2 | 3;
    /** Why there is no list. Never conflated with "there is nothing in it". */
    fehler: string | null;
    /** The topic being looked at, by canonical path, when one is. */
    current?: string;
    /** What a topic's entry links to — the shell passes one; see `TopicList`. */
    hrefFor?: (href: string) => string;
  }

  let { topics, titel, ebene, fehler, current, hrefFor }: Props = $props();

  const tree = $derived(treeOf(topics));
</script>

<!-- `aria-label` rather than `aria-labelledby`: this component is mounted twice on the same
     page (the sidebar and the index), and a hardcoded id would be two elements with one id
     and a landmark pointing at the wrong heading. The two placements pass different titles,
     so the two landmarks have different names, which is what a reader needs anyway. -->
<nav class="themenbaum" aria-label={titel}>
  <svelte:element this={`h${ebene}`} class="themen-titel">{titel}</svelte:element>

  {#if fehler}
    <!-- A failed request and a wiki with nothing filed in it are different things and are
         never conflated: "keine Themen" about a server that is down is a lie. The same
         distinction /projekte, /aufgaben, /graph and the admin console all make. -->
    <p class="notice notice--error" role="alert">{fehler}</p>
  {:else if tree.length === 0}
    <!-- One sentence for "nobody has filed anything yet" and for "none of it is yours". The
         conflation is deliberate and, here, is the disclosure rule itself: saying which of
         the two it is would say that something is being withheld, which is precisely what
         ADR 0011 keeps back. -->
    <p class="leer">
      Keine Themen. Ein Thema entsteht, indem eine Seite eines nennt — im Feld unter ihrem
      Titel oder als <code>tags:</code> in ihrer Datei.
    </p>
  {:else}
    <TopicList nodes={tree} {current} {hrefFor} />
  {/if}
</nav>

<style>
  /* `@layer components`, the plugin contract (ADR 0005). */
  @layer components {
    .themenbaum > * + * {
      margin-block-start: var(--space-3);
    }

    .themen-titel {
      color: var(--ink-faint);
      font-size: var(--text-xs);
      font-weight: 650;
      text-transform: uppercase;
      letter-spacing: 0.06em;
    }

    code {
      font-family: var(--font-mono);
      font-size: 0.9em;
    }

    .leer,
    .notice {
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--border);
      border-inline-start-width: 3px;
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
      color: var(--ink-muted);
      font-size: var(--text-sm);
      max-width: var(--measure);
    }

    .notice--error {
      border-inline-start-color: var(--danger);
      color: var(--ink);
    }
  }
</style>
