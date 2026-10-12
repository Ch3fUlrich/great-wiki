<script lang="ts">
  /**
   * A dataset page's saved GALLERY view (M8d D4b): the same rows the table shows, one card
   * each, covered by the file the view names — and only by files the page's attachment listing
   * says really are pictures.
   *
   * A failed read, a missing page or a config without a cover is STATED in a role=alert, never
   * drawn as an empty gallery. Titles, alts and card fields reach the page as TEXT: nothing
   * here parses markup, and a cover is fetched through the `href` the API made (D-16 — no
   * address is assembled here).
   *
   * `path` is carried for the shape of props the page hands every view; a gallery writes
   * nothing, so it has no use for it.
   */
  import { galleryEntries, type GalleryAttachment, type GalleryEntry } from './gallery';
  import type { Cell, Field, RowPage } from './table';
  import { galleryConfig, type DatasetView } from './views';

  let {
    path,
    fields,
    page,
    view,
    attachments = [],
    error = null
  }: {
    path: string;
    fields: Field[];
    page: RowPage | null;
    view: DatasetView | null;
    attachments?: GalleryAttachment[];
    error?: string | null;
  } = $props();

  /** What a cover that is not a picture says instead, per the reason `coverOf` gave. */
  const PLATZHALTER: Record<'none' | 'missing' | 'not-image', string> = {
    none: 'Kein Bild',
    missing: 'Datei fehlt',
    'not-image': 'Keine Bilddatei'
  };

  /** What there is to draw when there is nothing to draw. Never shown: `fehler` covers it. */
  const KEINE_KARTEN: GalleryEntry[] = [];

  /** The one text a cell of any shape can be drawn with — the card fields are text, not columns. */
  function text(cell: Cell): string {
    if (cell.kind === 'chips') return cell.items.join(', ');
    if (cell.kind === 'error') return cell.reason;
    return cell.text;
  }

  /**
   * One read of the rows and the listing, decided once: either the reason no gallery is drawn,
   * or the cards to draw.
   */
  const gebaut = $derived.by(
    (): { message: string } | { eintraege: GalleryEntry[] } => {
      if (error) return { message: error };
      if (!page) return { message: 'Die Zeilen der Galerie konnten nicht geladen werden.' };
      const config = galleryConfig(view);
      if (!config) return { message: 'Diese Galerie hat kein Feld, das ihre Karten bedecken könnte.' };
      return { eintraege: galleryEntries(page.rows, fields, config, attachments) };
    }
  );

  const fehler = $derived('message' in gebaut ? gebaut.message : null);
  const eintraege = $derived('message' in gebaut ? KEINE_KARTEN : gebaut.eintraege);
</script>

{#if fehler !== null}
  <p class="galerie-fehler" role="alert">{fehler}</p>
{:else}
  <section class="galerie" data-gallery aria-label="Galerie">
    {#each eintraege as karte (karte.row.id)}
      <figure class="galerie-karte" data-card={karte.row.id}>
        {#if karte.cover.kind === 'image'}
          <img class="galerie-bild" src={karte.cover.src} alt={karte.alt} loading="lazy" />
        {:else}
          <div class="galerie-platzhalter" data-placeholder={karte.cover.reason}>
            <span>{PLATZHALTER[karte.cover.reason]}</span>
          </div>
        {/if}
        <figcaption class="galerie-caption">
          <span class="galerie-titel">{karte.title}</span>
          {#each karte.extras as extra (extra.key)}
            {@const wert = text(extra.cell)}
            {#if wert !== ''}
              <span class="galerie-feld"
                ><span class="galerie-feld-label">{extra.label}</span> {wert}</span
              >
            {/if}
          {/each}
        </figcaption>
      </figure>
    {/each}
    {#if eintraege.length === 0}
      <p class="galerie-leer">Diese Galerie hat keine Zeilen.</p>
    {/if}
  </section>
{/if}

<style>
  @layer components {
    .galerie {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
      gap: var(--space-4);
      margin-block: var(--space-4);
    }
    .galerie-karte {
      margin: 0;
      border: 1px solid var(--border);
      border-radius: var(--radius);
      background: var(--bg-raised);
      overflow: hidden;
      display: flex;
      flex-direction: column;
    }
    .galerie-bild {
      inline-size: 100%;
      block-size: 9rem;
      object-fit: cover;
      display: block;
    }
    .galerie-platzhalter {
      block-size: 9rem;
      display: flex;
      align-items: center;
      justify-content: center;
      border-block-end: 1px dashed var(--border);
      color: var(--ink-faint);
      font-size: var(--text-sm);
    }
    .galerie-caption {
      display: flex;
      flex-direction: column;
      gap: var(--space-1);
      padding: var(--space-2) var(--space-3) var(--space-3);
      overflow-wrap: anywhere;
    }
    .galerie-titel {
      font-weight: 650;
      font-size: var(--text-sm);
    }
    .galerie-feld {
      font-size: var(--text-xs);
      color: var(--ink-muted);
    }
    .galerie-feld-label {
      color: var(--ink-faint);
      font-weight: 600;
    }
    .galerie-leer {
      font-size: var(--text-sm);
      color: var(--ink-muted);
    }
    .galerie-fehler {
      color: var(--danger, inherit);
      margin-block: var(--space-4);
    }
  }
</style>
