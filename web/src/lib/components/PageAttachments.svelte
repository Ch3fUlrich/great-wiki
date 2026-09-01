<!--
  What a page carries besides its words: the `Anhänge` list, and the control that adds to it.

  **The list is the authority (D-15), and that is the whole reason this section exists.** A
  file is attached because there is a row saying so, never because a paragraph mentions it —
  so cutting a picture out of a sentence leaves the file exactly where it was, and this is the
  only place that fact is visible. `gw_store::attachments` states the same rule from the other
  side, and states the consequence: nothing here is derived from the document's body, and
  nothing may become so.

  **Fetching a file is a link, and the address is the one the API sent (D-16).** A download is
  authorised against *the page it was reached through*, which is only true while the page is
  part of the address — so the API composes `href` and this component prints it. Nothing here
  assembles an address, and there is no content address on the wire to assemble one from.

  **Everything works before hydration.** The download is an `<a>`; the upload is a real
  multipart form submission to a form action, which unpacks the file and sends the bytes on.
  A control that only works once a bundle arrives is a control that looks live and does
  nothing, which is what `[...path]/+page.svelte` records about its own edit link.

  **The field states no list of file types.** `gw_store::blobs::sniff` owns the accepted set
  and is being widened; an `accept` attribute here would be a second answer that refuses a file
  the wiki would have taken — before the request is made, with nothing in any log to say why.
  The same goes for the size cap, which is `MAX_ATTACHMENT_BYTES` and not a number in this
  file. A refusal is framed in German and carries the server's own words inside it.

  **What is deliberately NOT here: the inline placement.** D-15 puts a file inline in the prose
  as well as in this list. That needs a new `BlockKind`, which is `#[non_exhaustive]` with four
  hand-maintained mirrors that fail silently — one of them once destroyed a checklist in the
  CRDT and broadcast the deletion. It is its own piece of work. Nothing in this component
  reaches into the document, and nothing here should start to.
-->
<script lang="ts">
  import {
    ATTACHMENTS_REGION_ID,
    formatInstant,
    kindText,
    sizeText,
    type Attachment
  } from '$lib/attachments';

  interface Props {
    /** What is attached, as the API answered it. Not re-sorted — the store orders by name. */
    anhaenge: Attachment[];
    /**
     * Whether this caller may attach here, straight from `GET /api/attachments/{path}`.
     *
     * **The same verdict that refuses the write**, not a second one that agrees with it
     * today: it falls out of the very authorisation that produced this list (ADR 0010). So a
     * control offered on it is one that will work, and one withheld is one that would have
     * been refused.
     */
    darfSchreiben: boolean;
    /**
     * Whether anybody is signed in — the other half, and `Store::attach` checks it FIRST.
     *
     * It refuses an unauthenticated caller before it consults a single grant, for the reason
     * a revision needs an author: the row records who put the file there, and "nobody" is not
     * an answer. A path carrying `anyone: write` makes a page writable by somebody who has
     * not said who they are, and putting a quarter of a gigabyte on the mount through one is
     * not the same act as editing a paragraph.
     */
    angemeldet: boolean;
    /** Why the list is not there, or why the last upload did not happen. Never conflated
     *  with "this page carries nothing". */
    fehler: string | null;
    /** What has just been attached, taken from the list itself so the notice cannot name a
     *  file the list below does not show. */
    hochgeladen: Attachment | null;
  }

  let { anhaenge, darfSchreiben, angemeldet, fehler, hochgeladen }: Props = $props();

  /** The one file field on this page. Named once; the label points at it by id. */
  const FELD = 'gw-anhang-datei';

  /** Both halves, composed. See the two props above for why neither alone is the question. */
  const darfHochladen = $derived(darfSchreiben && angemeldet);
</script>

<!-- Rendered at all only when there is something to say or something to do. Most pages in this
     wiki carry no file and most readers may not write them; an »Anhänge / Keine Anhänge« block
     under every one of them is furniture paid for by every reader who never asked. The call
     `Backlinks`, `Subpages` and `PageTopics` all make, for the same reason. -->
{#if anhaenge.length > 0 || darfSchreiben || fehler}
  <!-- The landing place a finished upload comes back to. `tabindex="-1"` so the browser can
       put focus here without making it a tab stop: a live region that is already in the
       document announces nothing, and moving focus into this one is what gets the notice read
       out — with no script, which is the requirement. -->
  <section
    class="anhaenge no-print"
    id={ATTACHMENTS_REGION_ID}
    tabindex="-1"
    aria-labelledby="anhaenge-titel"
  >
    <h2 id="anhaenge-titel">Anhänge</h2>

    {#if hochgeladen}
      <p class="notice notice--ok" role="status">
        »{hochgeladen.filename}« ist jetzt angehängt — {kindText(hochgeladen.media_type)},
        {sizeText(hochgeladen.byte_size)}. Die Datei steht unten in der Liste.
      </p>
    {/if}

    {#if anhaenge.length === 0}
      <!-- Not when something went wrong: a page with no files and a request that did not come
           back are different things, and printing »Keine Anhänge« beside a failure would say
           both at once — and this list is the authority on what a page carries, so saying it
           falsely is worse here than anywhere else in the interface. -->
      {#if !fehler}
        <p class="keine">Keine Anhänge.</p>
      {/if}
    {:else}
      <ul class="liste">
        {#each anhaenge as anhang (anhang.filename)}
          <li>
            <!-- A LINK, and the address is the API's own. It works before hydration, it can be
                 opened in a new tab, and it can be saved with a right-click — none of which is
                 true of a button that fetches.

                 `data-sveltekit-reload` says out loud what would happen anyway: `/api/…` is
                 not a route in this app, so the client-side router hands it to the browser.
                 Stating it means a future route under `/api` cannot quietly turn a download
                 into a client-side navigation that renders nothing. -->
            <a class="datei" href={anhang.href} data-sveltekit-reload>{anhang.filename}</a>
            <p class="fakten">
              <!-- In text, never an icon: the type and the size are what somebody needs before
                   deciding to fetch 1,2 MB over a phone connection, and a picture of a
                   document says neither of them to a screen reader. The exact media type sits
                   beside the German word because »Bild« does not tell a PNG from an AVIF —
                   and it is what the bytes ARE, sniffed from the file, never what an upload
                   claimed. -->
              <span>{kindText(anhang.media_type)}</span>
              <span class="typ">{anhang.media_type}</span>
              <span>{sizeText(anhang.byte_size)}</span>
            </p>
            <p class="herkunft">
              Hochgeladen von {anhang.uploaded_by_name} am {formatInstant(anhang.uploaded_at)}
            </p>
          </li>
        {/each}
      </ul>
    {/if}

    {#if darfHochladen}
      <form method="post" action="?/anhaengen" enctype="multipart/form-data" class="neu">
        <!-- »Datei auswählen«, and the button is »Hochladen«: the label names the FIELD and
             the button names the ACT. Naming both of them after uploading gives a screen
             reader two controls whose names differ by one word — and it is not hypothetical,
             the browser check found it: `getByRole('button', { name: 'Hochladen' })` matched
             the file input as well as the button, because a file input IS a button in the
             accessibility tree and took its name from this label. -->
        <label for={FELD}>Datei auswählen</label>
        <p id="anhang-hinweis" class="hint">
          Die Datei bleibt an dieser Seite hängen, auch wenn sie im Text nicht vorkommt. Welche
          Dateitypen und welche Größe angenommen werden, entscheidet der Server — er sagt es,
          wenn er etwas ablehnt.
        </p>
        <div class="row">
          <!-- No `accept`. See the note at the top of this file: the allowlist is the
               server's, it is being widened, and a copy here would refuse a file the wiki
               would have taken. -->
          <input
            id={FELD}
            name="datei"
            type="file"
            required
            aria-invalid={fehler ? 'true' : undefined}
            aria-describedby={fehler ? 'anhang-hinweis anhang-fehler' : 'anhang-hinweis'}
          />
          <button type="submit" class="btn">Hochladen</button>
        </div>
      </form>
    {:else}
      <!-- Written out rather than left blank. A control that is silently not there reads as a
           fault just as easily as an answer — the pattern `/papierkorb` established for its
           own withheld restore. Which sentence appears is decided by which half of
           `Store::attach`'s gate this caller fails, so it sends them to the right fix. -->
      {#if !darfSchreiben}
        <p class="muted">Hochladen darf nur, wer diese Seite bearbeiten darf.</p>
      {:else}
        <p class="muted">
          Zum Hochladen müssen Sie angemeldet sein: ein Anhang hält fest, wer ihn hochgeladen
          hat.
        </p>
      {/if}
    {/if}

    <!-- OUTSIDE the form, deliberately, and for a defect `/projekte` already paid for: a
         session that expires between the render and the submit withdraws the form — and with
         it, when the message lived inside, the sentence explaining what had just happened.
         `aria-describedby` is by id and does not care that the paragraph sits beside the form
         rather than in it. In words and announced, never a red border alone. -->
    {#if fehler}
      <p id="anhang-fehler" class="notice notice--error" role="alert">{fehler}</p>
    {/if}
  </section>
{/if}

<style>
  /* `@layer components`, the plugin contract (ADR 0005). */
  @layer components {
    .anhaenge > * + * {
      margin-block-start: var(--space-3);
    }

    h2 {
      font-size: var(--text-xl);
      line-height: var(--leading-tight);
    }

    .liste {
      list-style: none;
      margin: 0;
      padding: 0;
      display: flex;
      flex-direction: column;
      gap: var(--space-3);
    }

    .liste li {
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--border);
      border-inline-start: 3px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
    }

    .datei {
      color: var(--accent);
      font-size: var(--text-base);
      text-decoration: none;
      /* A filename is not a sentence and can be long; it wraps rather than widening the
         column or being cut off. */
      overflow-wrap: anywhere;
    }

    .datei:hover,
    .datei:focus-visible {
      text-decoration: underline;
      text-underline-offset: 0.15em;
    }

    .fakten {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1) var(--space-3);
      margin-block-start: var(--space-1);
      color: var(--ink-muted);
      font-size: var(--text-sm);
    }

    .typ {
      font-family: var(--font-mono);
      font-size: 0.9em;
    }

    .herkunft {
      color: var(--ink-muted);
      font-size: var(--text-sm);
    }

    .keine,
    .hint,
    .muted {
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

    input[type='file'] {
      flex: 1 1 18rem;
      min-inline-size: 0;
      padding: var(--space-1) var(--space-2);
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

    .notice {
      padding: var(--space-3) var(--space-4);
      border: 1px solid var(--border);
      border-inline-start-width: 3px;
      border-radius: var(--radius-sm);
      background: var(--bg-raised);
      color: var(--ink);
      font-size: var(--text-sm);
      max-width: var(--measure);
    }

    .notice--ok {
      border-inline-start-color: var(--accent);
    }

    .notice--error {
      border-inline-start-color: var(--danger);
    }

    /* The section takes focus after an upload so that the notice is read out. Without this it
       would take focus with no ring, which is a reader landing somewhere invisible. */
    .anhaenge:focus-visible {
      outline: 2px solid var(--focus);
      outline-offset: 2px;
    }
  }
</style>
