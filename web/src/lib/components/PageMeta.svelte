<!--
  The facts about a page that the API can actually support today.

  ORDER IS THE ARGUMENT HERE. Visibility leads, because this wiki answers on the public
  internet and every page in the imported corpus is `restricted`: a reader deciding whether
  to put something on a page needs to know who will be able to read it, and needs to know
  it before the body rather than after. Language and document type follow, and both are
  silent when they have nothing to say — a row reading "Sprache: Deutsch" on a German page
  in a German interface is noise that trains people to stop reading the panel.

  "ZULETZT BEARBEITET" IS A SLOT, NOT A GAP. There is no revisions endpoint yet, so the row
  is written, styled and tested but never rendered: `revision` is optional and nothing
  passes it. It sits LAST on purpose, so that landing it appends a row instead of pushing
  the visibility statement down the panel. What it expects is `RevisionInfo` from
  $lib/pagemeta — an RFC 3339 instant, a display name, and optionally a revision number.

  What it deliberately does NOT do is show a placeholder. A dash, an "unbekannt" or a
  today's-date fallback in a panel whose other rows are facts would be read as a fact.
-->
<script lang="ts">
  import {
    documentTypeLabel,
    editedAtLabel,
    foreignLanguage,
    visibilityStatement,
    type RevisionInfo
  } from '$lib/pagemeta';

  interface Props {
    /** `StoredDocument.visibility` — `public`, `internal` or `restricted`. */
    visibility: string;
    /** `StoredDocument.language` — a BCP-47 tag, `de` and `en` in this corpus. */
    language: string;
    /** `StoredDocument.doc_type` — `page` for nearly everything. */
    docType: string;
    /** The current revision, once an endpoint exists to report one. See the note above. */
    revision?: RevisionInfo;
  }

  let { visibility, language, docType, revision }: Props = $props();

  const access = $derived(visibilityStatement(visibility));
  const foreign = $derived(foreignLanguage(language));
  const type = $derived(documentTypeLabel(docType));
  const editedAt = $derived(revision ? editedAtLabel(revision.edited_at) : null);
</script>

<aside class="pagemeta" aria-label="Angaben zu dieser Seite">
  <dl>
    <dt>Sichtbarkeit</dt>
    <dd>
      <!-- `class:` rather than an interpolated class name: the compiler can see a
           directive and cannot see `chip--{tone}`, so an interpolated modifier is pruned
           as an unused selector and the chip silently loses its styling.

           `data-visibility` carries all three states even though only one of them is
           styled here, so a theme can distinguish internal from restricted without this
           component shipping two rules it does not use. -->
      <span class="chip" class:chip--public={access.tone === 'public'} data-visibility={access.tone}>
        {access.label}
      </span>
      <span class="detail">{access.detail}</span>
    </dd>

    {#if foreign}
      <dt>Sprache</dt>
      <dd>{foreign}</dd>
    {/if}

    {#if type}
      <dt>Dokumentart</dt>
      <dd>{type}</dd>
    {/if}

    {#if revision && editedAt}
      <dt>Zuletzt bearbeitet</dt>
      <dd>
        <time datetime={revision.edited_at}>{editedAt}</time>
        {#if revision.edited_by}<span class="by">von {revision.edited_by}</span>{/if}
        {#if revision.number}<span class="rev">Fassung {revision.number}</span>{/if}
      </dd>
    {/if}
  </dl>
</aside>

<style>
  /* `@layer components`, the plugin contract (ADR 0005). See Breadcrumb.svelte. */
  @layer components {
    .pagemeta {
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--bg-raised);
      padding: var(--space-3) var(--space-4);
      font-size: var(--text-sm);
    }

    /* A two-column grid on the `dl` itself, with `dt`/`dd` as its direct children rather
       than wrapped in rows: a grid can only align terms across rows if the terms are
       actually siblings in the same grid. The row wrappers that a `dl` often gets would
       each be their own formatting context, and the values would step in and out. */
    dl {
      display: grid;
      grid-template-columns: max-content minmax(0, 1fr);
      gap: var(--space-1) var(--space-4);
      margin: 0;
    }

    dt {
      color: var(--ink-faint);
      font-size: var(--text-xs);
      font-weight: 650;
      text-transform: uppercase;
      letter-spacing: 0.06em;
      /* The label is set smaller than its value, so aligning the two on their baselines
         is what keeps the row from looking like two unrelated lines. */
      align-self: baseline;
      padding-block-start: 0.25em;
    }

    dd {
      margin: 0;
      display: flex;
      flex-wrap: wrap;
      align-items: baseline;
      gap: var(--space-1) var(--space-2);
    }

    .chip {
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--bg-sunken);
      padding: 0 var(--space-2);
      font-weight: 650;
      white-space: nowrap;
    }

    /* Two visual states, not three, and that is the point: the question a reader has is
       "can strangers read this?", and a traffic light of three tints would answer a
       question nobody asked while making the one that matters just another colour.
       Internal and restricted differ in words, which is where the difference lives.

       The tint is mixed from `--warn` rather than written as a hex, so a theme that
       repaints the warning colour repaints this with it — the same construction as the
       view-as banner, and for the same reason: this is the state to notice.

       AFTER `.chip`, not before. Both selectors are (0,1,0), so source order is the only
       thing deciding which wins; a modifier written above its base silently does nothing.
       That is the same trap Dialog.svelte documents, arrived at from the other side. */
    .chip--public {
      border-color: var(--warn);
      background: color-mix(in srgb, var(--warn) 16%, var(--bg));
    }

    .detail,
    .by,
    .rev {
      color: var(--ink-muted);
    }

    .rev {
      font-variant-numeric: tabular-nums;
    }

    /* Below the measure the label column stops earning its width — "Sichtbarkeit" and a
       full sentence in the remaining space wrap into a column two words wide. */
    @media (max-width: 34rem) {
      dl {
        grid-template-columns: minmax(0, 1fr);
        gap: 0;
      }

      dd + dt {
        margin-block-start: var(--space-2);
      }
    }
  }
</style>
