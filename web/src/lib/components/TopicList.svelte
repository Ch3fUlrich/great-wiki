<!--
  One level of the topic hierarchy, and the levels inside it.

  Split out of `TopicTree.svelte` rather than written as a recursive snippet for the reason
  `Tree.svelte` is: a component that imports itself is the pattern this repository already
  uses for exactly this shape, and the chrome around the tree — the heading, the failure, the
  empty state — must appear once rather than once per level.

  **The nesting is in the MARKUP.** A nested `<ul>` inside the parent's `<li>`, not a flat
  list with padding: indentation is a fact about pixels, and a reader who is not looking at
  them still has to be told that `Tabellen` sits inside `Rundgang`. That is also why each
  topic is named by its LEAF here — the list around it is what says where it sits — where a
  chip on a page spells the whole path, because a chip has no list around it.
-->
<script lang="ts">
  import Self from './TopicList.svelte';
  import { countText, topicHref, type TopicNode } from '$lib/topics';

  interface Props {
    nodes: TopicNode[];
    /** The topic being looked at, by canonical path, so it can be marked. */
    current?: string;
    /**
     * What a topic's entry links to, when that is not simply the topic's own address.
     *
     * The shell passes one so a link here keeps the open workspace and the sidebar's own
     * choice: following it navigates the ACTIVE tab rather than collapsing the strip.
     * Optional and the identity by default, so this component is unchanged wherever no
     * workspace surrounds it.
     */
    hrefFor?: (href: string) => string;
  }

  let { nodes, current, hrefFor }: Props = $props();
</script>

{#if nodes.length}
  <ul>
    {#each nodes as node (node.topic.path)}
      <li>
        <a
          href={hrefFor ? hrefFor(topicHref(node.topic)) : topicHref(node.topic)}
          aria-current={node.topic.path === current ? 'page' : undefined}
        >
          <span class="name">{node.topic.name}</span>
          <!-- The only number this interface renders about a topic, and it is the length of
               the list this very reader would be handed — never a total and never a count of
               what was left out. See `countText`, which has nowhere for a second number to
               arrive. -->
          <span class="anzahl">{countText(node.topic.documents)}</span>
        </a>
        <Self nodes={node.children} {current} {hrefFor} />
      </li>
    {/each}
  </ul>
{/if}

<style>
  /* `@layer components`, the plugin contract (ADR 0005) — the same layer Tree, Backlinks and
     Board put their rules in. */
  @layer components {
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
    }

    /* Nested levels get a guide line, exactly as the page tree does, so depth is visible
       without counting indents. The line is the redundant channel; the nested list is what
       actually carries the structure. */
    :global(li) ul {
      margin-inline-start: var(--space-3);
      padding-inline-start: var(--space-3);
      border-inline-start: 1px solid var(--border);
    }

    a {
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: var(--space-2);
      padding: var(--space-1) var(--space-2);
      border-radius: var(--radius-sm);
      color: var(--accent);
      text-decoration: none;
      line-height: 1.4;
    }

    a:hover {
      background: var(--bg-sunken);
      text-decoration: underline;
      text-underline-offset: 0.15em;
    }

    .anzahl {
      color: var(--ink-faint);
      font-size: var(--text-xs);
      font-variant-numeric: tabular-nums;
    }

    /* The topic being looked at is marked by weight and a background, not by colour alone —
       colour alone fails for anyone who cannot distinguish these two hues. */
    a[aria-current='page'] {
      background: var(--accent-soft);
      color: var(--ink);
      font-weight: 650;
    }

    a[aria-current='page'] .anzahl {
      color: var(--ink-muted);
    }
  }
</style>
