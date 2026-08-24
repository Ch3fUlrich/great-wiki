<!--
  The board — **the one component both of D-12's placements render.**

  D-12 put a board at `/aufgaben`, filterable by project, *and* a board on every project's
  home page, and it named the cost in the same breath: two places that must agree. The answer
  it gave is that they are one query with a filter, not two implementations. This file is the
  other half of that answer: they are also one rendering. `/aufgaben/+page.svelte` and
  `[...path]/+page.svelte` both mount this, differing only in what they hand it — the heading,
  the level it sits at, and the path a move comes back to.

  **Everything here is already filtered, and nothing here filters again.** `GET /api/board`
  answers only the cards whose governing page the caller may read, per document, and that
  belongs to `Store::board_for` where it is mutation-tested. The one thing this file could add
  that the filter cannot take back is a NUMBER — a total, a "3 Karten", an "N ausgeblendet" —
  because that would be a fact about pages the reader may not read. There is none, and
  `Board.test.ts` asserts its absence rather than trusting this comment.

  **A card that cannot be moved is shown and marked, never hidden.** The owner chose that, and
  it is the right way round: the checkbox is already visible on the page the line was written
  on, so hiding the card hides nothing — and a task that silently vanishes from a board is the
  failure this whole design keeps trying to avoid. A detached card (D-8) is kept for the same
  reason, and says so in words: its page no longer holds the line, but the due date and the
  assignee somebody set are still real.

  **Nothing on this board needs a script.** A move is a form submission to a form action, so
  it works with JavaScript switched off — the pattern `/projekte` ships and the reason the
  edit control on a page is a link rather than a button. Dragging is added on top and uses the
  very same buttons: a drop finds the button it would have pressed and presses it, so there is
  one code path for a move and dragging can never be the only way to make one.
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

  /** Where a card may go: the two columns it is not already in. */
  function targets(task: BoardTask): TaskStatus[] {
    return STATUSES.filter((status) => status !== task.status);
  }

  // --- Dragging, which is an addition and never the only way ------------------------------
  //
  // A drop does not change anything by itself. It finds the submit button that would have
  // moved this card to this column and presses it, so the state change goes through exactly
  // the form submission the keyboard uses — one code path, one set of hidden fields, one
  // redirect, one announcement. A card with no such button (already in this column, or
  // read-only) simply produces nothing, which is the same answer the buttons give.

  function onDragStart(event: DragEvent, task: BoardTask) {
    if (!event.dataTransfer) return;
    event.dataTransfer.setData('text/plain', task.id);
    event.dataTransfer.effectAllowed = 'move';
  }

  function onDragOver(event: DragEvent) {
    // Without this the browser refuses the drop outright — the default for most elements is
    // "not a drop target", and cancelling the event is what says otherwise.
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  }

  function onDrop(event: DragEvent, ziel: TaskStatus) {
    event.preventDefault();
    const id = event.dataTransfer?.getData('text/plain');
    if (!id) return;
    const button = document.querySelector<HTMLButtonElement>(
      `button[data-karte="${CSS.escape(id)}"][data-ziel="${CSS.escape(ziel)}"]`
    );
    button?.click();
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
         The same distinction /projekte, /graph and the admin console all make. -->
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
      {#each columns as column, index (column.status)}
        <section
          class="spalte"
          aria-labelledby={`tafel-spalte-${index}`}
          data-spalte={column.status}
          ondragover={onDragOver}
          ondrop={(event) => onDrop(event, column.status)}
        >
          <svelte:element this={`h${ebene + 1}`} id={`tafel-spalte-${index}`} class="spalte-titel"
            >{STATUS_LABEL[column.status]}</svelte:element
          >

          {#if column.tasks.length === 0}
            <p class="leer-spalte">Nichts hier.</p>
          {:else}
            <ul class="karten">
              {#each column.tasks as task (task.id)}
                {@const sperre = readOnly(task, me)}
                {@const faellig = dueState(task.due_at, now)}
                <li
                  class="karte"
                  class:karte--abgeloest={task.detached}
                  data-karte={task.id}
                  data-status={task.status}
                  draggable={sperre ? undefined : 'true'}
                  ondragstart={(event) => onDragStart(event, task)}
                >
                  <p class="karte-titel">{task.title}</p>

                  {#if task.page}
                    <p class="quelle">
                      <a href={task.page.path}>{task.page.title}</a>
                    </p>
                  {:else}
                    <!-- Written out rather than left blank. A card made on a board names no
                         page, and that is a fact about it, not a missing value. -->
                    <p class="quelle quelle--keine">Auf der Tafel angelegt — keine Seite.</p>
                  {/if}

                  {#if faellig}
                    <!-- In WORDS first — "Überfällig seit …" — and coloured second, off the
                         attribute below. This codebase holds that line everywhere; the diff
                         views say why about their own colours. -->
                    <p class="faellig" data-faellig={faellig}>{describeDue(task.due_at, now)}</p>
                  {/if}

                  {#if task.assignee}
                    <!-- The name when the API resolved one, the id when it did not — and the
                         id is not a failure state. `assignee_name` is null when this viewer
                         may not be told who the person is (they may no longer read the page
                         this card is governed by, or the account is suspended), and the id is
                         what the card carried before names existed. Dropping the row, or
                         printing "Unbekannt" in it, would both destroy the one fact that
                         matters on a board — that somebody has this — and leave nothing for
                         anybody to clear. The `<code>` is kept for the id, because an id is a
                         handle you type somewhere; a name is not. -->
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
                  {:else}
                    <!-- A real POST to a form action, with no `use:enhance`: the browser
                         submits, the server answers 303 back to wherever this board is, and
                         the whole thing works with JavaScript switched off. The action lives
                         at /aufgaben for both placements, which is what keeps one
                         implementation of a move rather than one per page. -->
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
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/each}
    </div>
  {/if}
</section>

<style>
  /* `@layer components`, the plugin contract (ADR 0005) — the same layer Backlinks,
     Breadcrumb and Subpages put their rules in. */
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

    .spalten {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--space-4);
      align-items: start;
    }

    .spalte {
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--bg-sunken);
      padding: var(--space-3);
      min-inline-size: 0;
    }

    .spalte > * + * {
      margin-block-start: var(--space-3);
    }

    .spalte-titel {
      font-size: var(--text-xs);
      text-transform: uppercase;
      letter-spacing: 0.06em;
      font-weight: 650;
      color: var(--ink-muted);
    }

    .karten {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
    }

    /* --- One card ---------------------------------------------------------------------- */

    .karte {
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
      padding: var(--space-3);
      box-shadow: var(--shadow-sm);
    }

    .karte > * + * {
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
    .karte--abgeloest {
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
      .spalten {
        grid-template-columns: minmax(0, 1fr);
      }
    }
  }
</style>
