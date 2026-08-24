<!--
  Aufgaben (D-12): the global board — every task you may see, optionally narrowed to one
  project.

  The owner chose this **as well as** a board on each project's home page, over one global
  board alone (a project's own page is where you look when you are thinking about that
  project, and sending you elsewhere breaks that) and over per-project boards alone (a task
  belonging to no project would then have nowhere to appear at all, which is exactly how a
  to-do goes missing — the failure D-6 exists to prevent).

  **The cost D-12 named is two places that must agree**, and it is paid in one place: the
  board itself is `$lib/components/Board.svelte`, mounted here and in `[...path]/+page.svelte`
  with different props and no different rules. Everything this page adds is the frame around
  it — a heading, a lede, and the project filter.

  **Nothing on this page needs a script.** The filter is a plain GET form, so choosing a
  project is a navigation; a move is a POST to a form action. A control that only works once
  JavaScript arrives is a control that looks live and does nothing, which is what
  `[...path]/+page.svelte` records about its own edit link.
-->
<script lang="ts">
  import Board from '$lib/components/Board.svelte';

  let { data } = $props();

  const titel = $derived(
    data.projekt ? `Aufgaben zu »${data.projekt.home_title}«` : 'Alle Aufgaben'
  );
</script>

<svelte:head><title>Aufgaben — great-wiki</title></svelte:head>

<main id="content" class="page">
  <h1>Aufgaben</h1>
  <p class="lede">
    Jede Aufgabe steht als Zeile mit einem Kästchen auf einer Seite — oder als Karte auf der
    Tafel eines Projekts. Es erscheint nur, was Sie auch lesen dürfen.
  </p>

  {#if data.projekteFehler}
    <!-- The filter's options are missing, not the board. Said plainly rather than rendered
         as "es gibt keine Projekte", which would be a claim about pages this reader may not
         read. The board below is unaffected and still shows everything. -->
    <p class="notice notice--error" role="alert">{data.projekteFehler}</p>
  {:else if data.projects.length > 0}
    <!-- A plain GET form: choosing a project is a navigation, the address bar carries it,
         and the back button walks through it. `?projekt=` is German like `/graph`'s
         `?wurzel=` — this is a German interface and the address bar is part of it. -->
    <form method="get" action="/aufgaben" class="filter">
      <label for="projekt">Projekt</label>
      <select id="projekt" name="projekt">
        <option value="" selected={!data.projekt}>Alle Projekte</option>
        {#each data.projects as project (project.id)}
          <option value={project.id} selected={data.projekt?.id === project.id}>
            {project.home_title}
          </option>
        {/each}
      </select>
      <button type="submit" class="btn">Anzeigen</button>
    </form>
  {/if}

  {#if data.filterUnbekannt}
    <!-- Said, rather than silently ignored — and said without confirming or denying that
         such a project exists anywhere. "Gibt es nicht" and "gehört nicht Ihnen" read the
         same here, which is the same conflation every empty state in this interface makes. -->
    <p class="notice" role="status">
      Der gewählte Filter gehört zu keinem Projekt, das hier zu sehen ist. Es werden alle
      Aufgaben gezeigt.
    </p>
  {/if}

  <Board
    board={data.board}
    me={data.me}
    now={data.now}
    zurueck={data.zurueck}
    {titel}
    ebene={2}
    hinweis={data.hinweis}
    fehler={data.fehler}
  />
</main>

<style>
  .page {
    max-width: 78rem;
    margin-inline: auto;
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

  .filter {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-raised);
    padding: var(--space-3) var(--space-4);
  }

  label {
    font-size: var(--text-sm);
    font-weight: 650;
  }

  select {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink);
    font: inherit;
    font-size: var(--text-sm);
    max-inline-size: 100%;
  }

  .btn {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--accent);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .btn:hover,
  .btn:focus-visible {
    background: var(--accent-soft);
  }

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

  @media (max-width: 48rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }
  }
</style>
