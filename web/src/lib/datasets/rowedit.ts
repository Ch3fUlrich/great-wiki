/**
 * Saving one row (ADR 0029): a draft of form entries becomes the validated `values` of a POST
 * (new row) or PATCH (`{id, version, values}`, only the cells that changed; `null` clears).
 * A stale version answers 409 with the row as it is now; that is handed back as `stale` so the
 * caller can show it and KEEP the draft. No HTML is built here.
 */
import { EDITABLE, fromRaw, type Raw } from './cells/edit';
import { describeAddFailure, rowsApiPath, type Field, type Row } from './table';

export type Send = (
  method: 'POST' | 'PATCH',
  path: string,
  body: unknown
) => Promise<{ status: number; message: string | null; body: unknown }>;

export type Drafts = Record<string, Raw>;

export type SubmitResult =
  | { kind: 'saved' }
  | { kind: 'unchanged' }
  | { kind: 'invalid'; errors: Record<string, string> }
  | { kind: 'stale'; current: Row | null }
  | { kind: 'failed'; message: string };

/** Fields the editor offers: the kinds `fromRaw` can produce a value for. */
export const editableFields = (fields: Field[]) => fields.filter((f) => EDITABLE.includes(f.kind));

const same = (a: unknown, b: unknown) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

export function describeSaveFailure(status: number, message: string | null): string {
  if (status === 0) return 'Die Zeile konnte nicht gespeichert werden: keine Antwort.';
  if (status === 403) return 'Du darfst diese Zeile nicht ändern (Fehler 403).';
  return describeAddFailure(status, message);
}

export async function submitRow(
  send: Send,
  path: string,
  fields: Field[],
  row: Row | null,
  drafts: Drafts
): Promise<SubmitResult> {
  const values: Record<string, unknown> = {};
  const errors: Record<string, string> = {};
  for (const f of editableFields(fields)) {
    if (!(f.key in drafts)) continue;
    const p = fromRaw(f.kind, f.config, drafts[f.key]);
    if (!p.ok) {
      errors[f.key] = p.error;
      continue;
    }
    if (row) {
      if (!same(p.value, row.values?.[f.key])) values[f.key] = p.value;
    } else if (p.value !== null) {
      values[f.key] = p.value;
    }
  }
  if (Object.keys(errors).length > 0) return { kind: 'invalid', errors };
  if (row && Object.keys(values).length === 0) return { kind: 'unchanged' };

  const url = rowsApiPath(path, {});
  const answer = row
    ? await send('PATCH', url, { id: row.id, version: row.version, values })
    : await send('POST', url, { values });
  if (answer.status === 200 || answer.status === 201) return { kind: 'saved' };
  if (answer.status === 409) {
    const cur = (answer.body as { current?: Row } | null)?.current;
    return { kind: 'stale', current: cur ?? null };
  }
  return { kind: 'failed', message: describeSaveFailure(answer.status, answer.message) };
}
