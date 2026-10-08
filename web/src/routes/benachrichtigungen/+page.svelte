<!--
  Benachrichtigungen: the inbox. Built from the API's fields only (actor name, page title and
  path); the API already dropped what this reader may no longer see. "Alle als gelesen
  markieren" is a plain POST form, so it works without JavaScript.
-->
<script lang="ts">
  import { describe } from '$lib/notifications';

  let { data } = $props();
  const unread = $derived(data.notifications.filter((n) => !n.read).length);
</script>

<svelte:head><title>Benachrichtigungen — great-wiki</title></svelte:head>

<main id="content" class="page">
  <h1>Benachrichtigungen</h1>

  {#if data.fehler}
    <p class="notice notice--error" role="alert">{data.fehler}</p>
  {:else if data.notifications.length === 0}
    <p class="empty">Keine Benachrichtigungen.</p>
  {:else}
    {#if unread > 0}
      <form method="post" action="?/alleGelesen">
        <button type="submit" class="btn">Alle als gelesen markieren</button>
      </form>
    {/if}
    <ul class="list">
      {#each data.notifications as n (n.id)}
        {@const d = describe(n)}
        <li class:unread={!n.read}>
          {#if !n.read}<span class="sr-only">Ungelesen: </span>{/if}
          {#if d.actor}<strong>{d.actor}</strong>{/if}
          {d.text}
          {#if n.page.path}<a href={n.page.path}>{d.page}</a>{:else}{d.page}{/if}
          <time datetime={n.created_at}>{n.created_at}</time>
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  .list {
    list-style: none;
    padding: 0;
    margin: var(--space-3) 0 0;
  }
  li {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    color: var(--ink-faint);
  }
  li.unread {
    background: var(--bg-sunken);
    border-inline-start: 3px solid var(--border-strong);
    color: var(--ink);
    font-weight: 600;
  }
  time {
    margin-inline-start: var(--space-2);
    font-size: var(--text-xs);
    color: var(--ink-faint);
    font-weight: normal;
  }
  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
