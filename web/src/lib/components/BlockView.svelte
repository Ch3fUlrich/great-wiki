<script lang="ts">
  import type { Block, EmbeddedPage, Mark, Reference } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { codeText, embeddedPage, placedFile, plainText, safeHref } from '$lib/blocks/render';
  import { alignOf } from '$lib/blocks/table';
  import {
    attachmentNamed,
    describeMissingPlacement,
    isPicture,
    kindText,
    sizeText,
    type Attachment
  } from '$lib/attachments';
  import { isDiagramFence } from '$lib/blocks/diagram';
  import { isMathFence, type Formulas } from '$lib/blocks/maths';
  import type { Fences } from '$lib/blocks/code';
  import Self from './BlockView.svelte';
  import CodeView from './CodeView.svelte';
  import DiagramView from './DiagramView.svelte';
  import MathView from './MathView.svelte';
  import TableView from './TableView.svelte';

  interface Props {
    block: Block;
    /**
     * This page's `Anhänge` list, as the API answered it — what an `attachment` block is
     * resolved against.
     *
     * **Passed down rather than fetched, and passed down rather than read from a context.**
     * A download is authorised against the page it was reached through (D-16), so the only
     * address this component may use is the `href` the API built for THIS page's list; a
     * component that went looking for the list itself would be a second retrieval path for
     * something already answered, and one that read an ambient context would silently render
     * nothing wherever the context happened not to be set.
     *
     * Empty by default, which is the honest state for every caller that has no list: the
     * placement then says the file is not attached, which is exactly what an empty list
     * means. It never guesses an address from the name.
     */
    anhaenge?: Attachment[];
    /**
     * Every ` ```math ` fence on this page, typeset by the page's own `load`.
     *
     * **Passed down for the same reason `anhaenge` is, plus one of its own.** KaTeX is
     * called in `$lib/server/maths`, which SvelteKit refuses to let any client-reachable
     * module import — so the library never reaches the reader's browser and a formula is in
     * the first response, typeset, with JavaScript switched off. A component that called
     * KaTeX itself would render on the server and AGAIN while hydrating, which is how about
     * 272 kB of maths library ends up in a bundle to re-derive markup already sent.
     *
     * `null` by default, and that is the honest state for every caller with no page load
     * behind it — the editor renders this component while TipTap mounts. A fence then shows
     * its own source and says nothing, because being refused and never being asked are
     * different states and must not be reported as the same one.
     */
    formeln?: Formulas | null;
    /**
     * Every fenced code block on this page, tokenised by the page's own `load`.
     *
     * Here for exactly `formeln`'s reasons, arrived at the same way. Shiki is called in
     * `$lib/server/highlight`, which SvelteKit refuses to let any client-reachable module
     * import — so the highlighter and its eight grammars never reach the reader's browser,
     * a listing is coloured in the first response and with JavaScript switched off, and the
     * per-page caps are applied where the page is, rather than one block at a time where
     * nothing can see how many there are.
     *
     * `null` by default, and honest for every caller with no page load behind it: a fence
     * then renders exactly as typed and says nothing.
     */
    fences?: Fences | null;
    /**
     * Where this page's document references point, **for the reader this page is being
     * rendered for** — `gw_store::Store::references_for`'s answer, keyed by document id.
     *
     * Passed down for `anhaenge`'s reason and one of its own. A link stores the target's
     * IDENTITY (D-5), so its address and its name are resolved when the page is read rather
     * than when it was written — which is what makes a link survive the target being renamed
     * or moved. That resolution is a permission decision about a DIFFERENT page from the one
     * being read, so it belongs to the server and to nobody else: a component that looked a
     * target up itself would be a second answer to a question `Store::references_for`
     * already answers, written where the answer cannot be trusted.
     *
     * **An id with no entry here renders as the author's own text and nothing else** — no
     * anchor, no address, no tooltip. It is a page this reader may not read, one in the
     * Papierkorb, one that was purged, one that never existed, or one past the per-page cap,
     * and those are deliberately indistinguishable: telling them apart is itself the
     * disclosure. The words stay because they are the author's and are in a body this reader
     * is already reading; the target's current path and title do not, because they are the
     * target's and a rename would otherwise keep reporting its new name. ADR 0019.
     *
     * `{}` by default, and honest for every caller with no page load behind it — the editor
     * renders this component while TipTap mounts. Every reference then renders as text,
     * which is the same thing a reference to an unreadable page does, so nothing there can
     * disclose more than a reader would see.
     */
    verweise?: Record<string, Reference>;
    /**
     * What every `embed` block on this page may show **this reader** —
     * `gw_store::Store::embeds_for`'s answer, keyed by `embedKey`.
     *
     * Passed down for `verweise`' reason and one that is sharper: an embed puts a DIFFERENT
     * page's words on this one, so the permission question is about a page this component has
     * never read. A component that fetched the target itself would be a second answer to that
     * question, computed where the answer cannot be trusted.
     *
     * **A block with no entry here renders as the author's own label and nothing else** — no
     * frame contents, no title, no address. It is a page this reader may not read, one in the
     * Papierkorb, one that was purged, one that never existed, or one past the per-page cap,
     * and those are deliberately indistinguishable: telling them apart is the disclosure.
     *
     * `{}` by default, and honest for every caller with no page load behind it — the editor
     * renders this component while TipTap mounts. Every embed then shows its label, which is
     * the same thing an embed of an unreadable page shows, so nothing there can disclose more
     * than a reader would see.
     */
    einbettungen?: Record<string, EmbeddedPage>;
    /**
     * Whether this subtree is being drawn INSIDE an embed's frame.
     *
     * **Depth is one, and this is how.** The blocks handed back for an embed may themselves
     * hold embeds; inside a frame they render as their labels rather than expanding, which
     * closes the recursion by construction rather than by remembering to carry a visited set,
     * and bounds what one page read can cost. It also decides the two things D-30 is about: a
     * quoted checklist says where ticking happens, and a quoted file says where it is shown,
     * because neither belongs to the page being read.
     */
    eingebettet?: boolean;
    /**
     * Where the source page of the frame this subtree is inside lives, for the links those
     * two states need. `null` outside a frame.
     */
    quelle?: { path: string; title: string } | null;
  }

  let {
    block,
    anhaenge = [],
    formeln = null,
    fences = null,
    verweise = {},
    einbettungen = {},
    eingebettet = false,
    quelle = null
  }: Props = $props();

  /** Whether anything under `b` is a checklist line — what D-30's note is offered for. */
  function hatAufgaben(b: Block): boolean {
    if (b.kind === 'taskItem') return true;
    return (b.content ?? []).some(hatAufgaben);
  }
</script>

<!-- Only known kinds render. An unknown block is skipped rather than emitted raw, which is
     why there is no sanitisation step here: no untrusted HTML is ever constructed.

     One qualification, and it is the whole of it. `MathView` puts KaTeX's own output into
     the page as markup — the single exception in this reader, kept to one leaf on purpose,
     with the three things that make its input safe written on that component and a
     line-exact permission in `scripts/check-html-sinks.sh` rather than a file exemption.
     Nothing else here constructs markup from anything, and the exemption list in that
     script is still empty.

     `DiagramView` is deliberately NOT a second one. Mermaid also hands back a string of
     markup, and it goes into an `<img src>` as a data URI — an attribute rather than
     markup — which is the containment ADR 0014 already requires for an uploaded SVG. -->

{#if block.kind === 'doc'}
  {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}
{:else if block.kind === 'paragraph'}
  <p>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</p>
{:else if block.kind === 'heading'}
  {@const level = Math.min(6, Math.max(1, Number(block.attrs?.level ?? 1)))}
  {@const id = slugify(plainText(block))}
  <svelte:element this={`h${level}`} {id}>
    {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}
  </svelte:element>
{:else if block.kind === 'bulletList'}
  <ul>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</ul>
{:else if block.kind === 'orderedList'}
  <ol>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</ol>
{:else if block.kind === 'listItem'}
  <li>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</li>
{:else if block.kind === 'taskList'}
  <!-- A checklist. `data-type` is the attribute TipTap's own `TaskList` puts on its `<ul>`,
       so the editor and the reader can be styled by one rule instead of two that drift. -->
  <ul class="task-list" data-type="taskList">
    {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}
  </ul>
{:else if block.kind === 'taskItem'}
  {@const checked = block.attrs?.checked === true}
  <!-- A real `<input type="checkbox">`, not a glyph: a native checkbox is what tells a
       screen reader "checked"/"not checked" without any ARIA, and a ✓ or a styled span
       conveys the state by appearance alone — invisible to anybody not looking at it.

       `disabled`, and deliberately so. Per design decision D-2 the page owns the words and
       the RECORD owns the workflow state, so ticking a box in the reading view must not be
       possible: it would need write permission on the page for a click, and would file a
       revision nobody typed. Real interactivity belongs to the board, and waits for the
       board API — until then a control that looks live and does nothing is worse than one
       that plainly is not.

       Named from its own line rather than left anonymous. A page can hold many of these,
       and "checkbox, checked" with nothing else is what a reader gets from an unnamed one
       when moving control by control. The name is the item's FIRST child — its own text —
       not `plainText(block)`, which for a task with a checklist under it would read out
       every line beneath it as well. -->
  <li class="task-item" data-type="taskItem" data-checked={checked}>
    <input type="checkbox" {checked} disabled aria-label={plainText(block.content?.[0] ?? block)} />
    <div>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</div>
  </li>
{:else if block.kind === 'blockquote'}
  <blockquote>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</blockquote>
{:else if block.kind === 'codeBlock'}
  <!-- `codeText`, never `plainText`: the whitespace IS the content of a fence, and
       `plainText` collapses it — see its own doc comment for why widening THAT is not the
       fix.

       ```math is typeset, ```mermaid is drawn, and everything else is printed — and that is
       the only difference between the three branches: all of them are `BlockKind::CodeBlock`
       carrying a `language`, and none of them adds a kind, an attribute or a mirror (D-18).
       `isMathFence` is asked here and by the walker that filled `formeln`, so the two cannot
       disagree about which fence is a formula — if they could, a fence one of them claimed
       would silently render as neither.

       `CodeView` owns the `<pre>`, the highlighting and the note about a language this wiki
       does not know; `MathView` owns the one place in this reader where a string becomes
       markup, and its doc comment says what makes that safe; `DiagramView` owns the pair of
       `<img>` a diagram is drawn into and the library that never reaches this server. -->
  {#if isMathFence(block.attrs?.language)}
    <MathView source={codeText(block)} {formeln} />
  {:else if isDiagramFence(block.attrs?.language)}
    <DiagramView source={codeText(block)} />
  {:else}
    <CodeView text={codeText(block)} language={block.attrs?.language} {fences} />
  {/if}
{:else if block.kind === 'table'}
  <!-- TableView owns the scroll box, the sticky header and — once it has mounted in a
       browser — sorting and filtering. It renders no cell content itself: `nested` hands
       the recursion below straight back to it, so there is exactly one block renderer and
       no import cycle between the two components. -->
  <TableView {block} child={nested} />
{:else if block.kind === 'tableRow'}
  <tr>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</tr>
{:else if block.kind === 'tableHeader'}
  <th scope="col" style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</th
  >
{:else if block.kind === 'tableCell'}
  <td style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/each}</td
  >
{:else if block.kind === 'attachment'}
  <!-- A file placed in the prose (D-15). Three outcomes, and which one applies is decided by
       the page's `Anhänge` list rather than by anything in the block: the list is the
       authority on what is attached, and the block is a REFERENCE to a row in it.

       `attachmentNamed` is where that resolution lives and its doc comment has the reasoning;
       what matters here is what falls out of it. The address is the API's own `href`, which
       names the page and does not name the bytes — nothing in this interface assembles one.
       And whether a file is shown or offered is decided by `media_type`, which
       `gw_store::blobs::sniff` read out of the bytes, so renaming a PDF to `.png` changes
       nothing about how it renders. -->
  {@const placed = placedFile(block)}
  {#if placed}
    {@const anhang = eingebettet ? null : attachmentNamed(anhaenge, placed.filename)}
    {#if eingebettet}
      <!-- A file placed in an EMBEDDED page is that page's file, not this one's. Resolving it
           against the host page's `Anhänge` list would make every picture inside a frame read
           the German "not attached" sentence — a false statement about somebody else's page —
           and guessing an address would be worse: a download is authorised against the page
           it was reached through (D-16), and there is deliberately no address built from a
           digest. So the frame names the file and says where it is shown. -->
      <p class="datei-fehlt">
        »{placed.filename}« steht auf {#if quelle}<a
            href={safeHref(quelle.path) ?? ''}
            rel="noopener noreferrer">{quelle.title}</a
          >{:else}der Quellseite{/if}.
      </p>
    {:else if anhang === null}
      <!-- Stated, not drawn as a broken picture. See `describeMissingPlacement`: an `<img>`
           whose source 404s renders as an icon and reads as "the network failed", and the
           truth is different and actionable — the file was detached (which deliberately does
           not touch the prose) or has not been uploaded yet. -->
      <p class="datei-fehlt">{describeMissingPlacement(placed.filename, placed.alt)}</p>
    {:else if isPicture(anhang.media_type)}
      <!-- `<img>`, and NOTHING ELSE, ever. An SVG is a picture by media type and a program by
           capability — it is stored exactly as uploaded, because nothing sanitises it — and
           `<img src>` is one of the two contexts no browser executes it in. `<object>`,
           `<embed>` and `<iframe>` all run it, and putting its markup into this wiki's own DOM
           would run it IN THIS ORIGIN with the session cookie in reach. ADR 0014 and
           `content_disposition` in `gw_api::routes::attachments` state the same constraint
           from the server's side. There is exactly one element here so that there is exactly
           one thing to get right.

           The description is the alt text and there is deliberately no visible caption: that
           is what `![Beschreibung](anhang:datei.png)` means in markdown, and a caption
           carrying the same words would have every screen reader announce them twice.

           A placement is never decorative — somebody put it in the middle of their prose — so
           an empty description falls back to the FILENAME rather than to `alt=""`, which
           would make the picture invisible to a screen reader entirely. A filename is a poor
           description and it is the same string the `Anhänge` list below names the file by, so
           a reader who cannot see the picture can at least find and fetch it. -->
      <figure class="datei-bild">
        <img src={anhang.href} alt={placed.alt === '' ? anhang.filename : placed.alt} loading="lazy" />
      </figure>
    {:else}
      <!-- Everything that is not a picture: a card that downloads. A LINK, so it works before
           hydration, opens in a new tab and saves with a right-click — and the server already
           sends `Content-Disposition: attachment` for it, so following it saves the file
           rather than replacing the page.

           The type and the size are in words for the same reason the `Anhänge` list states
           them: they are what somebody needs before deciding to fetch 1,2 MB over a phone
           connection, and a picture of a document says neither to a screen reader. The exact
           media type sits beside the German word because »Datei« does not tell a CSV from a
           ZIP — and it is what the bytes ARE, never what an upload claimed. -->
      <a class="datei-karte" href={anhang.href} data-sveltekit-reload>
        <span class="datei-name">{placed.alt === '' ? anhang.filename : placed.alt}</span>
        <span class="datei-fakten">
          <span>{kindText(anhang.media_type)}</span>
          <span class="datei-typ">{anhang.media_type}</span>
          <span>{sizeText(anhang.byte_size)}</span>
          {#if placed.alt !== ''}<span class="datei-typ">{anhang.filename}</span>{/if}
        </span>
      </a>
    {/if}
  {/if}
{:else if block.kind === 'embed'}
  <!-- A live view of another page, or of one section of it (D-27). Four outcomes, and which
       one applies is decided by the map the server filled and by nothing in the block: an
       embed is a REFERENCE, and what may be said about its target is a permission decision
       about a page this component has never read.

       D-28: FRAMED, WITH THE SOURCE NAMED. Never seamless. Seamless reads better and leaves a
       reader unable to tell which words are this page's own — and an edit made "here" would
       land silently on a different page under a different ACL. Honesty about provenance is
       worth a border.

       The address is `ziel.path`, from the server's own resolution, through `safeHref` like
       every other one — the same sink, not a second judgement of what is safe. Nothing here
       interpolates `attrs.doc`, which is an arbitrary string that reached `documents.body`
       over the collaboration socket with nothing validating it. -->
  {@const ref = embeddedPage(block)}
  {#if ref}
    {@const ziel = einbettungen[ref.key]}
    {@const zielHref = ziel ? safeHref(ziel.path) : null}
    {#if !ziel || zielHref === null}
      <!-- The page is not this reader's to see, is in the Papierkorb, was purged, never
           existed, or is past the per-page cap. One state, one sentence, because telling
           them apart is itself the disclosure — and the author's own label stays, because it
           is the author's and is in a body this reader is already reading. -->
      <div class="einbettung einbettung-fremd" data-einbettung={ref.key}>
        <p class="einbettung-kopf">Eingebettete Seite</p>
        <p class="einbettung-wort">
          {ref.label === '' ? 'Diese Seite ist hier nicht verfügbar.' : ref.label}
        </p>
      </div>
    {:else if (ziel.cycle ?? []).length > 0}
      <!-- A cycle, stopped at render and NAMED. It is allowed to exist — refusing a publish
           because somebody else's page points back here would mean reading pages the author
           may not read — so it is caught where both ends are known. Every title in the chain
           is a page this reader may read; one passing through a page they may not is simply
           not found, exactly as it is not listed anywhere else. -->
      <div class="einbettung einbettung-kreis" data-einbettung={ref.key}>
        <p class="einbettung-kopf">
          Eingebettet aus <a href={zielHref} rel="noopener noreferrer">{ziel.title}</a>
        </p>
        <p class="einbettung-wort">
          Diese Einbettung würde sich selbst enthalten und wird deshalb nicht angezeigt:
          {(ziel.cycle ?? []).join(' → ')} → diese Seite.
        </p>
      </div>
    {:else if !ziel.body}
      <!-- D-29: an orphaned embed STAYS and says so. Never a fall back to the whole page,
           which never shows an empty frame and quietly swaps a dosage table for a page the
           author never meant to quote; never nothing at all, because nothing here vanishes
           silently (the rule a detached task follows, D-8). -->
      <div class="einbettung einbettung-verwaist" data-einbettung={ref.key}>
        <p class="einbettung-kopf">
          Eingebettet aus <a href={zielHref} rel="noopener noreferrer">{ziel.title}</a>
        </p>
        <p class="einbettung-wort">
          Den Abschnitt gibt es auf dieser Seite nicht mehr. Er wurde gelöscht oder mit einem
          anderen zusammengeführt.
        </p>
      </div>
    {:else}
      <!-- The frame. `eingebettet` is what makes depth one: an embed inside these blocks
           renders as its own label rather than expanding, which closes the recursion by
           construction. `verweise` is the EMBEDDED body's own references, resolved against
           this same reader — reusing the host page's map would leave every `dok:` inside the
           frame as unlinked words, which is the state that means "you may not read that". -->
      <figure class="einbettung" data-einbettung={ref.key}>
        <figcaption class="einbettung-kopf">
          Eingebettet aus <a href={zielHref} rel="noopener noreferrer">{ziel.title}</a>{ref.section
            ? ' (ein Abschnitt)'
            : ''}
        </figcaption>
        <div class="einbettung-inhalt">
          {#each ziel.body.content ?? [] as child, i (i)}<Self
              block={child}
              {anhaenge}
              formeln={null}
              fences={null}
              verweise={ziel.references ?? {}}
              einbettungen={{}}
              eingebettet={true}
              quelle={{ path: ziel.path, title: ziel.title }}
            />{/each}
        </div>
        {#if hatAufgaben(ziel.body)}
          <!-- D-30: the boxes above show their LIVE state and cannot be ticked here. One
               task, one record, one board (D-2): the record owns the state, and it belongs to
               the page the line is written on. So the click goes there, and the page says so
               rather than leaving a reader pressing a box that does nothing. -->
          <p class="einbettung-fuss">
            Abgehakt wird auf <a href={zielHref} rel="noopener noreferrer">{ziel.title}</a>.
          </p>
        {/if}
      </figure>
    {/if}
  {/if}
{:else if block.kind === 'text'}
  {@render marked(block.text ?? '', block.marks ?? [])}
{/if}

{#snippet nested(child: Block)}<Self block={child} {anhaenge} {formeln} {fences} {verweise} {einbettungen} {eingebettet} {quelle} />{/snippet}

<!-- A leaf's `marks`, applied outermost first — the order `gw_core::MARK_ORDER` already
     sorted them into (see `render.ts`'s `Block.marks` doc). Recursing one mark at a time
     rather than looping means each kind decides its own tag in one place, and an unknown
     kind (a later milestone's, arriving the same way an unknown block kind does) is skipped
     rather than rendered raw, matching the "unknown kinds render nothing extra" rule the
     block side of this component already follows. -->
{#snippet marked(text: string, marks: Mark[])}
  {#if marks.length === 0}{text}{:else}
    {@const mark = marks[0]}
    {@const rest = marks.slice(1)}
    {#if mark.kind === 'strong'}
      <strong>{@render marked(text, rest)}</strong>
    {:else if mark.kind === 'em'}
      <em>{@render marked(text, rest)}</em>
    {:else if mark.kind === 'code'}
      <code>{@render marked(text, rest)}</code>
    {:else if mark.kind === 'strike'}
      <s>{@render marked(text, rest)}</s>
    {:else if mark.kind === 'link'}
      {@const doc = mark.attrs?.doc}
      <!-- `safeHref` rather than the stored string: an `href` reaches here unvalidated from
           the importer, from the editor's Link control and from anything written later, and
           `javascript:` in it is stored XSS against every reader — see its doc comment for
           why the check belongs at this sink rather than at each of those. `null` means the
           run renders as text, the same fallthrough an unrecognised mark kind takes. -->
      {@const href = safeHref(mark.attrs?.href)}
      {#if typeof doc === 'string'}
        <!-- A reference to another page of this wiki, by identity (D-5), resolved for THIS
             reader by the server. `verweise[doc]` is the whole of what may be said about the
             target: it is there when the reader may read the page, and it is absent for a
             page they may not read, one in the Papierkorb, one that was purged, one that
             never existed and one past the per-page cap — four states that must answer
             identically, because distinguishing them is itself the disclosure.

             **The `doc` value never becomes an address.** It is an arbitrary string:
             `gw-collab`'s `attrs_to_marks` copies whatever the Yjs attribute carries and
             nothing between the collaboration socket and `documents.body` validates it, so
             `{"doc": "javascript:…"}` is a thing a page can hold. What becomes an address is
             the PATH the server resolved, and it goes through `safeHref` like every other
             one — the same sink, not a second judgement of what is safe.

             `title` is the target's CURRENT name. That is D-5's point and it is also why it
             is withheld above: it is live, so a reference written when the reader could see
             the page would otherwise go on reporting that page's new name after a rename.

             `data-doc` stays on both branches. It is neither an address nor a name, and the
             id is in the page's data either way — `+page.server.ts` returns the body — so it
             discloses nothing this reader does not already have. It is what a browser check
             finds the reference by. -->
        {@const ziel = verweise[doc]}
        {@const zielHref = ziel ? safeHref(ziel.path) : null}
        {#if ziel && zielHref !== null}
          <a href={zielHref} title={ziel.title} data-doc={doc} rel="noopener noreferrer"
            >{@render marked(text, rest)}</a
          >
        {:else}
          <span data-doc={doc}>{@render marked(text, rest)}</span>
        {/if}
      {:else if href !== null}
        <!-- `rel="noopener noreferrer"` unconditionally, not only when `target="_blank"` is
             also set: this component never adds a `target`, but the protection costs nothing
             where it is not needed and a future change that adds one must not be the change
             that also has to remember this. -->
        <a {href} rel="noopener noreferrer">{@render marked(text, rest)}</a>
      {:else}
        {@render marked(text, rest)}
      {/if}
    {:else}
      {@render marked(text, rest)}
    {/if}
  {/if}
{/snippet}

<style>
  /* A placed file. The picture fills the column it is in and never widens it — a phone is
     the case that breaks first, and a 4000px scan would otherwise push the whole page
     sideways. `block` because an `<img>` is inline by default and its baseline leaves a
     stripe of background under it that reads as a rendering fault. */
  .datei-bild {
    margin-block: var(--space-4);
  }

  .datei-bild img {
    display: block;
    max-inline-size: 100%;
    block-size: auto;
    border-radius: var(--radius-sm);
  }

  /* The card, shaped like an entry in the `Anhänge` list below rather than like a button:
     it is the same thing in a different place, and two shapes for one act would read as two
     different acts. */
  .datei-karte {
    display: block;
    margin-block: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-inline-start: 3px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--accent);
    text-decoration: none;
  }

  .datei-karte:hover,
  .datei-karte:focus-visible {
    background: var(--accent-soft);
  }

  .datei-name {
    /* A filename is not a sentence and can be long; it wraps rather than being cut off. */
    overflow-wrap: anywhere;
  }

  .datei-fakten {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    margin-block-start: var(--space-1);
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  .datei-typ {
    font-family: var(--font-mono);
    font-size: 0.9em;
  }

  /* Not an error colour and not an alert. A reference whose file is gone is an ordinary,
     recoverable state of a page — somebody detached the file, or it was imported before
     anything was uploaded — and painting it red would make every such page look broken. */
  .datei-fehlt {
    margin-block: var(--space-4);
    padding: var(--space-2) var(--space-3);
    border-inline-start: 3px solid var(--border-strong);
    background: var(--bg-sunken);
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  /* The frame (D-28). A subtle border and a named source, never seamless: a reader has to be
     able to tell which words are this page's own, and an edit made "here" would land on a
     different page under a different ACL. Tokens throughout, so a theme can repaint it. */
  .einbettung {
    margin-block: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-inline-start: 3px solid var(--accent);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
  }

  .einbettung-kopf {
    margin: 0 0 var(--space-2);
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  .einbettung-inhalt > :global(*:first-child) {
    margin-block-start: 0;
  }

  .einbettung-inhalt > :global(*:last-child) {
    margin-block-end: 0;
  }

  .einbettung-fuss {
    margin: var(--space-3) 0 0;
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  /* Not an error colour and not an alert, for `.datei-fehlt`'s reason: a frame whose section
     was renamed away, whose target is not this reader's to see, or which closes a ring is an
     ordinary, recoverable state of a page. Painting it red would make every such page look
     broken. */
  .einbettung-fremd,
  .einbettung-verwaist,
  .einbettung-kreis {
    border-inline-start-color: var(--border-strong);
    background: var(--bg-sunken);
  }

  .einbettung-wort {
    margin: 0;
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  /* A checklist puts its own control where the bullet would be, so the marker itself would
     be a second, meaningless one. Scoped to this component rather than added to `app.css`'s
     `.prose` block: this markup exists nowhere else, and the reader and the editor already
     share the `data-type` hooks TipTap emits if a page-wide rule is ever wanted. */
  .task-list {
    list-style: none;
    padding-inline-start: 0;
  }

  .task-item {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  /* Full opacity despite `disabled`. A greyed-out control reads as "broken" or "not yet
     loaded"; this one is not disabled because something went wrong, but because the page is
     not where a task's state lives (D-2). It should read as a *statement* of state. */
  .task-item > input {
    flex: none;
    accent-color: var(--accent);
    opacity: 1;
  }

  /* The line's own text and anything nested under it, kept out of the checkbox's column. */
  .task-item > div {
    min-width: 0;
  }
</style>
