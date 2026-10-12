import { describe, expect, it } from 'vitest';
import { IMAGE_TYPES, altOf, coverOf, galleryEntries, type GalleryAttachment } from './gallery';
import type { Field, Row } from './table';

const row = (values: Record<string, unknown>, id = 'r1'): Row => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});

const LISTING: GalleryAttachment[] = [
  { filename: 'plan.png', media_type: 'image/png', href: '/api/attachment/plan.png/d' },
  { filename: 'logo.svg', media_type: 'image/svg+xml', href: '/api/attachment/logo.svg/d' },
  { filename: 'bericht.pdf', media_type: 'application/pdf', href: '/api/attachment/bericht.pdf/d' },
  { filename: 'rntgen.tiff', media_type: 'image/tiff', href: '/api/attachment/rntgen.tiff/d' }
];

const fileCell = (filename: string) => ({ id: 'f1', filename });

describe('coverOf', () => {
  it('a png in the listing is an image drawn from the href the API made', () => {
    expect(coverOf(row({ bild: fileCell('plan.png') }), 'bild', LISTING)).toEqual({
      kind: 'image',
      src: '/api/attachment/plan.png/d',
      filename: 'plan.png'
    });
  });

  it('an svg never becomes a cover, whatever the media type claims (ADR 0014)', () => {
    expect(coverOf(row({ bild: fileCell('logo.svg') }), 'bild', LISTING)).toEqual({
      kind: 'placeholder',
      reason: 'not-image'
    });
  });

  it('a pdf is a document, not a picture', () => {
    expect(coverOf(row({ bild: fileCell('bericht.pdf') }), 'bild', LISTING)).toEqual({
      kind: 'placeholder',
      reason: 'not-image'
    });
  });

  it('a tiff is an attachable image no browser draws everywhere: it gets the placeholder, not a broken picture', () => {
    expect(coverOf(row({ bild: fileCell('rntgen.tiff') }), 'bild', LISTING)).toEqual({
      kind: 'placeholder',
      reason: 'not-image'
    });
  });

  it('a cell the server resolved to missing is missing', () => {
    expect(
      coverOf(row({ bild: { id: 'f1', missing: true } }), 'bild', LISTING)
    ).toEqual({ kind: 'placeholder', reason: 'missing' });
  });

  it('a filename gone from the listing — trashed or detached since the row was written — is missing', () => {
    expect(coverOf(row({ bild: fileCell('vergessen.png') }), 'bild', LISTING)).toEqual({
      kind: 'placeholder',
      reason: 'missing'
    });
  });

  it('an empty or absent cell covers nothing', () => {
    expect(coverOf(row({ bild: null }), 'bild', LISTING)).toEqual({
      kind: 'placeholder',
      reason: 'none'
    });
    expect(coverOf(row({}), 'bild', LISTING)).toEqual({ kind: 'placeholder', reason: 'none' });
  });

  it('IMAGE_TYPES is the raster set every browser draws in an img: png, jpeg, gif, webp', () => {
    // The server's sniffer (crates/gw-store/src/blobs.rs) also attaches tiff, which Chrome and
    // Firefox do not draw; svg is an exception checked BEFORE the image rule, and everything
    // else the sniffer knows (pdf, video, audio, zip) is not a picture.
    expect(IMAGE_TYPES).toEqual(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);
  });
});

const FIELDS: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'bild', label: 'Bild', kind: 'file', config: {}, position: 1 },
  { key: 'ort', label: 'Ort', kind: 'text', config: {}, position: 2 }
];

describe('altOf', () => {
  it('says the row title, never the filename', () => {
    const zeile = row({ name: 'Fundstelle Nord', bild: fileCell('plan.png') });
    expect(altOf(zeile, 'name', FIELDS)).toBe('Fundstelle Nord');
    expect(altOf(zeile, 'name', FIELDS)).not.toContain('plan.png');
  });

  it('falls back to the first text column by position, then to the row id', () => {
    const ohneTitel = row({ ort: 'Magazin A', bild: fileCell('plan.png') }, 'r7');
    expect(altOf(ohneTitel, null, FIELDS)).toBe('Magazin A');
    const ganzOhne = row({ bild: fileCell('plan.png') }, 'r8');
    expect(altOf(ganzOhne, 'name', FIELDS)).toBe('r8');
  });
});

describe('galleryEntries', () => {
  it('builds one card per row: caption, alt, cover and the card fields as cell text', () => {
    const config = { cover: 'bild', title: 'name', card_fields: ['ort', 'verschwunden'] };
    const out = galleryEntries(
      [row({ name: 'Fundstelle Nord', bild: fileCell('plan.png'), ort: 'Magazin A' }, 'r1')],
      FIELDS,
      config,
      LISTING
    );
    expect(out).toHaveLength(1);
    const card = out[0];
    expect(card.title).toBe('Fundstelle Nord');
    expect(card.alt).toBe('Fundstelle Nord');
    expect(card.cover).toEqual({ kind: 'image', src: '/api/attachment/plan.png/d', filename: 'plan.png' });
    // The column the dataset no longer has is skipped; what it has arrives as data.
    expect(card.extras).toEqual([
      { key: 'ort', label: 'Ort', cell: { kind: 'text', text: 'Magazin A' } }
    ]);
  });

  it('keeps rows in the order they came and draws no card from a missing file', () => {
    const config = { cover: 'bild', title: null, card_fields: [] };
    const out = galleryEntries(
      [row({ name: 'Zweite', bild: null }, 'r2'), row({ name: 'Erste' }, 'r1')],
      FIELDS,
      config,
      LISTING
    );
    expect(out.map((e) => e.row.id)).toEqual(['r2', 'r1']);
    expect(out[0].cover).toEqual({ kind: 'placeholder', reason: 'none' });
    expect(out[1].cover).toEqual({ kind: 'placeholder', reason: 'none' });
    expect(out[0].title).toBe('Zweite');
  });
});
