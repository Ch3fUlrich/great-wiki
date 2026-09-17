<!--
  Where a live view of another page is written: a page of this wiki, and optionally one
  section of it (D-27).

  # Two steps, because they are two questions

  The page first, from the **same picker the link dialog uses** — `matchingPages` over
  `GET /api/tree`, which is `Store::tree_for`, filtered per document through the same `can()`
  a page read goes through, with a refused branch skipped whole. There is no second listing of
  "which pages are there" to filter differently, and **nothing here says how many were left
  out**: a count of what was hidden is the same disclosure with the name filed off.

  Then the section, which needs the chosen page's headings and their **stable anchors** — the
  ids the store minted onto them on publish, which live in that page's body and nowhere else.
  They are fetched from `/_abschnitte/{pfad}`, which is a proxy for
  `GET /api/documents/{path}` with this caller's own cookie: it decides nothing, and a page
  this caller may not read is not in the picker to be chosen in the first place.

  **A page whose headings have no anchors offers no sections**, and says so rather than
  offering a choice that cannot be stored. That is a page not published since stable ids
  existed; publishing it once mints them.

  # Why a native `<dialog>`

  `LinkDialog`'s reasoning, verbatim: `showModal()` supplies the focus trap, Escape, the
  backdrop and inertness for the rest of the page, and this dialog is opened by a toolbar
  button that is not an Ark trigger.
-->
<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import { matchingPages, type PageChoice } from './pagePicker';

  /** One heading of the chosen page, as `/_abschnitte` answers it. */
  interface SectionChoice {
    text: string;
    level: number;
    anchor: string;
  }

  interface Props {
    /** Open. Bound, so Escape and the buttons below close it the same way. */
    offen: boolean;
    /**
     * The pages this caller may read, as the shell already asked for them. Handed down
     * rather than fetched: one filtered listing, one authorisation behind it.
     */
    seiten: TreeNode[];
    /** A page — and possibly one of its sections — was chosen. */
    onEinbetten: (wahl: { seite: PageChoice; abschnitt: SectionChoice | null }) => void;
  }

  let { offen = $bindable(), seiten, onEinbetten }: Props = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  let suche = $state('');
  let gewaehlt = $state<PageChoice | null>(null);
  let abschnitte = $state<SectionChoice[]>([]);
  let laedt = $state(false);
  let hinweis = $state<string | null>(null);

  const treffer = $derived(matchingPages(seiten, suche));

  function zuruecksetzen() {
    suche = '';
    gewaehlt = null;
    abschnitte = [];
    laedt = false;
    hinweis = null;
  }

  $effect(() => {
    const el = dialog;
    if (!el) return;
    if (offen && !el.open) {
      zuruecksetzen();
      el.showModal();
    } else if (!offen && el.open) {
      el.close();
    }
  });

  function schliessen() {
    offen = false;
  }

  /**
   * Read the chosen page's headings.
   *
   * A failure is STATED rather than rendered as "this page has no sections": those are
   * different things, and the second is a lie the rest of this interface refuses to tell.
   * Whole-page embedding still works either way, which is why a failure never blocks the
   * dialog.
   */
  async function seiteWaehlen(seite: PageChoice) {
    gewaehlt = seite;
    abschnitte = [];
    hinweis = null;
    laedt = true;
    try {
      const antwort = await fetch(`/_abschnitte${seite.path}`);
      if (!antwort.ok) throw new Error(String(antwort.status));
      const gelesen = (await antwort.json()) as { headings?: SectionChoice[] };
      abschnitte = gelesen.headings ?? [];
      if (abschnitte.length === 0) {
        hinweis =
          'Diese Seite bietet keine einbettbaren Abschnitte an. Sie kann als Ganzes ' +
          'eingebettet werden.';
      }
    } catch {
      hinweis =
        'Die Abschnitte dieser Seite konnten nicht gelesen werden. Die Seite kann trotzdem ' +
        'als Ganzes eingebettet werden.';
    } finally {
      laedt = false;
    }
  }

  function einbetten(abschnitt: SectionChoice | null) {
    if (!gewaehlt) return;
    onEinbetten({ seite: gewaehlt, abschnitt });
    schliessen();
  }
</script>

<dialog
  bind:this={dialog}
  class="gw-embeddialog"
  aria-labelledby="gw-embeddialog-titel"
  onclose={() => (offen = false)}
>
  <h2 id="gw-embeddialog-titel">Seite einbetten</h2>

  {#if gewaehlt === null}
    <div class="gw-embeddialog-abschnitt">
      <label for="gw-embeddialog-suche">Seite in diesem Wiki</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="gw-embeddialog-suche"
        type="search"
        autocomplete="off"
        autofocus
        placeholder="Titel oder Adresse suchen"
        bind:value={suche}
      />
      {#if treffer.length === 0}
        <p class="gw-embeddialog-leer">Keine Seite gefunden.</p>
      {:else}
        <ul class="gw-embeddialog-treffer">
          {#each treffer as seite (seite.id)}
            <li>
              <button type="button" data-einbettziel={seite.id} onclick={() => seiteWaehlen(seite)}>
                <span class="gw-embeddialog-titel">{seite.title}</span>
                <span class="gw-embeddialog-pfad">{seite.path}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <p class="gw-embeddialog-hinweis">
        Eingebettet wird der aktuelle Stand der gewählten Seite. Wer sie nicht lesen darf,
        sieht hier nur Ihre Beschriftung.
      </p>
    </div>
  {:else}
    <div class="gw-embeddialog-abschnitt">
      <p class="gw-embeddialog-gewaehlt">
        <strong>{gewaehlt.title}</strong>
        <span class="gw-embeddialog-pfad">{gewaehlt.path}</span>
      </p>
      <button type="button" class="gw-embeddialog-ganz" onclick={() => einbetten(null)}>
        Ganze Seite einbetten
      </button>
      {#if laedt}
        <p class="gw-embeddialog-leer">Abschnitte werden gelesen …</p>
      {:else if hinweis}
        <p class="gw-embeddialog-leer">{hinweis}</p>
      {:else}
        <p id="gw-embeddialog-abschnitte-label" class="gw-embeddialog-hinweis">
          … oder nur diesen Abschnitt:
        </p>
        <ul class="gw-embeddialog-treffer" aria-labelledby="gw-embeddialog-abschnitte-label">
          {#each abschnitte as abschnitt (abschnitt.anchor)}
            <li>
              <button
                type="button"
                data-abschnitt={abschnitt.anchor}
                onclick={() => einbetten(abschnitt)}
              >
                <span class="gw-embeddialog-titel">{abschnitt.text}</span>
                <span class="gw-embeddialog-pfad">Ebene {abschnitt.level}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <button type="button" onclick={() => zuruecksetzen()}>Andere Seite wählen</button>
    </div>
  {/if}

  <div class="gw-embeddialog-fuss">
    <button type="button" onclick={schliessen}>Abbrechen</button>
  </div>
</dialog>

<style>
  /* Tokens throughout, like `LinkDialog`: a theme has to be able to repaint this, and
     `<dialog>`'s user-agent styling is a white box with a black border. */
  .gw-embeddialog {
    max-width: min(36rem, calc(100vw - 2 * var(--space-4)));
    width: 100%;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
  }

  .gw-embeddialog::backdrop {
    background: rgb(0 0 0 / 40%);
  }

  .gw-embeddialog h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--font-size-lg);
  }

  .gw-embeddialog-abschnitt {
    display: grid;
    gap: var(--space-2);
  }

  .gw-embeddialog-abschnitt label {
    font-weight: 600;
  }

  .gw-embeddialog-abschnitt input {
    width: 100%;
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: inherit;
  }

  .gw-embeddialog-treffer {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-1);
    max-height: 14rem;
    overflow-y: auto;
  }

  .gw-embeddialog-treffer button {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: baseline;
    width: 100%;
    text-align: start;
    padding: var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    cursor: pointer;
  }

  .gw-embeddialog-treffer button:hover,
  .gw-embeddialog-treffer button:focus-visible {
    border-color: var(--color-border);
    background: var(--color-surface-2, var(--color-bg));
  }

  .gw-embeddialog-gewaehlt {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: baseline;
  }

  .gw-embeddialog-pfad,
  .gw-embeddialog-hinweis,
  .gw-embeddialog-leer {
    color: var(--color-text-muted);
    font-size: var(--font-size-sm);
  }

  .gw-embeddialog-hinweis,
  .gw-embeddialog-leer {
    margin: 0;
  }

  .gw-embeddialog-fuss {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    justify-content: flex-end;
    margin-top: var(--space-4);
  }

  .gw-embeddialog-fuss button,
  .gw-embeddialog-abschnitt button:not(.gw-embeddialog-treffer button) {
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: inherit;
    cursor: pointer;
    justify-self: start;
  }
</style>
