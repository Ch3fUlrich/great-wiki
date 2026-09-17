<!--
  Where a link is written: a page of this wiki, or an address.

  # Why there are two halves and not one

  The owner asked for both, and they are not the same act. Picking a page from a list stores
  the page's **identity** (D-5) — a `doc` mark carrying `documents.id` — so renaming or moving
  that page afterwards cannot break the link and the reader always lands where the page is
  now. Typing an address is what you do for the rest of the internet, and for a wiki path
  pasted out of somebody's message; that stores an `href`, and `Store::resolve_references`
  exchanges it for the target's identity when the page is published, if the author may read
  the page it names.

  **Neither half resolves anything here.** The picker already has the id, because the tree
  carries one; the free field hands over exactly what was typed. A client that turned a typed
  path into an id would be a second answer to a permission question, computed where the answer
  cannot be trusted, and it would have to be kept in step with the server's for ever.

  # What the list may show

  `seiten` is `GET /api/tree`, which is `Store::tree_for` — filtered per document through the
  same `can()` a page read goes through, with a refused branch skipped whole. So a page the
  author may not read is not offered, and **nothing here says how many were left out**: a
  count of what was hidden is the same disclosure with the name filed off, which is the rule
  the backlinks panel, the graph and the Papierkorb all already follow.

  # Why a native `<dialog>`

  `showModal()` is what gives the focus trap, Escape, the backdrop and inertness for the rest
  of the page — the five things ADR 0005 chose Ark for, here supplied by the platform. Ark's
  `Dialog` is trigger-driven and this dialog is opened by a `ToggleGroup` item that is not its
  trigger, so wiring it would mean either a second trigger the toolbar does not have or a
  controlled root reaching around the shared wrapper. The behaviour is the requirement; the
  library is not.
-->
<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import { matchingPages, type PageChoice } from './pagePicker';

  interface Props {
    /** Open. Bound, so Escape and the buttons below close it the same way. */
    offen: boolean;
    /**
     * The pages this caller may read, as the shell already asked for them. Handed down
     * rather than fetched: one filtered listing, one authorisation behind it.
     */
    seiten: TreeNode[];
    /** Whether the caret is already inside a link, which is what offers the removal. */
    verknuepft: boolean;
    /** A page was chosen: store its identity. */
    onSeite: (seite: PageChoice) => void;
    /** An address was typed: store it as written, for the server to resolve on publish. */
    onAdresse: (adresse: string) => void;
    /** Take the link off the selection. */
    onEntfernen: () => void;
  }

  let { offen = $bindable(), seiten, verknuepft, onSeite, onAdresse, onEntfernen }: Props =
    $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  let suche = $state('');
  let adresse = $state('');
  let fehler = $state<string | null>(null);

  const treffer = $derived(matchingPages(seiten, suche));

  // `showModal()` rather than the `open` attribute: only the modal form traps focus, makes
  // the rest of the page inert and closes on Escape. Driven from state so the toolbar can
  // open it without holding a reference to the element.
  $effect(() => {
    const el = dialog;
    if (!el) return;
    if (offen && !el.open) {
      suche = '';
      adresse = '';
      fehler = null;
      el.showModal();
    } else if (!offen && el.open) {
      el.close();
    }
  });

  function schliessen() {
    offen = false;
  }

  function seiteWaehlen(seite: PageChoice) {
    onSeite(seite);
    schliessen();
  }

  function adresseSenden(event: SubmitEvent) {
    event.preventDefault();
    const typed = adresse.trim();
    if (typed === '') {
      fehler = 'Bitte eine Adresse eingeben oder eine Seite aus der Liste wählen.';
      return;
    }
    onAdresse(typed);
    schliessen();
  }
</script>

<!-- `close` fires for Escape and for the backdrop as well as for our own buttons, so the
     bound state is corrected from the element rather than only from the code paths that
     happen to remember.

     Nothing puts the caret back by hand: a modal `<dialog>` returns focus to whatever had it
     when `showModal()` ran, which is the toolbar button that opened this. Calling
     `editor.commands.focus()` here instead was worse than redundant — TipTap resolves that
     to a position, and on a document whose only block is an atom (a placed file, say) it
     resolves to one with no inline content and warns about it in every reader's console. -->
<dialog
  bind:this={dialog}
  class="gw-linkdialog"
  aria-labelledby="gw-linkdialog-titel"
  onclose={() => (offen = false)}
>
  <h2 id="gw-linkdialog-titel">Seite verknüpfen</h2>

  <div class="gw-linkdialog-body">
    <div class="gw-linkdialog-abschnitt">
      <label for="gw-linkdialog-suche">Seite in diesem Wiki</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="gw-linkdialog-suche"
        type="search"
        autocomplete="off"
        autofocus
        placeholder="Titel oder Adresse suchen"
        bind:value={suche}
      />
      {#if treffer.length === 0}
        <p class="gw-linkdialog-leer">Keine Seite gefunden.</p>
      {:else}
        <ul class="gw-linkdialog-treffer">
          {#each treffer as seite (seite.id)}
            <li>
              <button type="button" data-verweisziel={seite.id} onclick={() => seiteWaehlen(seite)}>
                <span class="gw-linkdialog-titel">{seite.title}</span>
                <span class="gw-linkdialog-pfad">{seite.path}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <p class="gw-linkdialog-hinweis">
        Eine so gewählte Seite bleibt verknüpft, auch wenn sie später umbenannt oder
        verschoben wird.
      </p>
    </div>

    <form class="gw-linkdialog-abschnitt" onsubmit={adresseSenden}>
      <label for="gw-linkdialog-adresse">Adresse</label>
      <input
        id="gw-linkdialog-adresse"
        type="text"
        autocomplete="off"
        placeholder="https://… , mailto:… oder /darm/labor"
        bind:value={adresse}
      />
      {#if fehler}<p class="gw-linkdialog-fehler" role="alert">{fehler}</p>{/if}
      <button type="submit">Verknüpfen</button>
    </form>
  </div>

  <div class="gw-linkdialog-fuss">
    {#if verknuepft}
      <button
        type="button"
        onclick={() => {
          onEntfernen();
          schliessen();
        }}>Verknüpfung entfernen</button
      >
    {/if}
    <button type="button" onclick={schliessen}>Abbrechen</button>
  </div>
</dialog>

<style>
  /* Tokens throughout, like every other component here: a theme has to be able to repaint
     this, and `<dialog>`'s user-agent styling is a white box with a black border. */
  .gw-linkdialog {
    max-width: min(36rem, calc(100vw - 2 * var(--space-4)));
    width: 100%;
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
    color: var(--color-text);
  }

  .gw-linkdialog::backdrop {
    background: rgb(0 0 0 / 40%);
  }

  .gw-linkdialog h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--font-size-lg);
  }

  .gw-linkdialog-body {
    display: grid;
    gap: var(--space-4);
  }

  .gw-linkdialog-abschnitt {
    display: grid;
    gap: var(--space-2);
  }

  .gw-linkdialog-abschnitt label {
    font-weight: 600;
  }

  .gw-linkdialog-abschnitt input {
    width: 100%;
    padding: var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: inherit;
  }

  .gw-linkdialog-treffer {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-1);
    max-height: 14rem;
    overflow-y: auto;
  }

  .gw-linkdialog-treffer button {
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

  .gw-linkdialog-treffer button:hover,
  .gw-linkdialog-treffer button:focus-visible {
    border-color: var(--color-border);
    background: var(--color-surface-2, var(--color-bg));
  }

  .gw-linkdialog-pfad,
  .gw-linkdialog-hinweis,
  .gw-linkdialog-leer {
    color: var(--color-text-muted);
    font-size: var(--font-size-sm);
  }

  .gw-linkdialog-hinweis,
  .gw-linkdialog-leer {
    margin: 0;
  }

  .gw-linkdialog-fehler {
    margin: 0;
    color: var(--color-danger, crimson);
    font-size: var(--font-size-sm);
  }

  .gw-linkdialog-fuss {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    justify-content: flex-end;
    margin-top: var(--space-4);
  }

  .gw-linkdialog-fuss button,
  .gw-linkdialog-abschnitt button[type='submit'] {
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-bg);
    color: inherit;
    cursor: pointer;
    justify-self: start;
  }
</style>
