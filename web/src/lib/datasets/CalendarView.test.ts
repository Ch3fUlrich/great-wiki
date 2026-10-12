import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import CalendarView from './CalendarView.svelte';
import type { DatasetView } from './views';
import type { Field, RowPage } from './table';

const FIELDS: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'start', label: 'Start', kind: 'date', config: {}, position: 1 },
  { key: 'ende', label: 'Ende', kind: 'date', config: {}, position: 2 }
];
const CALENDAR: DatasetView = {
  id: 'c1',
  name: 'Termine',
  kind: 'calendar',
  config: { date: 'start', end: 'ende', title: 'name' },
  position: 0
};

const row = (id: string, values: Record<string, unknown>) => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});
// Date-only values: they are the same calendar day in every zone, so what the page draws does
// not depend on the zone the test run happens to be in. The zone itself is calendar.test.ts's job.
const PAGE: RowPage = {
  rows: [
    row('r1', { name: 'Tagung', start: '2026-03-02' }),
    row('r2', { name: 'Ferien', start: '2026-03-02', ende: '2026-03-04' }),
    row('r3', { name: 'Irgendwas', start: null })
  ],
  total: 3
};

const html = (props: Record<string, unknown> = {}): string =>
  render(CalendarView, {
    props: {
      path: '/d',
      fields: FIELDS,
      page: PAGE,
      view: CALENDAR,
      error: null,
      mayWrite: true,
      month: '2026-03',
      ...props
    }
  })
    .body.replace(/<!--.*?-->/g, '')
    .trim();

const days = (out: string) => [...out.matchAll(/data-day="(\d{4}-\d{2}-\d{2})"/g)].map((m) => m[1]);

describe('CalendarView', () => {
  it('draws a month table whose weekday headers run Monday to Sunday', () => {
    const out = html();
    expect(out).toContain('data-calendar');
    expect(out).toContain('<table');
    // The header cells, as rendered: Svelte puts its scoped-style class on every one of them.
    const kopf = out.slice(0, out.indexOf('<tbody'));
    const spalten = [...kopf.matchAll(/<th scope="col"[^>]*>([^<]+)<\/th>/g)].map((m) => m[1]);
    expect(spalten).toEqual(['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So']);
  });

  it('names the month it is drawing and shows its rows under that name', () => {
    const out = html();
    expect(out).toContain('März 2026');
    expect(out).toContain('Tagung');
  });

  it('is whole weeks padded with the neighbouring days', () => {
    const out = days(html());
    expect(out.length).toBe(42);
    expect(out[0]).toBe('2026-02-23');
    expect(out[out.length - 1]).toBe('2026-04-05');
    expect(out).toContain('2026-03-31');
  });

  it('lists an entry on its day and not on another', () => {
    const out = html();
    const tag = out.indexOf('data-day="2026-03-02"');
    const danach = out.indexOf('data-day="2026-03-05"');
    expect(tag).toBeGreaterThanOrEqual(0);
    const r1 = out.indexOf('data-event="r1"');
    expect(r1).toBeGreaterThan(tag);
    expect(r1).toBeLessThan(danach);
  });

  it('lists a multi-day row on every day of its span and marks only the continuations', () => {
    const out = html();
    expect([...out.matchAll(/data-event="r2"/g)]).toHaveLength(3);
    expect([...out.matchAll(/data-event="r1"/g)]).toHaveLength(1);
    expect([...out.matchAll(/fortgesetzt/g)]).toHaveLength(2);
  });

  it('lists rows without a date below the month instead of losing them', () => {
    const out = html();
    expect(out).toContain('Ohne Datum');
    expect(out.indexOf('data-ohne-datum="r3"')).toBeGreaterThan(out.indexOf('</table>'));
    expect(out).toContain('Irgendwas');
  });

  it('links the month before and after, carrying the view', () => {
    const out = html();
    expect(out).toContain('href="?view=c1&amp;month=2026-02"');
    expect(out).toContain('href="?view=c1&amp;month=2026-04"');
  });

  it('a title from the data reaches the page as text, not as markup', () => {
    const page: RowPage = { rows: [row('r9', { name: '<b>x</b>', start: '2026-03-03' })], total: 1 };
    const out = html({ page });
    // What Svelte's text escaping actually produces: the angle brackets are entities, the
    // characters between them are never re-parsed as markup.
    expect(out).toContain('&lt;b>x&lt;/b>');
    expect(out).not.toContain('<b>x</b>');
  });

  it('a failed read states its error instead of an empty month', () => {
    const out = html({ error: 'Die Tabelle konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('data-calendar');
  });

  it('no rows answered and no error stated is still not an empty month', () => {
    const out = html({ page: null });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-calendar');
  });

  it('a config without a date to place rows by says so instead of drawing a grid', () => {
    const out = html({ view: { ...CALENDAR, config: {} } });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-calendar');
  });

  it('a date field the dataset does not have is stated, not drawn as an empty month', () => {
    const out = html({ view: { ...CALENDAR, config: { date: 'fehlt' } } });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-calendar');
  });

  it('a reader who may not write is told the calendar is read-only', () => {
    const out = html({ mayWrite: false });
    expect(out).toContain('data-calendar');
    expect(out).toContain('role="status"');
    expect(out).toContain('Schreibrecht');
  });

  it('a month that is not a month falls back to drawing one, not to breaking', () => {
    const out = html({ month: 'irgendwas' });
    expect(out).toContain('data-calendar');
    expect(days(out).length % 7).toBe(0);
    expect(days(out).length).toBeGreaterThan(0);
  });
});
