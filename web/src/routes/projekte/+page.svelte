<!--
  Projekte (D-13): the list of projects, and the one place you would go to ask what projects
  exist.

  The owner chose this over a "make this page a project" control on the page itself — cheaper
  to build, but it buries exactly that question — and over the admin console, because a
  project is ordinary content and requiring an administrator to create one makes projects
  rare, which defeats them.

  **Everything here is already filtered, and nothing here filters again.** `/api/projects`
  answers only the projects whose home page the caller may read, per document, and that is the
  same check that decides whether they may read the board — so the two cannot disagree. The
  one thing this file could add that the filter cannot take back is a NUMBER: a total, or an
  "N ausgeblendet", would be a fact about pages the reader may not read. There is none, and
  `page.test.ts` asserts its absence rather than trusting this comment.

  **Nothing on this page needs a script.** The list is links, the form is a form action, and
  the question before a deletion is a URL. That is not a preference: a control that only works
  once JavaScript arrives is a control that looks live and does nothing, which is what
  `[...path]/+page.svelte` records about its own edit link.
-->
<script lang="ts">
  // `formatInstant` comes from the admin client rather than being spelled again: it is a pure
  // formatter that deliberately avoids `Intl`, because two ICU builds and two time zones
  // between the server render and hydration is a mismatch nobody would think to suspect.
  import { formatInstant } from '$lib/adminApi';
  import type { Project } from '$lib/projects';

  let { data, form } = $props();

  /**
   * Whether to offer creating and deleting at all, and why the answer is this crude.
   *
   * The same reasoning `[...path]/+page.svelte` gives for the edit control and
   * `[...path]/history/+page.svelte` gives for restoring, and the same one bit is missing:
   * nothing on the wire says "may I write this page". Both acts need Write on a page,
   * `/api/me` reports groups and a baseline, and D-M2-8 is explicit that no baseline confers
   * write — so the only thing that knows is the endpoint itself. The control is therefore
   * offered to whoever is signed in and the true answer is given the moment it is used: a
   * refusal comes back as a sentence, never as a silent nothing. The offer can be false; the
   * creation cannot.
   */
  const mayOffer = $derived(data.me?.authenticated === true);

  /** The refusal that belongs to the create form, as against the one that belongs to a delete. */
  const createError = $derived(form?.wo === 'anlegen' ? form.fehler : null);
  const deleteError = $derived(form?.wo === 'loeschen' ? form.fehler : null);

  /** What was typed, so a refused form does not also make somebody type it again. */
  const typed = $derived(form?.wo === 'anlegen' ? form.startseite : '');

  function confirmHref(project: Project): string {
    return `/projekte?loeschen=${encodeURIComponent(project.id)}`;
  }
</script>

<svelte:head><title>Projekte — great-wiki</title></svelte:head>

<main id="content" class="page">
  <h1>Projekte</h1>
  <p class="lede">
    Ein Projekt gehört zu genau einer Seite — seiner Startseite. Von dort sammelt es die
    Aufgaben, die auf ihr und auf den Seiten darunter geschrieben stehen. Es erscheint nur,
    was Sie auch lesen dürfen.
  </p>

  {#if data.created}
    <!-- Confirmed against the list the loader just read, not against the address bar: a
         hand-typed »?angelegt=« names a project only if it is genuinely there. -->
    <p class="notice notice--ok" role="status">
      »{data.created.home_title}« ist jetzt die Startseite eines Projekts.
    </p>
  {/if}

  <!-- Above the list, like the subtree filter on /graph: after a refused submission this is
       where the reader is looking and where the message has to be. The list below is still
       what the page is for. -->
  <section class="neu" aria-labelledby="neu-titel">
    <h2 id="neu-titel">Neues Projekt</h2>
    {#if mayOffer}
      <!-- A real POST to a named form action. No `use:enhance`: without it the browser
           submits, the server answers 303 to the list, and the whole thing works with
           JavaScript switched off — which is the requirement, not a fallback. -->
      <form method="post" action="?/anlegen">
        <label for="startseite">Startseite</label>
        <p id="startseite-hinweis" class="hint">
          Der Pfad einer Seite, die es schon gibt und die Sie bearbeiten dürfen — zum
          Beispiel <code>/rundgang/tabellen</code>. Eine ganze Adresse aus der Adresszeile
          geht auch.
        </p>
        <div class="row">
          <input
            id="startseite"
            name="startseite"
            type="text"
            required
            autocomplete="off"
            spellcheck="false"
            placeholder="/rundgang/tabellen"
            value={typed ?? ''}
            aria-invalid={createError ? 'true' : undefined}
            aria-describedby={createError ? 'startseite-hinweis startseite-fehler' : 'startseite-hinweis'}
          />
          <button type="submit" class="btn">Projekt anlegen</button>
        </div>
      </form>
    {:else}
      <p class="notice">
        Neue Projekte legt an, wer angemeldet ist und die gewählte Startseite bearbeiten darf.
      </p>
    {/if}

    <!-- OUTSIDE the offer, deliberately, and found the hard way: submitting this form
         against an API that was not answering renders `me` as nobody, which withdraws the
         form — and with it, when this lived inside, the sentence explaining what had just
         happened. The person got a page that said nothing about the button they had pressed.
         A session expiring between the render and the submit is the same shape and is not
         hypothetical. `aria-describedby` is by id and does not care that the paragraph sits
         beside the form rather than inside it.

         In words and announced, never a red border alone: the colour is the redundant
         channel, and the sentence carries the way out. -->
    {#if createError}
      <p id="startseite-fehler" class="notice notice--error" role="alert">{createError}</p>
    {/if}
  </section>

  {#if deleteError}
    <p class="notice notice--error" role="alert">{deleteError}</p>
  {/if}

  {#if data.confirming}
    <section class="confirm" aria-labelledby="confirm-titel">
      <h2 id="confirm-titel">Dieses Projekt löschen?</h2>
      <p>
        Das Projekt auf »{data.confirming.home_title}« ({data.confirming.home_path}) wird
        entfernt. Die Karten, die auf seiner Tafel angelegt wurden, gehen mit — Aufgaben, die
        als Zeile in einer Seite stehen, bleiben stehen, und die Seiten bleiben unverändert.
        Gelöscht wird also das Projekt, nicht sein Inhalt.
      </p>
      <form method="post" action="?/loeschen" class="actions">
        <input type="hidden" name="id" value={data.confirming.id} />
        <button type="submit" class="btn btn--danger">Löschen</button>
        <a class="btn" href="/projekte">Abbrechen</a>
      </form>
    </section>
  {/if}

  {#if data.error}
    <p class="notice notice--error" role="alert">{data.error}</p>
  {:else if data.projects.length === 0}
    <!-- One message for "no project has been made yet" and for "none of them are yours".
         The conflation is deliberate and is the same one `/graph`'s empty state makes: saying
         which of the two it is would say that something is being withheld, which is the whole
         of what the filtering was hiding. -->
    <p class="empty">
      Hier ist kein Projekt zu sehen. Ein Projekt entsteht, indem eine vorhandene Seite zu
      seiner Startseite wird — dafür ist das Formular oben da.
    </p>
  {:else}
    <table>
      <caption>Nach Pfad sortiert. Zeiten in UTC.</caption>
      <thead>
        <tr>
          <th scope="col">Startseite</th>
          <th scope="col">Etikett</th>
          <th scope="col">Angelegt</th>
          {#if mayOffer}<th scope="col">Aktionen</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each data.projects as project (project.id)}
          <tr data-projekt={project.id}>
            <td>
              <a href={project.home_path}>{project.home_title}</a>
              <span class="muted pfad">{project.home_path}</span>
            </td>
            <td>
              {#if project.tag_id}
                <code>{project.tag_id}</code>
              {:else}
                <!-- Written out rather than left blank. An empty cell reads as "not loaded"
                     just as easily as "none", and a dash reads as neither. -->
                <span class="muted">kein Etikett</span>
              {/if}
            </td>
            <td class="wann">{formatInstant(project.created_at)}</td>
            {#if mayOffer}
              <td class="actions">
                <!-- The name says which project, because "Löschen" repeated once per row is
                     the same link four times to anybody reading the links on their own. -->
                <a
                  href={confirmHref(project)}
                  aria-label={`Projekt auf »${project.home_title}« löschen`}>Löschen</a
                >
              </td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</main>

<style>
  /* The list is a table, and a table wants the room. The lede, the hint and every notice
     below stay capped at the measure, because those are sentences. */
  .page {
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

  .pfad {
    display: block;
    font-family: var(--font-mono);
  }

  code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  /* --- The form ----------------------------------------------------------------------- */

  .neu {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-6);
    background: var(--bg-raised);
  }

  .neu > * + * {
    margin-block-start: var(--space-3);
  }

  form > * + * {
    margin-block-start: var(--space-2);
  }

  label {
    display: block;
    font-size: var(--text-sm);
    font-weight: 650;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }

  input {
    flex: 1 1 22rem;
    min-inline-size: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink);
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  /* The colour is the second channel; the sentence below the field is the first. */
  input[aria-invalid='true'] {
    border-color: var(--danger);
  }

  .btn {
    display: inline-block;
    padding: var(--space-2) var(--space-3);
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

  /* --- Confirming a deletion ----------------------------------------------------------- */

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

  /* --- The list ------------------------------------------------------------------------ */

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

  .wann {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  td.actions a {
    color: var(--accent);
    white-space: nowrap;
  }

  /* --- Notices -------------------------------------------------------------------------- */

  .empty,
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

  @media (max-width: 48rem) {
    .page {
      padding: var(--space-6) var(--space-4);
    }

    /* The table scrolls inside itself rather than pushing the page sideways. */
    table {
      display: block;
      overflow-x: auto;
      white-space: normal;
    }
  }
</style>
