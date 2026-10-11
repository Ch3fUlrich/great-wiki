<script lang="ts">
  /**
   * The generic board over a RowSource (M8c): datasets and tasks render through this one
   * component. Snippets are the extension points; every value reaches the page as TEXT.
   */
  import type { Snippet } from 'svelte';
  import type { BoardCard, BoardColumn, RowSource } from './RowSource';
  import { applyMove, viewFromCards, type BoardView } from './move';

  let {
    source,
    hooks = {},
    card: cardSnippet,
    actions,
    columnHead,
    empty
  }: {
    source: RowSource;
    hooks?: { onMoved?(c: BoardCard): void; onStale?(cur: BoardCard | null): void };
    card?: Snippet<[BoardCard]>;
    actions?: Snippet<[BoardCard, BoardColumn[]]>;
    columnHead?: Snippet<[BoardColumn, BoardCard[]]>;
    empty?: Snippet;
  } = $props();

  // svelte-ignore state_referenced_locally -- the board is built once from the source it is given
  let view = $state<BoardView>(viewFromCards(source.cards()));

  const columns = $derived(source.columns());
  // The view's column wins over the card's, so the board moves without mutating the source.
  const groups = $derived.by(() => {
    const g = new Map<string | null, BoardCard[]>(columns.map((col) => [col.id, []]));
    for (const c of source.cards()) {
      const card = { ...c, column: view.columnOf.get(c.id) ?? null };
      (g.get(card.column) ?? g.get(null)!).push(card);
    }
    return g;
  });
  const hasCards = $derived(source.cards().length > 0);

  async function move(card: BoardCard, column: string | null) {
    view = await applyMove(source, view, card, column, hooks);
  }

  let dragged: string | null = null;

  const onDragStart = (event: DragEvent, card: BoardCard) => {
    dragged = card.id;
    event.dataTransfer?.setData('text/plain', card.id);
  };
  const onDrop = (event: DragEvent, column: string | null) => {
    event.preventDefault();
    const id = event.dataTransfer?.getData('text/plain') || dragged;
    const card = id ? source.cards().find((c) => c.id === id) : undefined;
    dragged = null;
    if (card) void move(card, column);
  };
</script>

<section class="board" data-board aria-label="Board">
  {#each columns as col (String(col.id))}
    {@const cards = groups.get(col.id) ?? []}
    <div class="spalte" role="group" aria-label={col.label} data-column={col.id ?? 'none'} ondragover={(event: DragEvent) => event.preventDefault()} ondrop={(event: DragEvent) => onDrop(event, col.id)}>
      {#if columnHead}
        {@render columnHead(col, cards)}
      {:else}
        <h3>{col.label}</h3>
      {/if}
      {#each cards as c (c.id)}
        {@const movable = source.canMove(c)}
        <article
          class="karte"
          data-card={c.id}
          data-readonly={!movable || undefined}
          draggable={movable ? "true" : undefined}
          ondragstart={(event: DragEvent) => onDragStart(event, c)}
        >
          {#if cardSnippet}
            {@render cardSnippet(c)}
          {:else}
            <p class="titel">{c.title}</p>
          {/if}
          {#if actions}
            {@render actions(c, columns)}
          {:else if movable}
            <select
              aria-label="Verschieben nach"
              onchange={(event: Event) => void move(c, (event.target as HTMLSelectElement).value === 'none' ? null : (event.target as HTMLSelectElement).value)}
            >
              {#each columns as option (String(option.id))}
                <option value={option.id ?? 'none'} selected={(option.id ?? 'none') === (view.columnOf.get(c.id) ?? 'none')}>{option.label}</option>
              {/each}
            </select>
          {/if}
        </article>
      {/each}
    </div>
  {/each}
  {#if empty && !hasCards}
    {@render empty()}
  {/if}
  <p class="meldung" role="status" data-board-message>{view.message ?? ''}</p>
</section>

<style>
  @layer components {
    .board {
      display: flex;
      gap: var(--space-4);
      overflow-x: auto;
      margin-block: var(--space-4);
    }
    .spalte {
      flex: 1 0 12rem;
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
    }
    .karte {
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      padding: var(--space-2) var(--space-3);
    }
    .karte[data-readonly] {
      opacity: 0.8;
    }
    .meldung {
      margin: 0;
    }
  }
</style>
