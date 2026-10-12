<!--
  A `dataset-view` block's reader (M8d D2W): the RESOLVED state of the embed, drawn. The
  frame and its caption belong to `BlockView`; the read belongs to `+page.server.ts`, which
  asked the embed endpoint and the row read under the READER's own cookie before this markup
  existed. This component fetches nothing and decides nothing — what the reader may see is
  entirely in the `state` value it was handed, and what that value does not hold (a title, a
  path, a count) cannot appear here.
-->
<script lang="ts">
  import DatasetViewBody from './DatasetViewBody.svelte';
  import type { EmbedState } from './embed';

  interface Props {
    /**
     * The server's answer for this block. Required: there is no client-side read to wait
     * for, so there is no state to start in and replace. `BlockView` passes
     * `{ status: 'unavailable' }` for a block the page never resolved, which renders
     * exactly like a refusal — deliberately indistinguishable (ADR 0022).
     */
    state: EmbedState;
  }

  let { state }: Props = $props();
</script>

{#if state.status === 'unavailable'}
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
  /* The frame's one line, muted and small: a view that is gone, or is not the kind to embed,
     is an ordinary state of a page, not an error to look at. Scoped here because a child
     component's elements are outside `BlockView`'s own style scope. */
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
