<!--
  Papierkorb (D-14): what has been deleted, what can come back, and the one act in this system
  that loses data.

  **Why there is a place at all.** Deleting happens on the page — beside »Bearbeiten« and
  »Verlauf«, where you are when you decide a page should go. Recovering cannot happen there: a
  deleted page is out of the tree, out of the export and out of the search, so a restore
  control on the page itself would sit behind an address somebody had to have kept. That is
  the owner's split, and it is the same reason `/projekte` exists rather than a control on
  each project's home page.

  **Everything here is already filtered, and nothing here filters again.** `GET /api/trash`
  authorises every entry, and every page inside it, through the same body a page read ends in
  — so a page you could not see before it was deleted is not one you can see here. The one
  thing this file could add that the filtering cannot take back is a NUMBER: a total, or an
  "N ausgeblendet", would be a fact about pages the reader may not read (ADR 0011). There is
  none, and `page.test.ts` asserts its absence rather than trusting this comment.

  **Two controls, two permissions, and the difference is the whole feature.**

  - »Wiederherstellen« is offered on `may_restore` — the store's own verdict, carried on the
    wire (ADR 0010), so the offer and the refusal it would receive cannot come apart. Where it
    is false the control is not drawn *and a sentence takes its place*: a control that is
    silently missing reads as a fault, not as an answer.
  - »Endgültig löschen« has no such bit and cannot have one from here: it is gated by
    `path_admin` on the page's own path, and nothing on the wire says who administers a path.
    So the list offers a **question**, never the act. The act lives behind it, on a
    confirmation the loader can only render once `GET /api/trash/purge/{path}` — gated
    identically to the `POST` — has already answered this caller with a report. The button
    that destroys is therefore never a guess. The question itself is withheld from anybody
    `path_admin` would turn away before it looked at anything else: it demands a signed-in,
    active account, and that half this interface does know.

  **The confirmation names every page.** The report comes from running the purge and rolling
  it back (ADR 0012), so those names are the names that go — including, by that decision's own
  disclosure section, a page fenced off with its own narrower grants that the purge reaches
  anyway. Folding them into "diese Seite und 3 weitere" would throw away the only part of the
  report that says *which*.

  **Nothing here needs a script.** The list is a table, the two acts are form actions, and the
  question before a destruction is a URL. That is not a preference: a control that only works
  once JavaScript arrives is a control that looks live and does nothing, which is what
  `[...path]/+page.svelte` records about its own edit link.
-->
<script lang="ts">
  import {
    confirmPurgeHref,
    formatInstant,
    pagesText,
    purgeLines,
    PURGE_REGION_ID,
    TRASH_PATH,
    TRASH_REGION_ID,
    type TrashEntry
  } from '$lib/trash';

  let { data, form } = $props();

  /**
   * Whether to offer the *question* before a purge — not the purge.
   *
   * `path_admin` refuses outright unless the caller is a signed-in, active account, before it
   * consults a single grant. That is the one half of the gate this interface can evaluate, and
   * offering a destructive question to a reader who is certainly not entitled to it is the
   * kind of lying control this codebase keeps finding. The other half — admin on this
   * particular path — is answered by the API when the question is actually asked, and until
   * then nothing here claims to know it.
   */
  const darfFragen = $derived(data.me?.authenticated === true);

  /** The refusal that belongs to a restore, as against one that belongs to a destruction. */
  const restoreFehler = $derived(form?.wo === 'wiederherstellen' ? form.fehler : null);
  const purgeFehler = $derived(form?.wo === 'endgueltig' ? form.fehler : null);

  /**
   * Whether there is anything to announce at all.
   *
   * The region is rendered only when it has something in it, rather than always and hidden by
   * `:empty`: that selector is unreliable about whitespace and comments — and Svelte's own
   * server rendering puts both inside this element — so the "hidden" version would in practice
   * be an empty block leaving a gap above the list on every ordinary visit. It is exactly the
   * moments something has happened that the redirect's fragment points here, so the anchor
   * exists precisely when anything aims at it.
   */
  const hatMeldung = $derived(
    Boolean(data.geloescht || data.wiederhergestellt || data.geleert || restoreFehler || purgeFehler)
  );

  /**
   * The destroying form keeps `?entfernen=` in its own action.
   *
   * `action="?/name"` replaces the whole query string, so without this a refused purge would
   * come back to a page that no longer knew which entry the reader had been looking at — the
   * report gone, and a bare error where the confirmation had been.
   */
  function purgeAction(entry: TrashEntry): string {
    return `?/endgueltigLoeschen&entfernen=${encodeURIComponent(entry.path)}`;
  }
</script>

<svelte:head><title>Papierkorb — great-wiki</title></svelte:head>

<main id="content" class="page">
  <h1>Papierkorb</h1>
  <p class="lede">
    Eine gelöschte Seite liegt hier — mit allen Seiten, die unter ihr lagen, denn sie ist mit
    ihnen zusammen gelöscht worden. Wiederherstellen holt den ganzen Eintrag zurück, genau so,
    wie er hineingegangen ist. Es erscheint nur, was Sie auch lesen dürfen.
  </p>

  <!-- The region a finished act comes back to. A live region that is already in the document
       announces nothing; this one receives FOCUS from the fragment the redirect carried, and a
       region that has just received focus is read out. No script is involved. -->
  {#if hatMeldung}
    <div id={TRASH_REGION_ID} tabindex="-1" class="meldungen">
      {#if data.geloescht}
        <!-- The page that was just deleted, arriving from the page it was deleted on. Named
             from the ENTRY rather than from the address, so the notice cannot say a page is
             here that the table below does not show — and the number is the row's own. -->
        <p class="notice notice--ok" role="status">
          »{data.geloescht.title}« liegt jetzt im Papierkorb — {pagesText(data.geloescht.pages)}.
          Nichts ist verloren: der Eintrag steht unten und lässt sich wiederherstellen.
        </p>
      {/if}
      {#if data.wiederhergestellt}
        <!-- Checked against the listing the loader just read: a page that is still in the list
             below did not come back, whatever the address bar says. -->
        <p class="notice notice--ok" role="status">
          »{data.wiederhergestellt}« ist wieder da und steht wieder im Wiki.
        </p>
      {/if}
      {#if data.geleert}
        <p class="notice notice--ok" role="status">
          »{data.geleert}« wurde endgültig gelöscht. Diese Seiten und ihre Versionsgeschichte
          sind fort; zurückholen lässt sich davon nichts mehr.
        </p>
      {/if}
      {#if restoreFehler}
        <p class="notice notice--error" role="alert">{restoreFehler}</p>
      {/if}
      {#if purgeFehler}
        <p class="notice notice--error" role="alert">{purgeFehler}</p>
      {/if}
    </div>
  {/if}

  {#if data.entfernen}
    <!-- Above the list, and focused by the link that opened it. The list below is still what
         the page is for, but this is the question that was asked. -->
    <section
      class="confirm"
      id={PURGE_REGION_ID}
      tabindex="-1"
      aria-labelledby="endgueltig-titel"
    >
      <h2 id="endgueltig-titel">»{data.entfernen.title}« endgültig löschen?</h2>

      {#if data.bericht}
        <p class="warnung">
          Das lässt sich <strong>nicht rückgängig machen</strong>. Es gibt keinen zweiten
          Papierkorb: Was hier steht, ist danach fort. Endgültig gelöscht wird genau dies —
        </p>

        <h3>Diese Seiten</h3>
        <!-- Every one of them, by name. The API ran the purge and rolled it back to get this
             list, so these are the pages that go — including one carrying its own narrower
             grants, which ADR 0012 says a purge of an ancestor reaches. Summarising them
             would throw away the only part of the report that says which. -->
        <ul class="seiten">
          {#each data.bericht.pages as seite (seite.path)}
            <li>
              <span class="titel">{seite.title}</span>
              <span class="pfad">{seite.path}</span>
            </li>
          {/each}
        </ul>

        <h3>Und was daran hängt</h3>
        <dl class="folgen">
          {#each purgeLines(data.bericht) as zeile (zeile.was)}
            <div class="zeile">
              <dt>{zeile.was}</dt>
              <dd>{zeile.zahl}</dd>
            </div>
          {/each}
        </dl>

        <p class="hinweis">
          Die Zugriffsrechte auf diesem Pfad bleiben bestehen: sie gelten dem Pfad, nicht der
          Seite. Eine Seite, die später an derselben Stelle entsteht, erbt sie wieder.
        </p>

        <form method="post" action={purgeAction(data.entfernen)} class="actions">
          <input type="hidden" name="pfad" value={data.entfernen.path} />
          <button type="submit" class="btn btn--danger">Endgültig löschen</button>
          <a class="btn" href={TRASH_PATH}>Abbrechen</a>
        </form>
      {:else}
        <!-- No report, so no control that destroys. The gate that would have refused the
             destruction is the same one that refused to describe it, which is exactly why
             this branch can be trusted to be the right one. -->
        <p class="notice notice--error" role="alert">{data.berichtFehler}</p>
        <p class="actions"><a class="btn" href={TRASH_PATH}>Zurück zum Papierkorb</a></p>
      {/if}
    </section>
  {/if}

  {#if data.fehler}
    <p class="notice notice--error" role="alert">{data.fehler}</p>
  {:else if data.entries.length === 0}
    <!-- One message for "nothing has been deleted" and for "none of it is yours". The
         conflation is deliberate and is the same one `/projekte` and `/graph` make: saying
         which of the two it is would say that something is being withheld, which is the whole
         of what the filtering was hiding. -->
    <p class="empty">
      Hier liegt nichts. Gelöschte Seiten kommen hierher — auf jeder Seite steht dafür
      »Löschen« neben »Bearbeiten«.
    </p>
  {:else}
    <table>
      <caption>
        Zuletzt Gelöschtes zuerst. Zeiten in UTC. Der Umfang ist die Zahl der Seiten, die mit
        dem Eintrag gegangen sind und mit ihm zurückkämen.
      </caption>
      <thead>
        <tr>
          <th scope="col">Seite</th>
          <th scope="col">Umfang</th>
          <th scope="col">Gelöscht von</th>
          <th scope="col">Gelöscht am</th>
          <th scope="col">Aktionen</th>
        </tr>
      </thead>
      <tbody>
        {#each data.entries as eintrag (eintrag.path)}
          <tr data-eintrag={eintrag.path}>
            <th scope="row">
              {eintrag.title}
              <!-- The address under the title, because two pages in this wiki can share a
                   title and the path is what tells them apart. It is not a link: the page is
                   in the trash, and a link to it would 404. -->
              <span class="pfad">{eintrag.path}</span>
            </th>
            <td>{pagesText(eintrag.pages)}</td>
            <td>{eintrag.deleted_by_name}</td>
            <td class="wann">{formatInstant(eintrag.deleted_at)}</td>
            <td class="actions">
              {#if eintrag.may_restore}
                <form method="post" action="?/wiederherstellen">
                  <input type="hidden" name="pfad" value={eintrag.path} />
                  <button
                    type="submit"
                    class="btn"
                    aria-label={`»${eintrag.title}« wiederherstellen`}>Wiederherstellen</button
                  >
                </form>
              {:else}
                <!-- Written out rather than left blank. `may_restore` is false when a page in
                     the entry is not this caller's to write — including one they cannot read
                     at all — and an empty cell would read as "not loaded" just as easily as
                     "not yours". -->
                <span class="muted"
                  >Zurückholen darf nur, wer jede Seite in diesem Eintrag bearbeiten darf.</span
                >
              {/if}
              {#if darfFragen}
                <!-- A link to a question, not a control that destroys, and named per row: a
                     row of identical »Endgültig löschen« links is the same link four times to
                     anybody reading the links on their own.

                     `data-sveltekit-reload` is not a preference and not a performance
                     mistake. The fragment is what moves focus to the confirmation, and only
                     a real navigation does that: the browser focuses a fragment target that
                     carries `tabindex="-1"` on arrival, while the client-side router
                     navigates and leaves focus where it was. Found in a browser — the check
                     read `document.activeElement` as the body after a hydrated click and as
                     the region after a full load, on the same markup. Without a script this
                     attribute changes nothing, because there is nothing to opt out of; with
                     one it is the difference between a destructive question being announced
                     and being merely drawn. One page load, on an administrative action. -->
                <a
                  class="gefahr"
                  href={confirmPurgeHref(eintrag.path)}
                  data-sveltekit-reload
                  aria-label={`»${eintrag.title}« endgültig löschen`}>Endgültig löschen …</a
                >
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</main>

<style>
  /* The list is a table, and a table wants the room. Every sentence on the page stays capped
     at the measure. The same split `/projekte` makes. */
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

  h3 {
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ink-muted);
  }

  .lede,
  .hinweis {
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
    color: var(--ink-faint);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: 400;
    overflow-wrap: anywhere;
  }

  /* The focus target for a finished act. It has no outline of its own: the notice inside it
     is what the reader sees, and a box drawn around a message that has just been announced is
     noise. Kept in the flow so it never collapses to nothing when empty. */
  .meldungen:focus {
    outline: none;
  }

  /* --- Confirming a destruction --------------------------------------------------------- */

  .confirm {
    border: 1px solid var(--danger);
    /* Thicker on the leading edge as well, so the section reads as a warning to somebody who
       cannot see the colour at all. */
    border-inline-start-width: 4px;
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-6);
    background: var(--bg-raised);
  }

  .confirm:focus {
    outline: none;
  }

  .confirm > * + * {
    margin-block-start: var(--space-3);
  }

  .warnung {
    max-width: var(--measure);
    font-size: var(--text-sm);
  }

  .seiten {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: var(--space-2) var(--space-6);
  }

  .titel {
    font-size: var(--text-sm);
    font-weight: 650;
  }

  .folgen {
    margin: 0;
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  .folgen .zeile {
    display: flex;
    justify-content: space-between;
    gap: var(--space-4);
    padding-block: var(--space-1);
    border-block-end: 1px solid var(--border);
  }

  .folgen dt {
    color: var(--ink-muted);
  }

  .folgen dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    font-weight: 650;
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
    max-width: var(--measure);
  }

  th,
  td {
    text-align: start;
    padding: var(--space-2) var(--space-3);
    border-block-end: 1px solid var(--border);
    vertical-align: top;
  }

  thead th {
    color: var(--ink-muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 650;
  }

  tbody th {
    font-weight: 650;
  }

  .wann {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  td.actions {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
  }

  /* --- Controls ------------------------------------------------------------------------- */

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

  /* Underlined as well as coloured: the one control on this page that leads towards a loss
     must not depend on hue to be told from the one beside it. */
  .gefahr {
    color: var(--danger);
    text-decoration: underline;
    text-underline-offset: 0.15em;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
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

  .meldungen > * + * {
    margin-block-start: var(--space-3);
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
