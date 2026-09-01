<!--
  What this page is about: its topics, as chips beneath the title, with the control that adds
  and removes one right beside them.

  **The owner's second decision, in full: tagging is something you do while reading.** Not
  something that requires opening the editor, and not something that lives in a settings
  panel — you are looking at the page, you can see what it is filed under, and you can change
  it there. Clicking a chip browses that topic, which is the only way topics are reachable at
  all (D-4).

  **Everything works before hydration.** The chips are links; adding is a real form action;
  removing is a submit button inside one form that serves every chip, carrying which topic in
  its own value — the shape a board's move buttons already use. A control that only works once
  a bundle arrives is a control that looks live and does nothing, which is what
  `[...path]/+page.svelte` records about its own edit link.

  **The suggestion list is the index.** ADR 0011 says plainly that an autocomplete is a
  disclosure surface and is the one that feels like a convenience and therefore gets
  forgotten. It cannot be forgotten here, because there is no second request to forget to
  filter: the `<datalist>` is fed the very array `/themen` and the sidebar render, which is
  `GET /api/topics` — already narrowed to the topics this caller may see at all.

  **A `<datalist>` rather than a written-from-scratch combobox**, deliberately. It is the
  browser's own: it filters as you type, it is operable by keyboard and by pointer, it is
  announced correctly by screen readers, and — the point — it works in the first response
  with no script at all. A hand-built listbox would be several hundred lines that only start
  working once JavaScript arrives, to do worse what is already there.
-->
<script lang="ts">
  import { topicHref, TOPICS_REGION_ID, type Topic, type TopicSummary } from '$lib/topics';

  interface Props {
    /** The topics this page states, as the API answered them. Not re-sorted. */
    themen: Topic[];
    /**
     * Every topic this caller may see — the suggestion source, and the same array the index
     * and the sidebar render. See the note above about why it is not a second request.
     */
    alle: TopicSummary[];
    /**
     * Whether this caller may write this page, straight from `/api/documents`.
     *
     * **The same verdict that refuses the write**, not a second one that agrees with it
     * today: `DocumentView::may_write` falls out of the very authorisation that let the page
     * be read. So a control offered here is a control that will work, and one withheld is one
     * that would have been refused — which is the whole of ADR 0010.
     */
    darfSchreiben: boolean;
    /** Why the last change did not happen. Never conflated with "there are no topics". */
    fehler: string | null;
    /** What was typed, so a refused submission does not also make somebody type it again. */
    getippt?: string;
    /** What a chip links to, when that is not simply the topic's own address. */
    hrefFor?: (href: string) => string;
  }

  let { themen, alle, darfSchreiben, fehler, getippt = '', hrefFor }: Props = $props();

  /** The one datalist on this page. Named once; the field points at it by id. */
  const VORSCHLAEGE = 'gw-themen-vorschlaege';

  function ziel(topic: Topic): string {
    const href = topicHref(topic);
    return hrefFor ? hrefFor(href) : href;
  }
</script>

{#snippet chips()}
  <ul class="chips">
    {#each themen as thema (thema.path)}
      <li class="chip">
        <!-- The WHOLE path on a chip — `Rundgang/Tabellen`, not `Tabellen`. The opposite
             decision from the tree, and for a reason: a chip has no list around it to say
             where the topic sits, so it has to say so itself. Spelled with `/`, the same
             separator the field below takes and a file states, so that what somebody reads
             here is what they can retype there. -->
        <a class="chip-name" href={ziel(thema)}>{thema.display_path}</a>
        {#if darfSchreiben}
          <button
            type="submit"
            name="pfad"
            value={thema.path}
            class="chip-weg"
            aria-label={`Thema »${thema.display_path}« entfernen`}
          >
            <span aria-hidden="true">×</span>
          </button>
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<!-- Rendered at all only when there is something to say or something to do. Most pages in
     this wiki carry no topic and most readers may not write them; a "Keine Themen" row under
     every title would be furniture paid for by every one of them. `Backlinks` and `Subpages`
     make the same call for the same reason. -->
{#if themen.length > 0 || darfSchreiben || fehler}
  <!-- The landing place a finished change comes back to. `tabindex="-1"` so the browser can
       put focus here without making it a tab stop: a live region that is already in the
       document announces nothing, and moving focus into this one is what gets the new list
       read out — with no script, which is the requirement. -->
  <nav
    id={TOPICS_REGION_ID}
    class="themen no-print"
    aria-label="Themen dieser Seite"
    tabindex="-1"
  >
    {#if themen.length === 0}
      <!-- Not when something went wrong: a page with no topics and a request that did not
           come back are different things, and printing »Keine Themen« beside a failure would
           say both at once. The same distinction every empty state in this interface makes;
           here it just happens to share one prop with a refused change. -->
      {#if !fehler}<p class="keine">Keine Themen.</p>{/if}
    {:else if darfSchreiben}
      <!-- ONE form around the whole list, with a button per chip carrying which topic in its
           value. Each removal is an ordinary submission; the action reads the page's current
           set, drops this one and puts the rest back, because the endpoint takes the whole
           set — that is what a frontmatter line says and what a file drop has to mean. -->
      <form method="post" action="?/themaEntfernen">
        {@render chips()}
      </form>
    {:else}
      {@render chips()}
    {/if}

    {#if darfSchreiben}
      <form method="post" action="?/themaHinzufuegen" class="neu">
        <label for="thema">Thema hinzufügen</label>
        <p id="thema-hinweis" class="hint">
          Freie Schlagwörter: tippen Sie eines, das es schon gibt — es wird vorgeschlagen —
          oder ein neues. <code>Medizin/Darm</code> legt »Darm« innerhalb von »Medizin« an.
        </p>
        <div class="row">
          <input
            id="thema"
            name="thema"
            type="text"
            list={VORSCHLAEGE}
            required
            autocomplete="off"
            spellcheck="false"
            placeholder="Medizin/Darm"
            value={getippt}
            aria-invalid={fehler ? 'true' : undefined}
            aria-describedby={fehler ? 'thema-hinweis thema-fehler' : 'thema-hinweis'}
          />
          <button type="submit" class="btn">Hinzufügen</button>
        </div>
        <datalist id={VORSCHLAEGE}>
          <!-- The STATED spelling, never the canonical path: `set_document_topics` refuses a
               leading separator, so an option of `/rundgang/tabellen` would be a suggestion
               that cannot be accepted. -->
          {#each alle as vorschlag (vorschlag.path)}
            <option value={vorschlag.display_path}></option>
          {/each}
        </datalist>
      </form>
    {/if}

    <!-- OUTSIDE the form, deliberately, and for a defect /projekte already paid for: a
         session that expires between the render and the submit withdraws the form — and with
         it, when the message lived inside, the sentence explaining what had just happened.
         `aria-describedby` is by id and does not care that the paragraph sits beside the form
         rather than in it. In words and announced, never a red border alone. -->
    {#if fehler}
      <p id="thema-fehler" class="notice notice--error" role="alert">{fehler}</p>
    {/if}
  </nav>
{/if}

<style>
  /* `@layer components`, the plugin contract (ADR 0005). */
  @layer components {
    .themen > * + * {
      margin-block-start: var(--space-2);
    }

    .chips {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
    }

    .chip {
      display: inline-flex;
      align-items: stretch;
      border: 1px solid var(--border-strong);
      border-radius: 999px;
      background: var(--bg-raised);
      overflow: hidden;
    }

    .chip-name {
      padding: var(--space-1) var(--space-3);
      color: var(--accent);
      text-decoration: none;
      font-size: var(--text-sm);
      line-height: 1.4;
      overflow-wrap: anywhere;
    }

    .chip-name:hover,
    .chip-name:focus-visible {
      background: var(--accent-soft);
      text-decoration: underline;
      text-underline-offset: 0.15em;
    }

    .chip-weg {
      border: 0;
      border-inline-start: 1px solid var(--border);
      background: transparent;
      color: var(--ink-muted);
      font: inherit;
      font-size: var(--text-sm);
      line-height: 1.4;
      padding: var(--space-1) var(--space-2);
      cursor: pointer;
    }

    .chip-weg:hover,
    .chip-weg:focus-visible {
      background: var(--accent-soft);
      color: var(--ink);
    }

    .keine,
    .hint {
      color: var(--ink-muted);
      font-size: var(--text-sm);
      max-width: var(--measure);
    }

    label {
      display: block;
      font-size: var(--text-sm);
      font-weight: 650;
    }

    .neu > * + * {
      margin-block-start: var(--space-1);
    }

    .row {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      align-items: center;
    }

    input {
      flex: 1 1 18rem;
      min-inline-size: 0;
      padding: var(--space-1) var(--space-3);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg);
      color: var(--ink);
      font: inherit;
      font-size: var(--text-sm);
    }

    /* The colour is the second channel; the sentence below the field is the first. */
    input[aria-invalid='true'] {
      border-color: var(--danger);
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

    code {
      font-family: var(--font-mono);
      font-size: 0.9em;
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

    /* The region takes focus after a change so that the new list is read out. Without this
       it would take focus with no ring, which is a reader landing somewhere invisible. */
    .themen:focus-visible {
      outline: 2px solid var(--focus);
      outline-offset: 2px;
    }
  }
</style>
