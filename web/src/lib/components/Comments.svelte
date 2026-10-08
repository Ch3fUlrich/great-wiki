<!--
  The page's comment thread (ADR 0025), under the page.

  Whoever may read the page sees the list; the form is offered to signed-in users only (the
  API would refuse an anonymous POST, and a form that always fails is worse than none).
  Resolved threads are collapsed, never deleted, and there is no delete control anywhere:
  the API has no such route. A passage comment whose passage is gone is flagged `orphaned`
  and keeps its quote, so what it was about is not lost with the text.

  The count in »Erledigt (n)« is the number of resolved threads RETURNED, nothing more.
-->
<script lang="ts">
  import {
    ORPHAN_NOTE,
    cleanBody,
    describeWriteFailure,
    fetchThreads,
    groupThreads,
    passageOf,
    postComment,
    setResolved,
    type Comment,
    type Thread
  } from '$lib/comments';

  interface Props {
    path: string;
    threads: Thread[];
    /** Why the list is not there; the page still renders. */
    fehler?: string | null;
    angemeldet: boolean;
  }

  let { path, threads, fehler = null, angemeldet }: Props = $props();

  let current = $state<Thread[] | null>(null);
  let note = $state<string | null>(null);
  let neu = $state('');
  let antworten = $state<Record<string, string>>({});
  let busy = $state(false);

  const list = $derived(current ?? threads);
  const groups = $derived(groupThreads(list));

  async function reload() {
    const r = await fetchThreads(fetch, path);
    if (r.data) current = r.data.threads ?? [];
    else note = describeWriteFailure(r.status);
  }

  async function submit(text: string, parentId?: string) {
    const body = cleanBody(text);
    if (!body || busy) return false;
    busy = true;
    note = null;
    const r = await postComment(fetch, path, { body, parentId });
    busy = false;
    if (r.status >= 200 && r.status < 300) {
      await reload();
      return true;
    }
    note = describeWriteFailure(r.status);
    return false;
  }

  async function toggle(id: string, resolved: boolean) {
    if (busy) return;
    busy = true;
    note = null;
    const r = await setResolved(fetch, id, resolved);
    busy = false;
    if (r.status >= 200 && r.status < 300) await reload();
    else note = describeWriteFailure(r.status);
  }
</script>

{#snippet entry(c: Comment)}
  <p class="meta"><strong>{c.author_name}</strong> <time>{c.created_at}</time></p>
  <p class="body">{c.body}</p>
{/snippet}

{#snippet thread(t: Thread)}
  <li class="thread" class:resolved={t.resolved}>
    {#if passageOf(t)}
      {@const p = passageOf(t)}
      <blockquote class="quote">{p?.quote}</blockquote>
      {#if p?.orphaned}<p class="orphan">{ORPHAN_NOTE}</p>{/if}
    {/if}
    {@render entry(t)}
    {#if t.replies.length > 0}
      <ul class="replies">
        {#each t.replies as r (r.id)}
          <li>{@render entry(r)}</li>
        {/each}
      </ul>
    {/if}
    {#if angemeldet}
      <div class="actions">
        {#if !t.resolved}
          <form
            class="reply"
            onsubmit={async (e) => {
              e.preventDefault();
              if (await submit(antworten[t.id] ?? '', t.id)) antworten[t.id] = '';
            }}
          >
            <label class="sr" for={`gw-antwort-${t.id}`}>Antwort</label>
            <textarea
              id={`gw-antwort-${t.id}`}
              rows="2"
              placeholder="Antworten …"
              bind:value={antworten[t.id]}
            ></textarea>
            <button type="submit" class="btn" disabled={busy}>Antworten</button>
          </form>
        {/if}
        <button type="button" class="btn" disabled={busy} onclick={() => toggle(t.id, !t.resolved)}>
          {t.resolved ? 'Wieder öffnen' : 'Erledigt'}
        </button>
      </div>
    {/if}
  </li>
{/snippet}

<section class="comments no-print" aria-labelledby="gw-comments">
  <h2 id="gw-comments">Kommentare</h2>

  {#if fehler}
    <p class="notice" role="alert">{fehler}</p>
  {:else}
    {#if groups.open.length === 0 && groups.resolved.length === 0}
      <p class="muted">Noch keine Kommentare.</p>
    {/if}
    {#if groups.open.length > 0}
      <ul class="threads">
        {#each groups.open as t (t.id)}
          {@render thread(t)}
        {/each}
      </ul>
    {/if}
    {#if groups.resolved.length > 0}
      <details class="done">
        <summary>Erledigt ({groups.resolved.length})</summary>
        <ul class="threads">
          {#each groups.resolved as t (t.id)}
            {@render thread(t)}
          {/each}
        </ul>
      </details>
    {/if}
  {/if}

  {#if note}<p class="notice" role="alert">{note}</p>{/if}

  {#if angemeldet}
    <form
      class="neu"
      onsubmit={async (e) => {
        e.preventDefault();
        if (await submit(neu)) neu = '';
      }}
    >
      <label for="gw-kommentar-neu">Neuer Kommentar</label>
      <textarea id="gw-kommentar-neu" rows="3" bind:value={neu}></textarea>
      <button type="submit" class="btn" disabled={busy}>Kommentieren</button>
    </form>
  {/if}
</section>

<style>
  @layer components {
    .comments {
      margin-block-start: var(--space-8);
      padding-block-start: var(--space-4);
      border-block-start: 1px solid var(--border);
    }
    .threads,
    .replies {
      list-style: none;
      padding: 0;
      margin: 0;
    }
    .thread {
      padding-block: var(--space-3);
      border-block-end: 1px solid var(--border);
    }
    .replies {
      margin-inline-start: var(--space-4);
    }
    .meta,
    .muted {
      font-size: var(--text-sm);
      color: var(--ink-muted);
      margin: 0;
    }
    .body {
      margin: var(--space-1) 0;
      white-space: pre-wrap;
    }
    .quote {
      margin: 0 0 var(--space-2);
      padding-inline-start: var(--space-3);
      border-inline-start: 3px solid var(--border);
      color: var(--ink-muted);
    }
    .orphan {
      font-size: var(--text-sm);
      color: var(--ink-muted);
      font-style: italic;
      margin: 0 0 var(--space-2);
    }
    .actions,
    .reply,
    .neu {
      display: grid;
      gap: var(--space-2);
      margin-block-start: var(--space-2);
    }
    textarea {
      font: inherit;
      width: 100%;
    }
    .btn {
      font: inherit;
      font-size: var(--text-sm);
      justify-self: start;
      padding: var(--space-1) var(--space-3);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      background: var(--bg);
      color: var(--ink);
      cursor: pointer;
    }
    .sr {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
    }
  }
</style>
