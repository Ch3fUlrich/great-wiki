<script lang="ts">
  /**
   * Rename a page, move it, or both — the owner's "dialog first" (roadmap 2026-09-24).
   *
   * **Two plain forms and no script.** The first is a GET back to this page: its fields land
   * in the address, the loader asks the API to measure the move, and the answer is in the next
   * first response. The second, drawn only under a measured move, POSTs the very fields that
   * were measured. Every control is native, so it works with a keyboard and with JavaScript
   * switched off; dragging in the sidebar (`dropHref`) opens this same dialog, prefilled and measured.
   *
   * **Nothing here decides anything.** Who gains and who loses reading access, and whether
   * this caller may make the move, are the API's answers (`MovePlan`), measured by carrying the
   * move out and rolling it back. The picker is the tree this reader was already given, which
   * is filtered per document.
   */
  import type { TreeNode } from '$lib/api';
  import {
    destinations,
    pagesText,
    parentOf,
    readerText,
    MOVE_PARAM,
    MOVE_REGION_ID,
    PARENT_FIELD,
    SLUG_FIELD,
    TITLE_FIELD,
    type MoveFields,
    type MovePlan
  } from '$lib/moves';

  let {
    path,
    title,
    slug,
    tree,
    felder = null,
    vorschau = null,
    vorschauFehler = null,
    abbrechen
  }: {
    path: string;
    title: string;
    slug: string;
    tree: TreeNode[];
    /** What the GET form last submitted; `null` when the dialog has just been opened. */
    felder?: MoveFields | null;
    vorschau?: MovePlan | null;
    vorschauFehler?: string | null;
    /** Where "Abbrechen" goes: this page, without the question. */
    abbrechen: string;
  } = $props();

  const werte = $derived(felder ?? { ziel: parentOf(path), titel: title, adresse: slug });
  const ziele = $derived(destinations(tree, path));
  // The GET form's own address carries the fragment, so the submitted dialog is focused — and
  // therefore announced — on arrival, exactly as the link that opened it was.
  const selbst = $derived(`${path}#${MOVE_REGION_ID}`);
</script>

<section
  class="verschieben no-print"
  id={MOVE_REGION_ID}
  tabindex="-1"
  aria-labelledby="verschieben-titel"
>
  <h2 id="verschieben-titel">Umbenennen oder verschieben</h2>
  <p>
    »{title}« zieht <strong>mit allen Seiten darunter</strong> um. Verweise aus anderen Seiten
    bleiben gültig, und die alte Adresse leitet alle, die die Seite lesen dürfen, zur neuen
    weiter. Wer die Seite lesen darf, richtet sich danach nach dem neuen Ort — wer dadurch
    Zugriff gewinnt oder verliert, steht vor dem Bestätigen hier.
  </p>

  <form method="get" action={selbst} class="verschieben-felder">
    <input type="hidden" name={MOVE_PARAM} value="1" />
    <label>
      <span>Titel</span>
      <input name={TITLE_FIELD} value={werte.titel} required autocomplete="off" />
    </label>
    <label>
      <span>Adresse (letzter Teil)</span>
      <input name={SLUG_FIELD} value={werte.adresse} autocomplete="off" />
    </label>
    <label>
      <span>Liegt unter</span>
      <select name={PARENT_FIELD}>
        <option value="" selected={werte.ziel === ''}>Oberste Ebene</option>
        {#each ziele as ziel (ziel.path)}
          <option value={ziel.path} selected={werte.ziel === ziel.path}
            >{'— '.repeat(ziel.depth)}{ziel.title}</option
          >
        {/each}
      </select>
    </label>
    <div class="verschieben-actions">
      <button type="submit" class="btn">Vorschau anzeigen</button>
      <a class="btn" href={abbrechen}>Abbrechen</a>
    </div>
  </form>

  {#if vorschauFehler}
    <p class="notice notice--error" role="alert">{vorschauFehler}</p>
  {/if}

  {#if vorschau}
    <div class="vorschau">
      <h3>Was sich ändert</h3>
      <p>
        Neue Adresse: <code>{vorschau.to}</code>. {pagesText(vorschau.pages)}
        {vorschau.pages === 1 ? 'zieht' : 'ziehen'} um.
      </p>

      <h4>Dürfen danach lesen, was sie jetzt nicht lesen dürfen</h4>
      {#if vorschau.gains.length}
        <ul>
          {#each vorschau.gains as wer (wer.username ?? wer.name)}
            <li>{readerText(wer, vorschau.pages)}</li>
          {/each}
        </ul>
      {:else}
        <p>Niemand.</p>
      {/if}

      <h4>Dürfen danach nicht mehr lesen</h4>
      {#if vorschau.losses.length}
        <ul>
          {#each vorschau.losses as wer (wer.username ?? wer.name)}
            <li>{readerText(wer, vorschau.pages)}</li>
          {/each}
        </ul>
      {:else}
        <p>Niemand.</p>
      {/if}

      {#if vorschau.refusal}
        <!-- The plan is complete and this caller may not carry it out: somebody would gain
             access, and widening needs admin rights on the destination. No button, because a
             button here would be a control that lies. The API's own words ride along. -->
        <p class="notice notice--error" role="alert">
          So kann diese Seite nicht verschoben werden: Dadurch dürften Personen sie lesen, die
          das jetzt nicht dürfen, und das erfordert Verwaltungsrechte am Ziel. ({vorschau.refusal})
        </p>
      {:else}
        <form method="post" action="?/verschieben" class="verschieben-actions">
          <input type="hidden" name={PARENT_FIELD} value={werte.ziel} />
          <input type="hidden" name={TITLE_FIELD} value={werte.titel} />
          <input type="hidden" name={SLUG_FIELD} value={werte.adresse} />
          <button type="submit" class="btn">Jetzt verschieben</button>
        </form>
      {/if}
    </div>
  {/if}
</section>

<style>
  /* The delete question's shape, deliberately: both are a question asked on the page before
     something happens to it, and neither is danger-styled — a move is undone by moving back. */
  .verschieben {
    margin-block-start: var(--space-3);
    max-inline-size: var(--measure);
    margin-inline: auto;
    border: 1px solid var(--border-strong);
    border-inline-start-width: 4px;
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-6);
    background: var(--bg-raised);
  }

  .verschieben:focus {
    outline: none;
  }

  .verschieben > * + *,
  .vorschau > * + * {
    margin-block-start: var(--space-3);
  }

  h2 {
    font-size: var(--text-xl);
    line-height: var(--leading-tight);
  }

  h3 {
    font-size: var(--text-lg);
  }

  h4 {
    font-size: var(--text-base);
  }

  p,
  li {
    font-size: var(--text-sm);
  }

  ul {
    padding-inline-start: var(--space-6);
  }

  .verschieben-felder {
    display: grid;
    gap: var(--space-3);
  }

  label {
    display: grid;
    gap: var(--space-1);
    font-size: var(--text-sm);
  }

  input,
  select {
    font: inherit;
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: inherit;
  }

  .verschieben-actions {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
  }

  .btn {
    display: inline-block;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
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

  .notice {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-inline-start-width: 3px;
    border-inline-start-color: var(--danger);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink);
  }
</style>
