<script lang="ts">
  import { goto } from '$app/navigation';
  // `formatInstant` is imported from the admin client rather than copied: it is a pure
  // formatter that deliberately avoids `Intl` (two ICU builds and two time zones between
  // server render and hydration is a mismatch nobody would think to suspect), and a second
  // spelling of it here would drift from that one the first time either is touched.
  import { formatInstant } from '$lib/adminApi';
  import {
    attributeLabel,
    blockLabel,
    CHANGE_LABEL,
    CHANGE_MARK,
    formatDelta,
    relativeTime,
    sizeDelta,
    VIEWS,
    VIEW_HINT,
    VIEW_LABEL,
    type RevisionSummary,
    type StructureChange,
    type View
  } from '$lib/history';

  let { data } = $props();

  /**
   * Whether to offer restoring at all, and why the answer is this crude.
   *
   * Exactly the reasoning `[...path]/+page.svelte` records for the edit control, and the
   * same one bit is missing: nothing on the wire says "may I write this page". Restoring
   * needs write, `/api/me` reports groups and a baseline, and D-M2-8 is explicit that no
   * baseline confers write — so the only thing that knows is the endpoint itself. The
   * control is therefore offered to whoever is signed in and the true answer is given the
   * moment it is pressed: a refusal produces a sentence, never a silent failure and never a
   * restore. The offer can be false; the restore cannot.
   */
  const mayRestore = $derived(data.me?.authenticated === true);

  const historyUrl = $derived(`${data.doc.path}/history`);

  /**
   * A link to this page in another state.
   *
   * Absolute rather than a bare `?…`, so a link is the same string wherever it is rendered
   * and a test can assert it. The current selection and view are carried through every link
   * — losing the comparison because somebody opened the source tab would be its own small
   * betrayal.
   */
  function href(overrides: Record<string, string | null> = {}): string {
    const params = new URLSearchParams();
    if (data.from) params.set('von', data.from.id);
    if (data.to) params.set('bis', data.to.id);
    if (data.view !== 'prosa') params.set('ansicht', data.view);
    for (const [key, value] of Object.entries(overrides)) {
      if (value === null) params.delete(key);
      else params.set(key, value);
    }
    const query = params.toString();
    return query ? `${historyUrl}?${query}` : historyUrl;
  }

  /** How a revision is named in a sentence: who and when, or just when for the import. */
  function name(revision: RevisionSummary): string {
    const when = `${formatInstant(revision.created_at)} UTC`;
    return revision.author_is_account ? `${revision.author_name}, ${when}` : when;
  }

  /** Where a structural change happened, counted from one as people count. */
  function position(change: StructureChange): string {
    const from = change.from_index === null ? null : change.from_index + 1;
    const to = change.to_index === null ? null : change.to_index + 1;
    if (from !== null && to !== null) return from === to ? `Position ${to}` : `Position ${from} → ${to}`;
    if (to !== null) return `Position ${to}`;
    if (from !== null) return `war Position ${from}`;
    return '';
  }

  const comparable = $derived(data.from !== null && data.to !== null && data.from.id !== data.to.id);

  let busy = $state(false);
  let restoreError = $state<string | null>(null);

  function refused(status: number): string {
    if (status === 403) return 'Dafür fehlen die Rechte: Wiederherstellen setzt Schreibrecht auf dieser Seite voraus.';
    if (status === 404) return 'Diese Fassung gibt es nicht (mehr).';
    if (status === 401) return 'Nicht angemeldet. Bitte melde dich erneut an.';
    return `Das Wiederherstellen ist fehlgeschlagen (Fehler ${status}).`;
  }

  /**
   * Confirm, post, navigate — in that order, and the confirmation is the URL rather than
   * component state so that the question itself is part of the page.
   *
   * A `fetch` and not a `<form method="post">` because the endpoint answers JSON, which is
   * the same call `PeoplePanel` documents. The POST carries no body: which revision is being
   * restored is the whole request, and the server takes the write decision.
   */
  async function restore(revision: RevisionSummary) {
    busy = true;
    restoreError = null;
    let response: Response;
    try {
      response = await fetch(`/api/revisions/${revision.id}/restore`, { method: 'POST' });
    } catch {
      restoreError = 'Die Anwendung antwortet nicht. Es wurde nichts geändert.';
      busy = false;
      return;
    }
    if (!response.ok) {
      restoreError = refused(response.status);
      busy = false;
      return;
    }
    // To the page itself, which is what was restored. The history is one click away again
    // and now carries the new fassung at the top.
    await goto(data.doc.path);
  }
</script>

<svelte:head><title>Verlauf: {data.doc.title} — great-wiki</title></svelte:head>

<main id="content" class="page">
  <p class="back"><a href={data.doc.path}>← Zurück zu »{data.doc.title}«</a></p>
  <h1>Verlauf</h1>
  <p class="lede">
    Jede veröffentlichte Fassung von »{data.doc.title}«, die neueste zuerst. Nichts wird
    überschrieben: Wiederherstellen veröffentlicht eine alte Fassung als <em>neue</em>, und der
    aktuelle Stand bleibt daneben stehen.
  </p>

  {#if data.confirming}
    <section class="confirm" aria-labelledby="confirm-title">
      <h2 id="confirm-title">Diese Fassung wiederherstellen?</h2>
      <p>
        {#if data.confirming.author_is_account}
          Die Fassung von {data.confirming.author_name} vom {formatInstant(
            data.confirming.created_at
          )} UTC
        {:else}
          Die Fassung vom {formatInstant(data.confirming.created_at)} UTC ({data.confirming
            .author_name})
        {/if}
        wird als neue Fassung veröffentlicht. Der aktuelle Stand bleibt als eigene Fassung im
        Verlauf erhalten — es wird nichts gelöscht, und das Wiederherstellen lässt sich selbst
        wieder rückgängig machen, indem du die andere Fassung wiederherstellst.
      </p>
      {#if restoreError}
        <p class="notice notice--error" role="alert">{restoreError}</p>
      {/if}
      <p class="actions">
        <button
          type="button"
          class="btn btn--danger"
          disabled={busy}
          onclick={() => data.confirming && restore(data.confirming)}
        >
          {busy ? 'Wird wiederhergestellt …' : 'Wiederherstellen'}
        </button>
        <a class="btn" href={href({ wiederherstellen: null })}>Abbrechen</a>
      </p>
    </section>
  {/if}

  {#if data.error}
    <p class="notice notice--error" role="alert">{data.error}</p>
  {:else if data.revisions.length === 0}
    <p class="notice">
      Diese Seite hat noch keine Fassungen — sobald jemand sie bearbeitet und veröffentlicht,
      steht hier, was sich geändert hat.
    </p>
  {:else}
    <!-- A GET form: the selection lands in the address bar, so a comparison is a link
         somebody can send and the back button walks through what was looked at. -->
    <form method="get" action={historyUrl} class="timeline">
      <input type="hidden" name="ansicht" value={data.view} />
      <table>
        <caption>
          Wähle eine ältere und eine neuere Fassung und vergleiche sie. Zeiten in UTC.
        </caption>
        <thead>
          <tr>
            <th scope="col">Von</th>
            <th scope="col">Bis</th>
            <th scope="col">Beschreibung</th>
            <th scope="col">Wer</th>
            <th scope="col">Wann</th>
            <th scope="col">Größe</th>
            <th scope="col">Aktionen</th>
          </tr>
        </thead>
        <tbody>
          {#each data.revisions as revision (revision.id)}
            <tr class:chosen={revision.id === data.from?.id || revision.id === data.to?.id}>
              <td>
                <input
                  type="radio"
                  name="von"
                  value={revision.id}
                  checked={revision.id === data.from?.id}
                  aria-label={`Ältere Fassung: ${name(revision)}`}
                />
              </td>
              <td>
                <input
                  type="radio"
                  name="bis"
                  value={revision.id}
                  checked={revision.id === data.to?.id}
                  aria-label={`Neuere Fassung: ${name(revision)}`}
                />
              </td>
              <td>
                {#if revision.summary}
                  {revision.summary}
                {:else}
                  <span class="muted">ohne Beschreibung</span>
                {/if}
              </td>
              <td>
                {revision.author_name}
                {#if !revision.author_is_account}
                  <span class="muted">— kein Konto</span>
                {/if}
              </td>
              <td>
                <time datetime={revision.created_at}>{relativeTime(revision.created_at, data.now)}</time>
                <span class="muted exact">{formatInstant(revision.created_at)}</span>
              </td>
              <td class="size">
                {formatDelta(sizeDelta(revision, data.revisions))}
                <span class="muted exact">{revision.byte_size} B</span>
              </td>
              <td class="actions">
                <a href={href({ ansicht: 'quelle', bis: revision.id })}>Quelltext</a>
                {#if mayRestore}
                  <a href={href({ wiederherstellen: revision.id })}>Wiederherstellen</a>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="compare"><button type="submit" class="btn">Vergleichen</button></p>
    </form>

    <section class="diff" aria-labelledby="diff-title">
      <h2 id="diff-title">
        {#if comparable && data.from && data.to}
          Vergleich: {name(data.from)} → {name(data.to)}
        {:else}
          Vergleich
        {/if}
      </h2>

      <!-- Real links, not buttons: each view is a state of the page with its own address,
           it works before hydration, and it can be opened in a second tab. -->
      <nav class="tabs" aria-label="Ansicht">
        {#each VIEWS as view (view)}
          <a
            href={href({ ansicht: view })}
            class:current={data.view === view}
            aria-current={data.view === view ? 'page' : undefined}>{VIEW_LABEL[view]}</a
          >
        {/each}
      </nav>
      <p class="hint">{VIEW_HINT[data.view as View]}</p>

      {#if data.view === 'quelle'}
        {#if data.sourceError}
          <p class="notice notice--error" role="alert">{data.sourceError}</p>
        {:else if data.source}
          <p class="hint">Fassung: {name(data.source.revision)}</p>
          {#if data.source.problem}
            <p class="notice notice--warn">
              Diese Fassung lässt sich nicht verlustfrei als Markdown schreiben:
              {data.source.problem}
            </p>
          {/if}
          {#if data.source.markdown !== null}
            <h3 class="file">{data.doc.slug}.md</h3>
            <pre>{data.source.markdown}</pre>
          {/if}
          {#if data.source.meta}
            <h3 class="file">{data.doc.slug}.meta.yml</h3>
            <p class="hint">
              Metadaten gehören zur Seite, nicht zur Fassung: sie beschreiben den heutigen
              Stand.
            </p>
            <pre>{data.source.meta}</pre>
          {/if}
          <h3 class="file">{data.doc.slug}.design.json</h3>
          <pre>{data.source.design}</pre>
        {/if}
      {:else if !comparable}
        <p class="notice">
          Wähle oben zwei verschiedene Fassungen und drücke »Vergleichen«.
        </p>
      {:else if data.diffError}
        <p class="notice notice--error" role="alert">{data.diffError}</p>
      {:else if data.diff}
        {#if data.view === 'prosa'}
          {#if data.diff.prose.length === 0}
            <p class="notice">Keine Änderungen am Text. Sieh unter »Struktur« und »Design« nach.</p>
          {:else}
            <ul class="changes">
              {#each data.diff.prose as change, index (index)}
                <li class={`change change--${change.kind}`}>
                  <span class="mark" aria-hidden="true">{CHANGE_MARK[change.kind]}</span>
                  <span class="label">{CHANGE_LABEL[change.kind]}:</span>
                  <span class="text">{change.text}</span>
                </li>
              {/each}
            </ul>
          {/if}
        {:else if data.view === 'struktur'}
          {#if data.diff.structure.length === 0}
            <p class="notice">Keine Änderungen am Aufbau der Seite.</p>
          {:else}
            <ul class="changes">
              {#each data.diff.structure as change, index (index)}
                <li class={`change change--${change.kind}`}>
                  <span class="mark" aria-hidden="true">{CHANGE_MARK[change.kind]}</span>
                  <span class="label">{CHANGE_LABEL[change.kind]}:</span>
                  <span class="what">{blockLabel(change.block)}</span>
                  <span class="muted">{position(change)}</span>
                  {#if change.text}<span class="text">»{change.text}«</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        {:else if data.diff.design.length === 0}
          <p class="notice">Keine Änderungen am Aussehen.</p>
        {:else}
          <table class="design">
            <thead>
              <tr>
                <th scope="col">Block</th>
                <th scope="col">Eigenschaft</th>
                <th scope="col">Vorher</th>
                <th scope="col">Nachher</th>
              </tr>
            </thead>
            <tbody>
              {#each data.diff.design as change, index (index)}
                <tr>
                  <td>
                    {blockLabel(change.block)}
                    {#if change.text}<span class="muted">»{change.text}«</span>{/if}
                  </td>
                  <td>{attributeLabel(change.attribute)}</td>
                  <td class="before">
                    <span class="mark" aria-hidden="true">{CHANGE_MARK.removed}</span>
                    {change.before ?? '— nicht gesetzt'}
                  </td>
                  <td class="after">
                    <span class="mark" aria-hidden="true">{CHANGE_MARK.added}</span>
                    {change.after ?? '— nicht gesetzt'}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}
    </section>
  {/if}
</main>

<style>
  .page {
    max-width: 68rem;
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

  h2 {
    font-size: var(--text-xl);
    line-height: var(--leading-tight);
  }

  .back a {
    color: var(--accent);
    font-size: var(--text-sm);
    text-decoration: none;
  }

  .back a:hover,
  .back a:focus-visible {
    text-decoration: underline;
  }

  .lede,
  .hint {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  .muted {
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }

  .exact {
    display: block;
  }

  /* --- The timeline ------------------------------------------------------------------ */

  table {
    inline-size: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  caption {
    caption-side: top;
    text-align: start;
    color: var(--ink-muted);
    font-size: var(--text-sm);
    padding-block-end: var(--space-2);
  }

  th,
  td {
    text-align: start;
    padding: var(--space-2) var(--space-3);
    border-block-end: 1px solid var(--border);
    vertical-align: top;
  }

  th {
    color: var(--ink-muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 650;
  }

  tr.chosen {
    background: var(--accent-soft);
  }

  td.actions a {
    color: var(--accent);
    margin-inline-end: var(--space-3);
    white-space: nowrap;
  }

  .size {
    font-variant-numeric: tabular-nums;
  }

  .compare {
    margin-block-start: var(--space-3);
  }

  .btn {
    display: inline-block;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--accent);
    font: inherit;
    font-size: var(--text-sm);
    text-decoration: none;
    cursor: pointer;
  }

  .btn:hover,
  .btn:focus-visible {
    background: var(--accent-soft);
  }

  .btn--danger {
    color: var(--danger);
    border-color: var(--danger);
  }

  .btn[disabled] {
    opacity: 0.6;
    cursor: progress;
  }

  /* --- Confirming a restore ----------------------------------------------------------- */

  .confirm {
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-6);
    background: var(--bg-raised);
  }

  .confirm > * + * {
    margin-block-start: var(--space-3);
  }

  .confirm p {
    max-width: var(--measure);
    font-size: var(--text-sm);
  }

  .actions {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
  }

  /* --- The four views ----------------------------------------------------------------- */

  .tabs {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
    border-block-end: 1px solid var(--border);
  }

  .tabs a {
    padding: var(--space-2) var(--space-3);
    color: var(--ink-muted);
    text-decoration: none;
    border-block-end: 2px solid transparent;
    margin-block-end: -1px;
    font-size: var(--text-sm);
  }

  .tabs a:hover,
  .tabs a:focus-visible {
    color: var(--ink);
  }

  .tabs a.current {
    color: var(--accent);
    border-block-end-color: var(--accent);
    font-weight: 650;
  }

  .diff > * + * {
    margin-block-start: var(--space-3);
  }

  .changes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-1);
  }

  /* Every change carries a symbol AND a word as well as a background colour. That is the
     requirement, not a flourish: additions and removals told apart by colour alone are
     indistinguishable to a reader who cannot see the difference, on a printed page, and in
     a black-and-white screenshot. The colours below are the redundant channel. */
  .change {
    display: grid;
    grid-template-columns: 1.25rem auto 1fr;
    gap: var(--space-2);
    align-items: baseline;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    border-inline-start: 3px solid var(--border-strong);
    background: var(--bg-sunken);
    font-size: var(--text-sm);
  }

  .change .mark {
    font-family: var(--font-mono);
    font-weight: 700;
    text-align: center;
  }

  .change .label {
    color: var(--ink-muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .change--added {
    border-inline-start-color: var(--ok);
  }

  .change--added .mark {
    color: var(--ok);
  }

  .change--removed {
    border-inline-start-color: var(--danger);
  }

  .change--removed .mark {
    color: var(--danger);
  }

  .change--removed .text {
    text-decoration: line-through;
  }

  .change--moved {
    border-inline-start-color: var(--accent);
  }

  .change--moved .mark {
    color: var(--accent);
  }

  .change--changed {
    border-inline-start-color: var(--warn);
  }

  .change--changed .mark {
    color: var(--warn);
  }

  .change .what {
    font-weight: 650;
  }

  .design .before .mark {
    color: var(--danger);
  }

  .design .after .mark {
    color: var(--ok);
  }

  /* --- Source ------------------------------------------------------------------------- */

  .file {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--ink-muted);
  }

  pre {
    background: var(--bg-sunken);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-3);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* --- Notices ------------------------------------------------------------------------ */

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

  .notice--warn {
    border-inline-start-color: var(--warn);
    color: var(--ink);
  }

  @media (max-width: 48rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }

    /* The table scrolls inside itself rather than pushing the page sideways. */
    .timeline {
      overflow-x: auto;
    }

    table {
      min-inline-size: 40rem;
    }
  }
</style>
