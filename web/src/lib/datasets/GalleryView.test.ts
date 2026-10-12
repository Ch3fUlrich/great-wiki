import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import GalleryView from './GalleryView.svelte';
import type { DatasetView } from './views';
import type { Field, RowPage } from './table';
import type { GalleryAttachment } from './gallery';

const FIELDS: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'bild', label: 'Bild', kind: 'file', config: {}, position: 1 },
  { key: 'ort', label: 'Ort', kind: 'text', config: {}, position: 2 }
];
const GALERIE: DatasetView = {
  id: 'g1',
  name: 'Galerie',
  kind: 'gallery',
  config: { cover: 'bild', title: 'name', card_fields: ['ort'] },
  position: 0
};

const LISTING: GalleryAttachment[] = [
  { filename: 'plan.png', media_type: 'image/png', href: '/api/attachment/plan.png/d' },
  { filename: 'rntgen.tiff', media_type: 'image/tiff', href: '/api/attachment/rntgen.tiff/d' },
  { filename: 'logo.svg', media_type: 'image/svg+xml', href: '/api/attachment/logo.svg/d' },
  { filename: 'bericht.pdf', media_type: 'application/pdf', href: '/api/attachment/bericht.pdf/d' }
];

const row = (id: string, values: Record<string, unknown>) => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});

const PAGE: RowPage = {
  rows: [
    row('r1', { name: 'Fundstelle Nord', bild: { id: 'f1', filename: 'plan.png' }, ort: 'Magazin A' }),
    row('r2', { name: 'Altes Protokoll', bild: { id: 'f2', missing: true } }),
    row('r3', { name: 'Vertippt', bild: { id: 'f3', filename: 'weg.png' } }),
    row('r4', { name: 'Diagramm', bild: { id: 'f4', filename: 'logo.svg' } }),
    row('r5', { name: 'Ohne Bild', bild: null })
  ],
  total: 5
};

const html = (props: Record<string, unknown> = {}): string =>
  render(GalleryView, {
    props: {
      path: '/d',
      fields: FIELDS,
      page: PAGE,
      view: GALERIE,
      attachments: LISTING,
      error: null,
      ...props
    }
  })
    .body.replace(/<!--.*?-->/g, '')
    .trim();

const cards = (out: string) => [...out.matchAll(/data-card="([^"]+)"/g)].map((m) => m[1]);
const card = (out: string, id: string): string => {
  const start = out.indexOf(`data-card="${id}"`);
  const next = out.indexOf('data-card="', start + 1);
  return next === -1 ? out.slice(start) : out.slice(start, next);
};

describe('GalleryView', () => {
  it('draws one figure per row, in order', () => {
    expect(cards(html())).toEqual(['r1', 'r2', 'r3', 'r4', 'r5']);
  });

  it('an image cover is an <img> whose alt is the row title, never the filename', () => {
    const r1 = card(html(), 'r1');
    expect(r1).toMatch(/<img[^>]*src="\/api\/attachment\/plan\.png\/d"/);
    expect(r1).toMatch(/<img[^>]*alt="Fundstelle Nord"/);
    expect(r1).not.toMatch(/alt="plan\.png"/);
    expect(r1).toContain('loading="lazy"');
  });

  it('a file trashed since the row was written — missing flag or gone from the listing — is a stated placeholder, not an <img>', () => {
    const out = html();
    for (const id of ['r2', 'r3']) {
      expect(card(out, id)).not.toContain('<img');
      expect(card(out, id)).toContain('Datei fehlt');
    }
  });

  it('an svg cover is a placeholder, never an <img> (ADR 0014)', () => {
    const r4 = card(html(), 'r4');
    expect(r4).not.toContain('<img');
    expect(r4).toContain('Keine Bilddatei');
  });

  it('a row with no file says so', () => {
    const r5 = card(html(), 'r5');
    expect(r5).not.toContain('<img');
    expect(r5).toContain('Kein Bild');
  });

  it('names the card and puts the card fields under it as text', () => {
    const r1 = card(html(), 'r1');
    expect(r1).toContain('<figcaption');
    expect(r1).toContain('Fundstelle Nord');
    expect(r1).toContain('Ort');
    expect(r1).toContain('Magazin A');
  });

  it('renders a title as escaped text, never as markup', () => {
    const out = html({
      page: { rows: [row('x', { name: '<b>x</b>', bild: null })], total: 1 }
    });
    expect(out).toContain('&lt;b>x&lt;/b>');
    expect(out).not.toContain('<b>x</b>');
  });

  it('states a failed read in an alert instead of drawing an empty gallery', () => {
    const out = html({ error: 'Die Tabelle konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('data-card');
  });

  it('states a gallery without a usable cover configuration', () => {
    const out = html({ view: { ...GALERIE, config: { title: 'name' } } });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-card');
  });

  it('states a missing rows page in an alert', () => {
    const out = html({ page: null });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-card');
  });
});
