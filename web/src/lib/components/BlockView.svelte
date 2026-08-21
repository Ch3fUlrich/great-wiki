<script lang="ts">
  import type { Block, Mark } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { plainText, safeHref } from '$lib/blocks/render';
  import { alignOf } from '$lib/blocks/table';
  import Self from './BlockView.svelte';
  import TableView from './TableView.svelte';

  let { block }: { block: Block } = $props();
</script>

<!-- Only known kinds render. An unknown block is skipped rather than emitted raw, which
     is why there is no sanitisation step here: no untrusted HTML is ever constructed. -->
{#if block.kind === 'doc'}
  {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
{:else if block.kind === 'paragraph'}
  <p>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</p>
{:else if block.kind === 'heading'}
  {@const level = Math.min(6, Math.max(1, Number(block.attrs?.level ?? 1)))}
  {@const id = slugify(plainText(block))}
  <svelte:element this={`h${level}`} {id}>
    {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
  </svelte:element>
{:else if block.kind === 'bulletList'}
  <ul>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ul>
{:else if block.kind === 'orderedList'}
  <ol>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ol>
{:else if block.kind === 'listItem'}
  <li>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</li>
{:else if block.kind === 'taskList'}
  <!-- A checklist. `data-type` is the attribute TipTap's own `TaskList` puts on its `<ul>`,
       so the editor and the reader can be styled by one rule instead of two that drift. -->
  <ul class="task-list" data-type="taskList">
    {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
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
    <div>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</div>
  </li>
{:else if block.kind === 'blockquote'}
  <blockquote>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</blockquote>
{:else if block.kind === 'codeBlock'}
  <pre><code>{plainText(block)}</code></pre>
{:else if block.kind === 'table'}
  <!-- TableView owns the scroll box, the sticky header and — once it has mounted in a
       browser — sorting and filtering. It renders no cell content itself: `nested` hands
       the recursion below straight back to it, so there is exactly one block renderer and
       no import cycle between the two components. -->
  <TableView {block} child={nested} />
{:else if block.kind === 'tableRow'}
  <tr>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</tr>
{:else if block.kind === 'tableHeader'}
  <th scope="col" style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</th
  >
{:else if block.kind === 'tableCell'}
  <td style:text-align={alignOf(block)}
    >{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</td
  >
{:else if block.kind === 'text'}
  {@render marked(block.text ?? '', block.marks ?? [])}
{/if}

{#snippet nested(child: Block)}<Self block={child} />{/snippet}

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
