/**
 * The pure half of a dataset page's saved GALLERY view (M8d D4b): which of a row's file
 * attachments may be drawn as a picture, and what a card is captioned with.
 *
 * A cover is only ever made from the page's attachment LISTING (`GET /api/attachments/{path}`):
 * the resolved file cell names a filename, the listing says what that file IS (`media_type`,
 * sniffed from the bytes by `gw_store::blobs::sniff`) and where it is fetched (`href`, used
 * verbatim — D-16, and `web/src/lib/attachments.ts` states why no client assembles one). A
 * cell whose file is gone from the listing — trashed, detached — has no address to draw.
 *
 * Pure — no DOM, no fetch.
 */
import type { Cell, Field, Row } from './table';
import { cellOf } from './table';
import { rowTitle } from './calendar';

/**
 * The media types a card may carry an `<img>` for: the RASTER image types of
 * `gw_store::blobs::sniff`'s allowlist that every browser draws in an `<img>`. Not a copy of
 * everything the server serves inline — `image/svg+xml` is an image by media type and a
 * program by capability (ADR 0014 refuses it inline for that reason), `application/pdf` is a
 * document, not a cover, and `image/tiff` is an attachable image that Chrome and Firefox do
 * not draw: a TIFF cover would be a broken picture, so it gets the placeholder, like a file
 * that is gone. If the sniff allowlist ever gains a raster type, this list is checked
 * against it, not assumed.
 */
export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];

export interface GalleryConfig {
  cover: string;
  title?: string | null;
  card_fields: string[];
}

/** One listing entry, as far as a gallery cares: what the file is and where it is fetched. */
export interface GalleryAttachment {
  filename: string;
  media_type: string;
  href: string;
}

export type GalleryCover =
  | { kind: 'image'; src: string; filename: string }
  | { kind: 'placeholder'; reason: 'none' | 'missing' | 'not-image' };

/** One card: the row, its caption text, the alt/aria text, the cover, and the fields under it. */
export interface GalleryEntry {
  row: Row;
  title: string;
  alt: string;
  cover: GalleryCover;
  extras: { key: string; label: string; cell: Cell }[];
}

/**
 * The cover of one row: the file cell named by `coverKey`, read against the page's attachment
 * listing. Every way this is NOT a picture is said as a reason, because the view draws the
 * reason rather than an empty box.
 */
export function coverOf(row: Row, coverKey: string, attachments: GalleryAttachment[]): GalleryCover {
  const placeholder = (reason: 'none' | 'missing' | 'not-image'): GalleryCover => ({
    kind: 'placeholder',
    reason
  });
  const cell = row.values?.[coverKey];
  if (cell === null || cell === undefined) return placeholder('none');
  if (typeof cell !== 'object' || Array.isArray(cell)) return placeholder('none');
  const file = cell as { filename?: unknown; missing?: unknown };
  if (file.missing === true) return placeholder('missing');
  if (typeof file.filename !== 'string') return placeholder('none');
  const listed = attachments.find((a) => a.filename === file.filename);
  if (!listed) return placeholder('missing');
  if (!IMAGE_TYPES.includes(listed.media_type)) return placeholder('not-image');
  return { kind: 'image', src: listed.href, filename: listed.filename };
}

/**
 * The text a card speaks its picture with: the row's caption — the titled column, else the
 * first text column, else the row id, exactly the calendar's rule — and NEVER the filename.
 * A picture of a scan called `Röntgen – links.png` describes the row, not the file on disk.
 */
export function altOf(row: Row, titleKey: string | null | undefined, fields: Field[]): string {
  return rowTitle(row, fields, titleKey);
}

/**
 * The cards of a gallery, in row order: caption, alt, cover, and whatever `card_fields` names
 * of the dataset's columns as cell DATA (never markup — `TableView`'s rule). A card field the
 * dataset no longer has is skipped; the server refused it at save time, so this only covers
 * columns deleted after the view was saved.
 */
export function galleryEntries(
  rows: Row[],
  fields: Field[],
  config: GalleryConfig,
  attachments: GalleryAttachment[]
): GalleryEntry[] {
  const titleKey = typeof config.title === 'string' && config.title !== '' ? config.title : null;
  const cardFields = config.card_fields
    .map((key) => fields.find((f) => f.key === key))
    .filter((f): f is Field => f !== undefined);
  return rows.map((row) => {
    const title = altOf(row, titleKey, fields);
    return {
      row,
      title,
      alt: title,
      cover: coverOf(row, config.cover, attachments),
      extras: cardFields.map((field) => ({
        key: field.key,
        label: field.label,
        cell: cellOf(field, row.values?.[field.key])
      }))
    };
  });
}
