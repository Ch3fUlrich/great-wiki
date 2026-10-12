<!--
  A `dataset-view` block's reader (M8d D2W): the client-side half of the embed. The frame and
  its caption belong to `BlockView`; this component owns what lives inside the frame, and it
  asks the server — under the READER's own identity — whether the dataset behind the block may
  show them anything. See `crates/gw-store/src/datasets/embed.rs` for the answer's shape and
  `./embed.ts` for how it is read.
-->
<script lang="ts">
  import { browser } from '$app/environment';
  import DatasetViewBody from './DatasetViewBody.svelte';
  import { loadDataset } from './load';
  import { resolveEmbed, type EmbedState } from './embed';

  interface Props {
    /** The dataset page's document id, from the block. Never rendered. */
    datasetDoc: string;
    /** The saved view's id, from the block. Never rendered. */
    view: string;
    /** The author's own label. `BlockView` draws it in the caption; kept for parity. */
    label?: string;
    /**
     * The state to render before the client read settles — which, on the server, is the only
     * state there ever is (the fetch is browser-only). Defaults to `loading`, so an
     * SSR-rendered frame carries the same one line the reader sees first. A test passes a
     * terminal state to pin the markup each one produces.
     */
    seed?: EmbedState;
  }

  let { datasetDoc, view, seed = { status: 'loading' } }: Props = $props();

  // `seed` is a value to render with, not a signal to follow: SSR and tests pass a terminal
  // state once, and the browser starts at `loading` and replaces it from the effect below.
  // Capturing only the initial value is exactly right, hence the ignore.
  // svelte-ignore state_referenced_locally
  let state = $state<EmbedState>(seed);

  /**
   * The read, in the browser only, guarded against a stale answer.
   *
   * `$effect` never runs during server rendering, so the first response always carries
   * `seed` and nothing else — an embed cannot leak a title, a count or a path to a reader the
   * server has not yet decided anything about. `label` is deliberately not read here: it is
   * the author's words and belongs in the caption, not in a decision about data the reader may
   * not have.
   *
   * `token` is the staleness guard and `AbortController` the real stop: if the props move on
   * while a read is in flight, the old read's promise resolving must not overwrite the newer
   * one's state, so only the latest `token` is allowed to commit — and the cleanup aborts the
   * superseded request outright.
   */
  let token = 0;
  $effect(() => {
    if (!browser) return;
    const doc = datasetDoc;
    const v = view;
    const mine = ++token;
    const ctrl = new AbortController();
    resolveEmbed(fetch, doc, v, loadDataset, ctrl.signal).then((next) => {
      if (mine === token) state = next;
    });
    return () => ctrl.abort();
  });
</script>

{#if state.status === 'loading'}
  <p class="hinweis">Datenansicht wird geladen …</p>
{:else if state.status === 'unavailable'}
  <!-- NOTHING. Whether the dataset is absent, not a dataset, thrown away, unreadable or a bad
       id is one indistinguishable answer (ADR 0022) and this branch must not be able to tell
       them apart even by its DOM: an empty render, with only the frame and the label the
       caller already drew. No title, no path, no count. -->
{:else if state.status === 'view_missing'}
  <p class="hinweis">Diese Ansicht gibt es nicht mehr.</p>
{:else if state.status === 'not_embeddable'}
  <p class="hinweis">Diese Ansicht kann nicht eingebettet werden.</p>
{:else if state.status === 'error'}
  <p class="fehler" role="alert">Die Datenansicht konnte nicht geladen werden.</p>
{:else}
  <DatasetViewBody path={state.path} dataset={state.load} mayWrite={false} embedded={true} />
{/if}

<style>
  /* The frame's one line, muted and small: a view that has not arrived, is gone, or is not the
     kind to embed is an ordinary state of a page, not an error to look at. Scoped here because
     a child component's elements are outside `BlockView`'s own style scope. */
  .hinweis {
    margin: 0;
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  .fehler {
    margin: 0;
    color: var(--danger, inherit);
    font-size: var(--text-sm);
  }
</style>
