/**
 * The pure half of relation cells (M8b): reading a column's config, naming a target row, and
 * deciding what the picker lists. Everything here is data — the components only ever print it.
 *
 * A picker offers EXACTLY the rows the target's first page returned (plus a placeholder for a
 * selected id that is gone), never anything from elsewhere; whether the caller may read the
 * target is already answered by the schema, which omits both the column and its path if not.
 */
import { rowsApiPath, type Row } from '../table';

export const RELATION_FETCH_FAILURE = 'Die Zielzeilen konnten nicht geladen werden.';
export const MISSING_LABEL = '(nicht mehr vorhanden)';

export interface RelationConfig {
  multiple: boolean;
  targetPath: string;
}

export interface PickerOption {
  id: string;
  label: string;
}

/** `{multiple, target_path}` from the schema; without a path there is nothing to pick from. */
export function relationConfig(config: unknown): RelationConfig | null {
  const c = config as { multiple?: unknown; target_path?: unknown } | null;
  if (typeof c?.target_path !== 'string' || c.target_path === '') return null;
  return { multiple: c.multiple === true, targetPath: c.target_path };
}

/** A target row's own name: its first non-empty string or number value, else its id. */
export function rowLabel(row: Row): string {
  // Strings win over numbers whatever the key order (the API's value maps are key-sorted, so
  // `alter` comes before `name`): a number names a row only when no text does.
  const values = Object.values(row.values ?? {});
  const text = values.find((v): v is string => typeof v === 'string' && v.trim() !== '');
  if (text !== undefined) return text;
  const num = values.find((v): v is number => typeof v === 'number');
  return num !== undefined ? String(num) : row.id;
}

/** `rowId -> label` for one page of target rows — the shape a relation column's cells want. */
export function labelMap(rows: Row[]): Record<string, string> {
  const m: Record<string, string> = {};
  for (const r of rows) m[r.id] = rowLabel(r);
  return m;
}

/** The picker's whole list: the target's rows, then a placeholder per selected id not among them. */
export function pickerOptions(rows: Row[], selected: string[]): PickerOption[] {
  const opts = rows.map((r) => ({ id: r.id, label: rowLabel(r) }));
  const known = new Set(rows.map((r) => r.id));
  for (const id of new Set(selected)) if (!known.has(id)) opts.push({ id, label: MISSING_LABEL });
  return opts;
}

/** Tick or untick one id without ever repeating it. */
export function toggleId(current: string[], id: string, on: boolean): string[] {
  const rest = current.filter((x) => x !== id);
  return on ? [...rest, id] : rest;
}

/** One page of the target's rows. Anything but a 2xx throws — the component states it in German. */
export async function fetchTargetRows(fetchFn: typeof fetch, targetPath: string): Promise<Row[]> {
  const res = await fetchFn(rowsApiPath(targetPath, {}));
  if (!res.ok) throw new Error(`target rows answered ${res.status}`);
  const body = (await res.json()) as { rows?: Row[] };
  return body.rows ?? [];
}
