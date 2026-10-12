import { describe, expect, it } from 'vitest';
import {
  bucketRows,
  monthGrid,
  monthHref,
  monthLabel,
  parseMonth,
  rowTitle,
  shiftMonth,
  type CalendarConfig
} from './calendar';
import type { Field, Row } from './table';

const FIELDS: Field[] = [
  { key: 'notiz', label: 'Notiz', kind: 'text', config: {}, position: 3 },
  { key: 'zahl', label: 'Zahl', kind: 'number', config: {}, position: 4 },
  { key: 'start', label: 'Start', kind: 'date', config: {}, position: 0 },
  { key: 'ende', label: 'Ende', kind: 'date', config: {}, position: 1 },
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 2 }
];
const CFG: CalendarConfig = { date: 'start', end: 'ende', title: 'name' };

const row = (id: string, values: Record<string, unknown>): Row => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});

/** The ids listed on one day, in bucket order. */
const on = (days: Map<string, { row: Row }[]>, day: string): string[] =>
  (days.get(day) ?? []).map((e) => e.row.id);

const Berlin = 'Europe/Berlin';
const LosAngeles = 'America/Los_Angeles';

describe('bucketRows — a date-only value', () => {
  it('is the same calendar day in every zone', () => {
    for (const tz of [Berlin, LosAngeles, 'UTC', 'Pacific/Kiritimati']) {
      const { days, undated } = bucketRows([row('r1', { start: '2026-03-01' })], FIELDS, CFG, tz);
      expect([...days.keys()], tz).toEqual(['2026-03-01']);
      expect(undated).toEqual([]);
    }
  });
});

describe('bucketRows — an instant', () => {
  it('lands on the local day the zone says, not on the UTC one', () => {
    const value = '2026-03-01T23:30:00Z';
    expect([...bucketRows([row('r1', { start: value })], FIELDS, CFG, Berlin).days.keys()]).toEqual([
      '2026-03-02'
    ]);
    expect([
      ...bucketRows([row('r1', { start: value })], FIELDS, CFG, LosAngeles).days.keys()
    ]).toEqual(['2026-03-01']);
  });

  it('crosses a DST switch onto the right local day', () => {
    // 2026-03-29 is the Monday the European switch happens on (02:00 → 03:00).
    const cases: [string, string][] = [
      ['2026-03-28T22:30:00Z', '2026-03-28'], // 23:30 still CET
      ['2026-03-28T23:30:00Z', '2026-03-29'], // 00:30, one minute past midnight
      ['2026-03-29T01:30:00Z', '2026-03-29'], // 02:30, inside the skipped hour
      ['2026-03-29T02:30:00Z', '2026-03-29'] // 04:30, after it
    ];
    for (const [value, day] of cases) {
      expect([...bucketRows([row('r1', { start: value })], FIELDS, CFG, Berlin).days.keys()], value).toEqual(
        [day]
      );
    }
  });

  it('reads an explicit offset, not a guessed UTC one', () => {
    const { days } = bucketRows([row('r1', { start: '2026-03-01T23:30:00+05:00' })], FIELDS, CFG, Berlin);
    expect([...days.keys()]).toEqual(['2026-03-01']);
  });
});

describe('bucketRows — a multi-day row', () => {
  it('is listed on every day from start to end, inclusive', () => {
    const { days, undated } = bucketRows(
      [row('r1', { start: '2026-03-02', ende: '2026-03-04' })],
      FIELDS,
      CFG,
      Berlin
    );
    expect([...days.keys()]).toEqual(['2026-03-02', '2026-03-03', '2026-03-04']);
    expect(on(days, '2026-03-02')).toEqual(['r1']);
    expect(undated).toEqual([]);
  });

  it('marks every day after the first as a continuation', () => {
    const { days } = bucketRows([row('r1', { start: '2026-03-02', ende: '2026-03-04' })], FIELDS, CFG, Berlin);
    expect(days.get('2026-03-02')![0].continues).toBe(false);
    expect(days.get('2026-03-03')![0].continues).toBe(true);
    expect(days.get('2026-03-04')![0].continues).toBe(true);
  });

  it('stays in its zones: an instant span follows the local days', () => {
    const { days } = bucketRows(
      [row('r1', { start: '2026-03-01T23:30:00Z', ende: '2026-03-03T22:30:00Z' })],
      FIELDS,
      CFG,
      Berlin
    );
    expect([...days.keys()]).toEqual(['2026-03-02', '2026-03-03']);
    expect(on(days, '2026-03-02')).toEqual(['r1']);
  });

  it('clips a span at 366 days rather than running away with the page', () => {
    const { days } = bucketRows([row('r1', { start: '2026-01-01', ende: '2030-01-01' })], FIELDS, CFG, Berlin);
    const keys = [...days.keys()];
    expect(keys.length).toBe(366);
    expect(keys[0]).toBe('2026-01-01');
    expect(keys[365]).toBe('2027-01-01');
    for (const entries of days.values()) expect(entries[0].row.id).toBe('r1');
  });

  it('an end before the start is one day at the start, never a dropped row', () => {
    const { days, undated } = bucketRows([row('r1', { start: '2026-03-05', ende: '2026-03-01' })], FIELDS, CFG, Berlin);
    expect([...days.keys()]).toEqual(['2026-03-05']);
    expect(undated).toEqual([]);
  });

  it('a missing or unusable end is a single day', () => {
    for (const ende of [undefined, null, '', 'nope', 7, '2026-02-30']) {
      const { days } = bucketRows([row('r1', { start: '2026-03-05', ende })], FIELDS, CFG, Berlin);
      expect([...days.keys()], JSON.stringify(ende)).toEqual(['2026-03-05']);
    }
  });
});

describe('bucketRows — an undated row', () => {
  it('goes to undated in input order, and never onto a day', () => {
    const rows = [
      row('u1', { start: null }),
      row('u2', {}),
      row('u3', { start: 'morgen' }),
      row('u4', { start: '' }),
      row('u5', { start: '2026-03-09' }),
      row('u6', { start: 42 })
    ];
    const { days, undated } = bucketRows(rows, FIELDS, CFG, Berlin);
    expect(undated.map((r) => r.id)).toEqual(['u1', 'u2', 'u3', 'u4', 'u6']);
    expect([...days.keys()]).toEqual(['2026-03-09']);
    expect(on(days, '2026-03-09')).toEqual(['u5']);
  });

  it('a date key the dataset has no date field for says so rather than drawing nothing', () => {
    expect(() => bucketRows([row('r1', { start: '2026-03-09' })], FIELDS, { ...CFG, date: 'fehlt' }, Berlin)).toThrow();
    expect(() => bucketRows([row('r1', { start: '2026-03-09' })], FIELDS, { ...CFG, date: 'name' }, Berlin)).toThrow();
    expect(() => bucketRows([row('r1', { start: '2026-03-09' })], FIELDS, { ...CFG, date: '' }, Berlin)).toThrow();
  });

  it('an unusable end is not unusable: the rows simply land on their start day', () => {
    const { days } = bucketRows(
      [row('r1', { start: '2026-03-09', ende: '2026-03-11' })],
      FIELDS,
      { ...CFG, end: 'name' },
      Berlin
    );
    expect([...days.keys()]).toEqual(['2026-03-09']);
  });
});

describe('bucketRows — the title', () => {
  const titleOf = (rows: Row[], cfg: CalendarConfig, fields = FIELDS): (string | undefined)[] => [
    ...bucketRows(rows, fields, cfg, Berlin).days.values()
  ][0].map((e) => e.title);

  it('comes from the title field the config names', () => {
    expect(titleOf([row('r1', { start: '2026-03-02', name: 'Tagung' })], CFG)).toEqual(['Tagung']);
  });

  it('falls back to the first text field BY POSITION, not by schema order', () => {
    const cfg = { date: 'start', end: 'ende' } as CalendarConfig;
    // `name` is the later entry in FIELDS but the earlier column by position.
    expect(titleOf([row('r1', { start: '2026-03-02', notiz: 'Vorlauf', name: 'Tagung' })], cfg)).toEqual([
      'Tagung'
    ]);
    // An empty text column does not win the fallback.
    expect(titleOf([row('r1', { start: '2026-03-02', notiz: 'Vorlauf', name: '   ' })], cfg)).toEqual(['Vorlauf']);
  });

  it('falls back to the row id when no text field has a value', () => {
    const cfg = { date: 'start' } as CalendarConfig;
    expect(titleOf([row('r1', { start: '2026-03-02', zahl: 3 })], cfg)).toEqual(['r1']);
    expect(titleOf([row('r1', { start: '2026-03-02' })], cfg)).toEqual(['r1']);
  });

  it('is the same on every day of a span', () => {
    const { days } = bucketRows(
      [row('r1', { start: '2026-03-02', ende: '2026-03-03', name: 'Tagung' })],
      FIELDS,
      CFG,
      Berlin
    );
    expect([...days.values()].flat().map((e) => e.title)).toEqual(['Tagung', 'Tagung']);
  });
});

describe('bucketRows — the order on a day', () => {
  it('is the order the rows came in', () => {
    const { days } = bucketRows(
      [row('r2', { start: '2026-03-02', name: 'Zweite' }), row('r1', { start: '2026-03-02', name: 'Erste' })],
      FIELDS,
      CFG,
      Berlin
    );
    expect(on(days, '2026-03-02')).toEqual(['r2', 'r1']);
  });
});

describe('monthGrid', () => {
  const flat = (grid: string[][]) => grid.flat();

  it('is whole weeks of seven days, Monday first', () => {
    for (const grid of [monthGrid(2026, 3), monthGrid(2026, 2), monthGrid(2027, 1)]) {
      for (const week of grid) expect(week.length).toBe(7);
      const weekday = (d: string) => (new Date(`${d}T12:00:00Z`).getUTCDay() + 6) % 7;
      expect(grid[0].map(weekday)).toEqual([0, 1, 2, 3, 4, 5, 6]);
      expect(weekday(grid[grid.length - 1][6])).toBe(6);
    }
  });

  it('covers every day of the month, padded with the neighbouring days', () => {
    const grid = monthGrid(2026, 3);
    const days = flat(grid);
    for (let d = 1; d <= 31; d++) {
      expect(days).toContain(`2026-03-${String(d).padStart(2, '0')}`);
    }
    expect(days[0]).toBe('2026-02-23'); // the Monday of the week 2026-03-01 (a Sunday) falls in
    expect(days[days.length - 1]).toBe('2026-04-05');
    expect(grid.length).toBe(6);
  });

  it('steps one day at a time across a month boundary', () => {
    const days = flat(monthGrid(2026, 2));
    for (let i = 1; i < days.length; i++) {
      const gap = Date.parse(`${days[i]}T12:00:00Z`) - Date.parse(`${days[i - 1]}T12:00:00Z`);
      expect(gap).toBe(86_400_000);
    }
  });

  it('is December-to-January contiguous', () => {
    const days = flat(monthGrid(2026, 12));
    expect(days[0]).toBe('2026-11-30');
    expect(days[days.length - 1]).toBe('2027-01-03');
    expect(days.length).toBe(35);
  });
});

describe('shiftMonth', () => {
  it('walks the year boundary in both directions', () => {
    expect(shiftMonth('2026-12', 1)).toBe('2027-01');
    expect(shiftMonth('2027-01', -1)).toBe('2026-12');
    expect(shiftMonth('2026-03', 0)).toBe('2026-03');
    expect(shiftMonth('2026-01', -13)).toBe('2024-12');
  });

  it('leaves something that is not a month alone', () => {
    expect(shiftMonth('alles', 1)).toBe('alles');
  });
});

describe('monthLabel', () => {
  it('names the month in German', () => {
    expect(monthLabel('2026-03')).toBe('März 2026');
    expect(monthLabel('2027-01')).toBe('Januar 2027');
  });

  it('leaves something that is not a month alone', () => {
    expect(monthLabel('alles')).toBe('alles');
  });
});

describe('parseMonth', () => {
  const today = new Date('2026-10-12T08:00:00Z');

  it('reads a well-formed month', () => {
    expect(parseMonth('2026-03', today)).toBe('2026-03');
    expect(parseMonth('2027-12', today)).toBe('2027-12');
  });

  it('is today\'s month with nothing to read', () => {
    expect(parseMonth(null, today)).toBe('2026-10');
    expect(parseMonth('', today)).toBe('2026-10');
  });

  it('is today\'s month for anything unusable', () => {
    for (const bad of ['2026-13', '2026-00', '2026-3', '3-2026', 'March', '2026-03-01', 'x']) {
      expect(parseMonth(bad, today), bad).toBe('2026-10');
    }
  });
});

describe('monthHref', () => {
  it('is the view with the month on it', () => {
    const view = { id: 'v1', name: 'Termine', kind: 'calendar', config: CFG, position: 0 };
    expect(monthHref(view, '2026-03')).toBe('?view=v1&month=2026-03');
  });

  it('encodes the id it is given', () => {
    const view = { id: 'zwei eins', name: 'x', kind: 'calendar', config: CFG, position: 0 };
    expect(monthHref(view, '2026-03')).toBe('?view=zwei%20eins&month=2026-03');
  });

  it('still carries a month with no view', () => {
    expect(monthHref(null, '2026-03')).toBe('?month=2026-03');
  });
});

describe('rowTitle', () => {
  it('is the titled column, the first text column, or the row id', () => {
    const r = row('r1', { start: '2026-03-02', name: 'Tagung', notiz: 'Vorlauf' });
    expect(rowTitle(r, FIELDS, 'name')).toBe('Tagung');
    expect(rowTitle(r, FIELDS, null)).toBe('Tagung');
    expect(rowTitle(r, FIELDS, 'mit'))
      .toBe('Tagung');
    expect(rowTitle(row('r2', { start: '2026-03-02', notiz: 'Vorlauf' }), FIELDS, 'name')).toBe('Vorlauf');
    expect(rowTitle(row('r3', {}), FIELDS, 'name')).toBe('r3');
    expect(rowTitle(r, [], 'name')).toBe('r1');
  });

  it('ignores a title naming a column that is not a text column', () => {
    const r = row('r1', { start: '2026-03-02', name: 'Tagung' });
    expect(rowTitle(r, FIELDS, 'start')).toBe('Tagung');
    expect(rowTitle(r, FIELDS, 'name')).toBe('Tagung');
  });

  it('is the same caption the day cell shows', () => {
    const rows = [row('r1', { start: '2026-03-02', name: 'Tagung' })];
    const entry = [...bucketRows(rows, FIELDS, CFG, Berlin).days.values()][0][0];
    expect(entry.title).toBe(rowTitle(rows[0], FIELDS, CFG.title));
  });
});
