import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ViewBar from './ViewBar.svelte';
import type { DatasetView } from './views';

const VIEWS: DatasetView[] = [
  { id: 'b1', name: 'Status', kind: 'board', config: { group_by: 'status' }, position: 0 },
  { id: 't2', name: 'Fällig', kind: 'table', config: {}, position: 1 }
];

const html = (views: DatasetView[], active: string | null): string =>
  render(ViewBar, { props: { views, active } })
    .body.replace(/<!--.*?-->/g, '')
    .trim();

describe('ViewBar', () => {
  it('links the table and every view, named as text', () => {
    const out = html(VIEWS, null);
    expect(out).toContain('aria-label="Ansichten"');
    expect(out).toMatch(/<a [^>]*href="\?"/);
    expect(out).toContain('Tabelle');
    expect(out).toMatch(/<a [^>]*href="\?view=b1"/);
    expect(out).toContain('Status');
    expect(out).toMatch(/<a [^>]*href="\?view=t2"/);
    expect(out).toContain('Fällig');
    expect(out.match(/<a /g)).toHaveLength(3);
  });

  it('marks the table current when no view is picked', () => {
    const out = html(VIEWS, null);
    expect(out).toMatch(/<a [^>]*aria-current="page"[^>]*>Tabelle<\/a>/);
    expect(out.match(/aria-current/g)).toHaveLength(1);
  });

  it('marks the picked view current and not the table', () => {
    const out = html(VIEWS, 'b1');
    expect(out).toMatch(/<a [^>]*aria-current="page"[^>]*>Status<\/a>/);
    expect(out).toMatch(/<a [^>]*href="\?"[^>]*>Tabelle<\/a>/);
    expect(out).not.toMatch(/<a [^>]*href="\?"[^>]*aria-current/);
    expect(out.match(/aria-current/g)).toHaveLength(1);
  });

  it('renders a view name as escaped text, never as markup', () => {
    const out = html([{ id: 'x', name: '<b>x</b>', kind: 'board', config: {}, position: 0 }], null);
    expect(out).toContain('&lt;b>x&lt;/b>');
    expect(out).not.toContain('<b>x</b>');
  });

  it('lists a calendar view by name and links it by id', () => {
    const out = html(
      [{ id: 'c1', name: 'Termine', kind: 'calendar', config: { date: 'start' }, position: 0 }],
      'c1'
    );
    expect(out).toMatch(/<a [^>]*href="\?view=c1"/);
    expect(out).toContain('Termine');
    expect(out).toMatch(/<a [^>]*aria-current="page"[^>]*>Termine<\/a>/);
  });

  it('lists a gallery view by name and links it by id', () => {
    const out = html(
      [{ id: 'g1', name: 'Fotos', kind: 'gallery', config: { cover: 'bild' }, position: 0 }],
      'g1'
    );
    expect(out).toMatch(/<a [^>]*href="\?view=g1"/);
    expect(out).toContain('Fotos');
    expect(out).toMatch(/<a [^>]*aria-current="page"[^>]*>Fotos<\/a>/);
  });

  it('renders nothing when the dataset has no saved views', () => {
    const out = html([], null);
    expect(out).not.toContain('<nav');
    expect(out).not.toContain('Tabelle');
  });
});
