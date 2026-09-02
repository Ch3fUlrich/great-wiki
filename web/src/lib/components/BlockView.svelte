<script lang="ts">
  import type { Block, Mark } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { placedFile, plainText, safeHref } from '$lib/blocks/render';
  import { alignOf } from '$lib/blocks/table';
  import {
    attachmentNamed,
    describeMissingPlacement,
    isPicture,
    kindText,
    sizeText,
    type Attachment
  } from '$lib/attachments';
  import Self from './BlockView.svelte';
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
  }

  let { block, anhaenge = [] }: Props = $props();
</script>

<!-- Only known kinds render. An unknown block is skipped rather than emitted raw, which
     is why there is no sanitisation step here: no untrusted HTML is ever constructed. -->
{#if block.kind === 'doc'}
  {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}
{:else if block.kind === 'paragraph'}
  <p>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</p>
{:else if block.kind === 'heading'}
  {@const level = Math.min(6, Math.max(1, Number(block.attrs?.level ?? 1)))}
  {@const id = slugify(plainText(block))}
  <svelte:element this={`h${level}`} {id}>
    {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}
  </svelte:element>
{:else if block.kind === 'bulletList'}
  <ul>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</ul>
{:else if block.kind === 'orderedList'}
  <ol>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</ol>
{:else if block.kind === 'listItem'}
  <li>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</li>
{:else if block.kind === 'taskList'}
  <!-- A checklist. `data-type` is the attribute TipTap's own `TaskList` puts on its `<ul>`,
       so the editor and the reader can be styled by one rule instead of two that drift. -->
  <ul class="task-list" data-type="taskList">
    {#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}
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
    <div>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</div>
  </li>
{:else if block.kind === 'blockquote'}
  <blockquote>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</blockquote>
{:else if block.kind === 'codeBlock'}
  <pre><code>{plainText(block)}</code></pre>
{:else if block.kind === 'table'}
  <!-- TableView owns the scroll box, the sticky header and — once it has mounted in a
       browser — sorting and filtering. It renders no cell content itself: `nested` hands
       the recursion below straight back to it, so there is exactly one block renderer and
       no import cycle between the two components. -->
  <TableView {block} child={nested} />
{:else if block.kind === 'tableRow'}
  <tr>{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</tr>
{:else if block.kind === 'tableHeader'}
  <th scope="col" style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</th
  >
{:else if block.kind === 'tableCell'}
  <td style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} {anhaenge} />{/each}</td
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
    {@const anhang = attachmentNamed(anhaenge, placed.filename)}
    {#if anhang === null}
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
{:else if block.kind === 'text'}
  {@render marked(block.text ?? '', block.marks ?? [])}
{/if}

{#snippet nested(child: Block)}<Self block={child} {anhaenge} />{/snippet}

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
        <!-- Internal target, not yet resolved to a path — Task 7's job. A real `<a href>`
             needs that resolution and an `<a>` with no `href` reads as broken, so this is
             neither: the text and the target id are both here, nothing is clickable, and
             nothing claims to navigate anywhere until it actually can. -->
        <span data-doc={doc}>{@render marked(text, rest)}</span>
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
