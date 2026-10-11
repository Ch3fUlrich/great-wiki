<!--
  The task board, rendered on the generic Board (M8c C3a-2).

  `$lib/components/Board.svelte` is the same rendering before this file existed, and it is
  still what a project's home page mounts; this one is re-founded on `boards/Board.svelte` fed
  through `TaskRowSource`, so a task board and a dataset board share one column, card and drag
  machine. What stays **here** is everything a task board says — the heading and its level, the
  notice a move answers in, the words on a card, and the form a move actually is. Those are the
  interface's contract, not the grid's, and `TaskBoard.test.ts` is the old `Board.test.ts`
  unchanged but for its import: the assertions are the specification being carried over.

  **A drag is still not a second code path.** `submit` finds the very submit button the
  keyboard would have pressed and presses it, exactly as the old `onDrop` did, so a move by
  pointer and a move by form are one submission with one redirect and one announcement.
-->
<script lang="ts">
  import {
    BOARD_NOTICE_ID,
    BOARD_PATH,
    columnsOf,
    describeDue,
    detachedText,
    dueState,
    readOnly,
    readOnlyText,
    STATUSES,
    STATUS_LABEL,
    type BoardNotice,
    type BoardResponse,
    type BoardTask,
    type TaskStatus
  } from '$lib/board';
  import type { Me } from '$lib/api';
  import Board from './Board.svelte';
  import { TaskRowSource } from './TaskRowSource';
  import type { BoardCard, BoardColumn as SourceColumn } from './RowSource';

  interface Props {
    /** The board, exactly as the endpoint answered it. Not re-sorted, not re-filtered. */
    board: BoardResponse;
    /** Who is signed in — the only thing on the wire that bears on moving a card. */
    me: Me | null;
    /**
     * One instant for both renders. Overdue is relative, and a server clock and a browser
     * clock would otherwise disagree about the same card between the first response and
     * hydration — the same reasoning `[...path]/history` gives for its own `now`.
     */
    now: number;
    /** Where a move returns to. Validated server-side by `returnTo` before it is followed. */
    zurueck: string;
    /** The board's heading, and the level it sits at. See the placement notes above. */
    titel: string;
    ebene: 2 | 3;
    /** What just happened on this board, if anything. Built by the loader, not from the URL. */
    hinweis: BoardNotice | null;
    /** Why there is no board. Never conflated with "there is nothing on it". */
    fehler: string | null;
  }

  let { board, me, now, zurueck, titel, ebene, hinweis, fehler }: Props = $props();

  const columns = $derived(columnsOf(board));
  const empty = $derived(columns.every((column) => column.tasks.length === 0));

  const taskOf = (karte: BoardCard): BoardTask => karte.meta!.task as BoardTask;

  /**
   * A drop moves the card through its own submit button rather than through anything of the
   * board's, so pointer and keyboard are one submission with one redirect and one
   * announcement. The generic board declines a drop onto the card's own column and a card the
   * source will not move before ever calling in here, so arriving with no such button means
   * the board lost its form — which is said as a failed move rather than done silently.
   */
  async function submit(taskId: string, status: TaskStatus) {
    const button = document.querySelector<HTMLButtonElement>(
      `button[data-karte="${CSS.escape(taskId)}"][data-ziel="${CSS.escape(status)}"]`
    );
    if (!button) throw new Error(`kein Verschiebe-Knopf für ${taskId} nach ${status}`);
    button.click();
  }

  const source = $derived(TaskRowSource(board, me, submit));

  /** Where a card may go: the two columns it is not already in. */
  function targets(task: BoardTask): TaskStatus[] {
    return STATUSES.filter((status) => status !== task.status);
  }

  /** Which of the three a source column names, so a heading keeps the id it always had. */
  function spaltenIndex(column: SourceColumn): number {
    return STATUSES.indexOf(column.id as TaskStatus);
  }

  /** The `<li>` the generic Board renders carries the card's own identity, as the old one did. */
  function karteAttribute(karte: BoardCard): Record<string, string> {
    const task = taskOf(karte);
    return {
      'data-karte': task.id,
      'data-status': task.status,
      // The dashed border of a detached card (D-8) lives on the `<li>`, which this file no
      // longer writes; the marker travels as an attribute so the rule can still reach it.
      ...(task.detached ? { 'data-abgeloest': 'true' } : {})
    };
  }

  function spalteAttribute(column: SourceColumn): Record<string, string> {
    return { 'data-spalte': String(column.id) };
  }
</script>

<section class="tafel" aria-labelledby="tafel-titel">
  <svelte:element this={`h${ebene}`} id="tafel-titel" class="tafel-titel">{titel}</svelte:element>

  {#if board.project}
    <p class="projekt">
      Projekt auf <a href={board.project.home_path}>{board.project.home_title}</a>
    </p>
  {/if}

  <!-- The answer to whatever was just done here. It is focusable and the redirect points at
       it by fragment, which is what makes it ANNOUNCED rather than merely rendered: a live
       region that is already present when the page loads announces nothing, because a live
       region announces what changes. Moving focus into it reads it out, with no script. -->
  {#if hinweis}
    <p
      id={BOARD_NOTICE_ID}
      class="notice"
      class:notice--ok={hinweis.art === 'ok'}
      class:notice--error={hinweis.art === 'fehler'}
      role={hinweis.art === 'fehler' ? 'alert' : 'status'}
      tabindex="-1"
    >
      {hinweis.text}
    </p>
  {/if}

  {#if fehler}
    <!-- A failed request and a board with nothing on it are different things and are never
         conflated: "hier ist keine Aufgabe zu sehen" about a server that is down is a lie.
         The same distinction /projekte, /graph and the admin console all make. There is then
         no board at all — nothing was answered, so nothing would be honest to render. -->
    <p class="notice notice--error" role="alert">{fehler}</p>
  {:else}
    {#if empty}
      <!-- One sentence for "nothing has been written down yet" and for "none of it is
           yours". The conflation is deliberate, exactly as it is in /projekte's empty state:
           saying which of the two it is would say that something is being withheld. -->
      <p class="leer">
        Hier ist keine Aufgabe zu sehen. Eine Aufgabe entsteht als Zeile mit einem Kästchen
        auf einer Seite — oder als Karte auf der Tafel eines Projekts.
      </p>
    {/if}

    <div class="spalten">
      <Board
        source={source}
        card={card}
        actions={actions}
        columnHead={columnHead}
        emptyColumn={emptyColumn}
        cardAttrs={karteAttribute}
        columnAttrs={spalteAttribute}
      />
    </div>
  {/if}
</section>

{#snippet columnHead(column: SourceColumn, karten: BoardCard[])}
  <svelte:element
    this={`h${ebene + 1}`}
    id={`tafel-spalte-${spaltenIndex(column)}`}
    class="spalte-titel"
  >{STATUS_LABEL[column.id as TaskStatus]}</svelte:element>
{/snippet}

{#snippet emptyColumn(column: SourceColumn)}
  <p class="leer-spalte">Nichts hier.</p>
{/snippet}

{#snippet card(karte: BoardCard)}
  {@const task = taskOf(karte)}
  {@const sperre = readOnly(task, me)}
  {@const faellig = dueState(task.due_at, now)}

  <p class="karte-titel">{task.title}</p>

  {#if task.page}
    <p class="quelle">
      <a href={task.page.path}>{task.page.title}</a>
    </p>
  {:else}
    <!-- Written out rather than left blank. A card made on a board names no page, and that
         is a fact about it, not a missing value. -->
    <p class="quelle quelle--keine">Auf der Tafel angelegt — keine Seite.</p>
  {/if}

  {#if faellig}
    <!-- In WORDS first — "Überfällig seit …" — and coloured second, off the attribute below.
         This codebase holds that line everywhere; the diff views say why about their own
         colours. -->
    <p class="faellig" data-faellig={faellig}>{describeDue(task.due_at, now)}</p>
  {/if}

  {#if task.assignee}
    <!-- The name when the API resolved one, the id when it did not — and the id is not a
         failure state. `assignee_name` is null when this viewer may not be told who the
         person is, and the id is what the card carried before names existed. Dropping the
         row, or printing "Unbekannt" in it, would both destroy the one fact that matters on
         a board — that somebody has this — and leave nothing for anybody to clear. -->
    <p class="wer">
      Zuständig:
      {#if task.assignee_name}
        {task.assignee_name}
      {:else}
        <code>{task.assignee}</code>
      {/if}
    </p>
  {/if}

  {#if task.detached}
    <p class="abgeloest">{detachedText(task)}</p>
  {/if}

  {#if sperre}
    <p class="nurlesbar">{readOnlyText(sperre)}</p>
  {/if}
{/snippet}

{#snippet actions(karte: BoardCard, spalten: SourceColumn[])}
  {@const task = taskOf(karte)}
  {@const sperre = readOnly(task, me)}
  {#if !sperre}
    <!-- A real POST to a form action, with no `use:enhance`: the browser submits, the server
         answers 303 back to wherever this board is, and the whole thing works with JavaScript
         switched off. The action lives at /aufgaben for both placements, which is what keeps
         one implementation of a move rather than one per page. -->
    <form method="post" action={`${BOARD_PATH}?/verschieben`} class="zug">
      <input type="hidden" name="karte" value={task.id} />
      <input type="hidden" name="zurueck" value={zurueck} />
      <span class="zug-titel">Verschieben nach</span>
      {#each targets(task) as ziel (ziel)}
        <button
          type="submit"
          name="status"
          value={ziel}
          class="zug-knopf"
          data-karte={task.id}
          data-ziel={ziel}
          aria-label={`»${task.title}« nach ${STATUS_LABEL[ziel]} verschieben`}
        >
          {STATUS_LABEL[ziel]}
        </button>
      {/each}
    </form>
  {/if}
{/snippet}

<style>
  /* `@layer components`, the plugin contract (ADR 0005). The frame, the card's lines and the
     move form are markup written HERE, so their rules are scoped as usual. The columns, the
     `<ul>` and the `<li>` are written by the generic Board and carry its scope instead, so
     those rules reach them through `:global(...)` — always under `.spalten`, so this board's
     three-column look never restyles a dataset board's columns. */
  @layer components {
    .tafel > * + * {
      margin-block-start: var(--space-4);
    }

    .tafel-titel {
      font-size: var(--text-xl);
      line-height: var(--leading-tight);
    }

    .projekt,
    .leer-spalte,
    .quelle,
    .wer {
      color: var(--ink-muted);
      font-size: var(--text-sm);
    }

    /* --- The three columns ------------------------------------------------------------ */

    /* The generic board is a flex row of columns; a task board is a grid of three, which is
       what makes a column the width of a third of the room rather than of its content. */
    .spalten :global(.board) {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--space-4);
      align-items: start;
      margin-block: 0;
    }

    /* The generic board's own status line is a child of that grid, so it goes under the
       columns rather than becoming a fourth one. */
    .spalten :global(.meldung) {
      grid-column: 1 / -1;
    }

    /* Back to the old column: a block with margins between its parts, not a flex stack with
       a gap, which would double the space the `> * + *` rule below already accounts for. */
    .spalten :global(.spalte) {
      display: block;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--bg-sunken);
      padding: var(--space-3);
      min-inline-size: 0;
    }

    .spalten :global(.spalte > * + *) {
      margin-block-start: var(--space-3);
    }

    .spalte-titel {
      font-size: var(--text-xs);
      text-transform: uppercase;
      letter-spacing: 0.06em;
      font-weight: 650;
      color: var(--ink-muted);
    }

    .spalten :global(.karten) {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
    }

    /* --- One card ---------------------------------------------------------------------- */

    .spalten :global(.karte) {
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
      padding: var(--space-3);
      box-shadow: var(--shadow-sm);
    }

    .spalten :global(.karte > * + *) {
      margin-block-start: var(--space-2);
    }

    .karte-titel {
      font-weight: 650;
      line-height: var(--leading-tight);
      overflow-wrap: anywhere;
    }

    .quelle a {
      color: var(--accent);
    }

    .quelle--keine {
      color: var(--ink-faint);
    }

    .wer code {
      font-family: var(--font-mono);
      font-size: var(--text-xs);
      overflow-wrap: anywhere;
    }

    /* The COLOUR of a due date, and nothing else — the state is spelled out in the text of
       the very same element, so this adds emphasis and never meaning. */
    .faellig {
      font-size: var(--text-sm);
      color: var(--ink-muted);
    }

    .faellig[data-faellig='überfällig'] {
      color: var(--danger);
      font-weight: 650;
    }

    .faellig[data-faellig='heute'] {
      color: var(--warn);
      font-weight: 650;
    }

    /* D-8. Dashed rather than merely tinted, and the sentence inside says the same thing:
       a marker only a sighted reader notices is not a marker. */
    .spalten :global(.karte[data-abgeloest]) {
      border-style: dashed;
      border-color: var(--warn);
    }

    .abgeloest,
    .nurlesbar {
      font-size: var(--text-xs);
      color: var(--ink-muted);
      border-inline-start: 3px solid var(--warn);
      padding-inline-start: var(--space-2);
    }

    .nurlesbar {
      border-inline-start-color: var(--border-strong);
    }

    /* --- Moving one -------------------------------------------------------------------- */

    .zug {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-1);
      border-block-start: 1px solid var(--border);
      padding-block-start: var(--space-2);
    }

    .zug-titel {
      color: var(--ink-faint);
      font-size: var(--text-xs);
      inline-size: 100%;
    }

    .zug-knopf {
      padding: var(--space-1) var(--space-2);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg);
      color: var(--accent);
      font: inherit;
      font-size: var(--text-xs);
      cursor: pointer;
    }

    .zug-knopf:hover,
    .zug-knopf:focus-visible {
      background: var(--accent-soft);
    }

    /* --- Notices ----------------------------------------------------------------------- */

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

    .notice--ok {
      border-inline-start-color: var(--ok);
      color: var(--ink);
    }

    /* The notice takes focus after a move so that it is read out. Without this it would take
       focus with no ring, which is a reader landing somewhere invisible. */
    .notice:focus-visible {
      outline: 2px solid var(--focus);
      outline-offset: 2px;
    }

    /* Three columns side by side stop being readable long before a phone. Below this they
       stack, which also puts the card you are looking for above the fold rather than in a
       third of a screen's width. */
    @media (max-width: 60rem) {
      .spalten :global(.board) {
        grid-template-columns: minmax(0, 1fr);
      }
    }
  }
</style>
