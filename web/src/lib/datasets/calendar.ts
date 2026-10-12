/**
 * The pure half of a dataset page's saved CALENDAR view (M8d D3b): which day of which month a
 * row sits on, and which weeks a month is drawn over.
 *
 * A day is decided in a zone that is handed IN rather than assumed — the caller passes the
 * reader's own, the tests pass a fixed one — because the same instant is Tuesday in Berlin and
 * Monday in Los Angeles, and a calendar that quietly used UTC would put a late appointment on
 * the wrong page. A date written without a time is a calendar day: it never moves.
 *
 * Pure — no DOM, no fetch.
 */
import type { Field, Row } from './table';
import { VIEW_PARAM, type DatasetView } from './views';

export interface CalendarConfig {
  date: string;
  end?: string | null;
  title?: string | null;
}

/** One row as it appears on one day. `continues` is the multi-day marker: not its first day. */
export interface CalendarEntry {
  row: Row;
  title: string;
  continues: boolean;
}

export interface CalendarBuckets {
  /** `YYYY-MM-DD` → the entries on it, in the order the rows came in. Only days that have one. */
  days: Map<string, CalendarEntry[]>;
  /** Rows with nothing to place them by, in input order. Never dropped, never on a day. */
  undated: Row[];
}

export const MONTH_PARAM = 'month';

/** A span longer than this is a mistake or a malicious config; the page must not be eaten by it. */
const MAX_SPAN_DAYS = 366;

const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/;
const MONTH = /^(\d{4})-(\d{2})$/;
const DAY_MS = 86_400_000;

const utcDay = (ms: number): string => new Date(ms).toISOString().slice(0, 10);

/** `YYYY-MM-DD` for a real calendar day, else `null`. A date is never shifted by a zone. */
function plainDay(value: unknown): string | null {
  if (typeof value !== 'string' || !DATE_ONLY.test(value)) return null;
  const ms = Date.parse(`${value}T00:00:00Z`);
  return Number.isNaN(ms) ? null : utcDay(ms);
}

/** The formatter that answers "which local day is this instant, in this zone". */
function localDay(tz: string): (ms: number) => string {
  let parts: Intl.DateTimeFormat;
  try {
    parts = new Intl.DateTimeFormat('en-US', { timeZone: tz, year: 'numeric', month: '2-digit', day: '2-digit' });
  } catch {
    throw new Error(`Die Zeitzone ${tz} kann hier nicht verwendet werden.`);
  }
  return (ms: number): string => {
    const seen: Record<string, string> = {};
    for (const part of parts.formatToParts(new Date(ms))) {
      if (part.type !== 'literal') seen[part.type] = part.value;
    }
    return `${seen.year}-${seen.month}-${seen.day}`;
  };
}

/** The local day of one cell value: a plain date as written, an instant converted to `tz`. */
function dayOf(value: unknown, toLocalDay: (ms: number) => string): string | null {
  if (typeof value !== 'string') return null;
  const plain = plainDay(value);
  if (plain) return plain;
  if (!value.includes('T')) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : toLocalDay(ms);
}

/** Every day from `start` to `end` inclusive; an end that is not after the start is no span. */
function spanDays(start: string, end: string | null): string[] {
  if (!end || end < start) return [start];
  const days: string[] = [];
  const last = Date.parse(`${end}T00:00:00Z`);
  for (let cursor = Date.parse(`${start}T00:00:00Z`); cursor <= last && days.length < MAX_SPAN_DAYS; ) {
    days.push(utcDay(cursor));
    cursor += DAY_MS;
  }
  return days;
}

/**
 * The caption of one row: the titled column, else the first text column by position, else the
 * row's own id. Exported because the undated list beside the month needs the same one the cells
 * use, and `bucketRows` hands those rows back as they came.
 */
export function rowTitle(row: Row, fields: Field[], titleKey: string | null | undefined): string {
  const value = (key: string): string | null => {
    const raw = row.values[key];
    return typeof raw === 'string' && raw.trim() !== '' ? raw : null;
  };
  // Only a key the dataset has as a TEXT column counts as a title: a config naming a column that
  // is gone, hidden or not text falls through to the fallback rather than printing a value the
  // schema never offered.
  const isText = (key: string | null | undefined): key is string =>
    fields.some((f) => f.key === key && f.kind === 'text');
  if (isText(titleKey)) {
    const named = value(titleKey);
    if (named) return named;
  }
  const byPosition = [...fields].sort((a, b) => a.position - b.position);
  for (const field of byPosition) {
    if (field.kind !== 'text') continue;
    const text = value(field.key);
    if (text) return text;
  }
  return row.id;
}

/**
 * Each row on the day or days it occupies, and the rows nothing places beside them.
 *
 * `config.date` has to name a date column of `fields` or there is no calendar to draw, and this
 * says so by throwing — the view turns that into a stated failure rather than an empty month.
 * An unusable `end` or `title` is not that kind of failure: the row simply gets one day, or the
 * fallback caption.
 */
export function bucketRows(
  rows: Row[],
  fields: Field[],
  config: CalendarConfig,
  tz: string
): CalendarBuckets {
  const toLocalDay = localDay(tz);
  const dateField = fields.find((f) => f.key === config.date);
  if (!dateField || dateField.kind !== 'date') {
    throw new Error(`Dieser Kalender hat kein Datumsfeld: ${JSON.stringify(config.date)}.`);
  }
  const endKey = typeof config.end === 'string' && config.end !== '' ? config.end : null;
  const usableEnd = endKey && fields.find((f) => f.key === endKey)?.kind === 'date' ? endKey : null;

  const days = new Map<string, CalendarEntry[]>();
  const undated: Row[] = [];
  for (const row of rows) {
    const start = dayOf(row.values[config.date], toLocalDay);
    if (!start) {
      undated.push(row);
      continue;
    }
    const end = usableEnd ? dayOf(row.values[usableEnd], toLocalDay) : null;
    const title = rowTitle(
      row,
      fields,
      typeof config.title === 'string' && config.title !== '' ? config.title : null
    );
    spanDays(start, end).forEach((day, index) => {
      const onDay = days.get(day);
      const entry: CalendarEntry = { row, title, continues: index > 0 };
      if (onDay) onDay.push(entry);
      else days.set(day, [entry]);
    });
  }
  return { days, undated };
}

/** The weeks covering a month: whole weeks of seven days, Monday first, padded at both ends. */
export function monthGrid(year: number, month: number): string[][] {
  const first = Date.UTC(year, month - 1, 1);
  const leading = (new Date(first).getUTCDay() + 6) % 7;
  const inMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  const weeks = Math.ceil((leading + inMonth) / 7);
  const grid: string[][] = [];
  for (let w = 0; w < weeks; w++) {
    const week: string[] = [];
    for (let d = 0; d < 7; d++) week.push(utcDay(first - leading * DAY_MS + (w * 7 + d) * DAY_MS));
    grid.push(week);
  }
  return grid;
}

/** `YYYY-MM` from the address; anything else is the month `today` is in. */
export function parseMonth(raw: string | null, today: Date): string {
  const found = raw ? MONTH.exec(raw) : null;
  if (found) {
    const month = Number(found[2]);
    if (month >= 1 && month <= 12) return `${found[1]}-${found[2]}`;
  }
  return `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}`;
}

/** One month over: `shiftMonth('2026-12', 1)` is `2027-01`. */
export function shiftMonth(month: string, by: number): string {
  const found = MONTH.exec(month);
  if (!found) return month;
  const index = Number(found[1]) * 12 + (Number(found[2]) - 1) + by;
  return `${String(Math.floor(index / 12)).padStart(4, '0')}-${String((index % 12 + 12) % 12 + 1).padStart(2, '0')}`;
}

/** The month's name for a heading, in German: `März 2026`. */
export function monthLabel(month: string): string {
  const found = MONTH.exec(month);
  if (!found) return month;
  const stamp = `${found[1]}-${found[2]}-01T00:00:00Z`;
  return new Intl.DateTimeFormat('de-DE', { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(
    new Date(stamp)
  );
}

/** The address of one month of this view. The same plain-query convention as `viewHref`. */
export function monthHref(view: DatasetView | null, month: string): string {
  const parts: string[] = [];
  if (view) parts.push(`${VIEW_PARAM}=${encodeURIComponent(view.id)}`);
  parts.push(`${MONTH_PARAM}=${encodeURIComponent(month)}`);
  return `?${parts.join('&')}`;
}
