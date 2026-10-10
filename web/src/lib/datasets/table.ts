/**
 * The pure half of the dataset table (ADR 0029): which request a sort/filter/cursor becomes,
 * which link a header carries, and what each kind of value looks like in a cell.
 *
 * Every cell is described as DATA (`text`, `chips`, `link`, `error`), never as markup, so the
 * component can only ever put it on the page as text. A `url` becomes a link only for
 * `http(s)`; anything else (`javascript:`, `data:`) stays plain text.
 */

/** Mirrors `gw_store::datasets::DatasetField`. */
export interface Field {
  key: string;
  label: string;
  kind: string;
  config: unknown;
  position: number;
}

/** Mirrors `gw_store::datasets::DatasetRow`. */
export interface Row {
  id: string;
  values: Record<string, unknown>;
  version: number;
  created_at: string;
  updated_at: string;
}

/** Mirrors `gw_store::datasets::RowPage`; `next` is absent on the last page. */
export interface RowPage {
  rows: Row[];
  total: number;
  next?: string;
}

export interface SortState {
  sort: string;
  desc: boolean;
}

/** One "contains" filter on one column; the table offers a single chip. */
export interface FilterState {
  key: string;
  value: string;
}

export interface RowsQuery {
  sort?: string | null;
  desc?: boolean;
  filter?: FilterState | null;
  after?: string | null;
  limit?: number;
}

export const SORT_PARAM = 'sort';
export const DESC_PARAM = 'desc';
export const FILTER_PARAM = 'f';

const strip = (path: string) => path.replace(/^\/+/, '');

export const fieldsApiPath = (path: string) => `/api/datasets/schema/${strip(path)}`;

/** `GET /api/datasets/rows/{path}` with whichever of sort, filter and cursor are set. */
export function rowsApiPath(path: string, q: RowsQuery): string {
  const p = new URLSearchParams();
  if (q.sort) {
    p.set('sort', q.sort);
    p.set('desc', String(!!q.desc));
  }
  if (q.filter) {
    p.set('filter', JSON.stringify([{ op: 'contains', key: q.filter.key, value: q.filter.value }]));
  }
  if (q.after) p.set('after', q.after);
  if (q.limit) p.set('limit', String(q.limit));
  const qs = p.toString();
  return `/api/datasets/rows/${strip(path)}${qs ? `?${qs}` : ''}`;
}

/** Clicking a header: ascending first, then descending; another column starts ascending. */
export function nextSort(current: SortState | null, key: string): SortState {
  if (current && current.sort === key) return { sort: key, desc: !current.desc };
  return { sort: key, desc: false };
}

/** The address a header link points at: a plain query string, so it works without a script. */
export function sortHref(current: SortState | null, key: string, filter: FilterState | null): string {
  const n = nextSort(current, key);
  const p = new URLSearchParams({ [SORT_PARAM]: n.sort, [DESC_PARAM]: n.desc ? '1' : '0' });
  if (filter) p.set(FILTER_PARAM, `${filter.key}:${filter.value}`);
  return `?${p.toString()}`;
}

/** The address with the filter removed (the sort stays). */
export function withoutFilterHref(sort: SortState | null): string {
  if (!sort) return '?';
  return `?${new URLSearchParams({ [SORT_PARAM]: sort.sort, [DESC_PARAM]: sort.desc ? '1' : '0' })}`;
}

/** `key:value` from the address; only a key the dataset really has is accepted. */
export function filterFrom(raw: string | null, fields: Field[]): FilterState | null {
  if (!raw) return null;
  const at = raw.indexOf(':');
  if (at < 1) return null;
  const key = raw.slice(0, at);
  const value = raw.slice(at + 1);
  if (!value || !fields.some((f) => f.key === key)) return null;
  return { key, value };
}

/** The sort from the address; only a key the dataset really has is accepted. */
export function sortFrom(sort: string | null, desc: string | null, fields: Field[]): SortState | null {
  if (!sort || !fields.some((f) => f.key === sort)) return null;
  return { sort, desc: desc === '1' || desc === 'true' };
}

export type Cell =
  | { kind: 'text'; text: string; align?: 'end' }
  | { kind: 'chips'; items: string[] }
  | { kind: 'link'; href: string; text: string }
  | { kind: 'error'; reason: string };

const numberFormat = new Intl.NumberFormat('de-DE', { maximumFractionDigits: 10 });

function isErrorValue(v: unknown): v is { error: string } {
  return typeof v === 'object' && v !== null && !Array.isArray(v) && typeof (v as { error?: unknown }).error === 'string';
}

function plain(v: unknown): string {
  if (v === null || v === undefined) return '';
  if (typeof v === 'string') return v;
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  return JSON.stringify(v);
}

/** What one value looks like in a column of this kind. */
export function cellOf(field: Field, value: unknown): Cell {
  if (isErrorValue(value)) return { kind: 'error', reason: value.error };
  if (value === null || value === undefined || value === '') return { kind: 'text', text: '' };
  switch (field.kind) {
    case 'number':
      return typeof value === 'number'
        ? { kind: 'text', text: numberFormat.format(value), align: 'end' }
        : { kind: 'text', text: plain(value) };
    case 'bool':
      return { kind: 'text', text: value === true ? 'Ja' : 'Nein' };
    case 'date': {
      const m = typeof value === 'string' ? /^(\d{4})-(\d{2})-(\d{2})/.exec(value) : null;
      return { kind: 'text', text: m ? `${m[3]}.${m[2]}.${m[1]}` : plain(value) };
    }
    case 'select':
      return { kind: 'chips', items: [plain(value)] };
    case 'multi_select':
    case 'tags':
      return { kind: 'chips', items: Array.isArray(value) ? value.map(plain) : [plain(value)] };
    case 'url': {
      const text = plain(value);
      return /^https?:\/\//i.test(text) ? { kind: 'link', href: text, text } : { kind: 'text', text };
    }
    default:
      return { kind: 'text', text: plain(value) };
  }
}

export function describeRowsFailure(status: number): string {
  if (status === 0) return 'Die Tabelle konnte nicht geladen werden: keine Antwort.';
  return `Die Tabelle konnte nicht geladen werden (Fehler ${status}).`;
}

export function describeAddFailure(status: number, message: string | null): string {
  if (status === 0) return 'Die Zeile konnte nicht gespeichert werden: keine Antwort.';
  const base = `Die Zeile konnte nicht gespeichert werden (Fehler ${status}).`;
  return message ? `${base} ${message}` : base;
}

/** Kinds the minimal inline form can fill; the rest wait for the row editor. */
export const ADDABLE = ['text', 'number', 'bool', 'date', 'url', 'select', 'multi_select', 'tags'];

/** One form entry as the value the API validates for its kind; `undefined` leaves the cell out. */
export function valueFromInput(kind: string, raw: string | boolean): unknown {
  if (kind === 'bool') return raw === true ? true : undefined;
  if (typeof raw !== 'string' || raw.trim() === '') return undefined;
  if (kind === 'number') {
    const n = Number(raw.replace(',', '.'));
    return Number.isFinite(n) ? n : raw;
  }
  if (kind === 'multi_select' || kind === 'tags') {
    return raw
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
  }
  return raw.trim();
}
