/**
 * The pure half of a saved FORM view (M8c C6): which fields the form asks for, whether the
 * draft answers them, and what one submit becomes on the wire. A form is the caller's promise
 * about a subset of the dataset — `fields` in view order, optional `label` and `required` —
 * and a submission carries `{values, view}` so the SERVER re-checks the promise (the store
 * refuses a key the form does not hold and an unanswered required field). This file enforces
 * the same subset client-side, so a crafted draft cannot even try. No HTML is built here.
 */
import { editableFields, type Drafts, type Send } from './rowedit';
import { describeAddFailure, rowsApiPath, type Field } from './table';
import type { DatasetView } from './views';
import { fromRaw } from './cells/edit';

export type { Drafts, Send } from './rowedit';

/** One question of the form, resolved against the schema: the column it asks about. */
export interface FormItem {
  field: Field;
  label: string;
  required: boolean;
}

export type FormValues = { ok: true; values: Record<string, unknown> } | { ok: false; errors: Record<string, string> };

export type FormResult =
  | { kind: 'saved' }
  | { kind: 'invalid'; errors: Record<string, string> }
  | { kind: 'failed'; message: string };

/** The id to submit under, or `null` for anything that is not a form view. */
export function formKeyOf(view: DatasetView | null | undefined): string | null {
  return view && view.kind === 'form' ? view.id : null;
}

/**
 * The form's questions in the order the view lists them. A key the dataset does not have, or
 * a column this editor cannot produce a value for (computed kinds), is dropped — the view is
 * not allowed to invent a field, and the row editor has no control for the rest.
 */
export function formFields(view: DatasetView | null | undefined, fields: Field[]): FormItem[] {
  if (!view || formKeyOf(view) === null) return [];
  const list = (view.config as { fields?: unknown } | null)?.fields;
  if (!Array.isArray(list)) return [];
  const byKey = new Map(editableFields(fields).map((f) => [f.key, f]));
  const seen = new Set<string>();
  const items: FormItem[] = [];
  for (const entry of list) {
    const key = (entry as { key?: unknown } | null)?.key;
    if (typeof key !== 'string' || seen.has(key)) continue;
    const field = byKey.get(key);
    if (!field) continue;
    seen.add(key);
    const label = (entry as { label?: unknown }).label;
    const required = (entry as { required?: unknown }).required === true;
    items.push({ field, label: typeof label === 'string' && label.trim() !== '' ? label : field.label, required });
  }
  return items;
}

const isUnanswered = (raw: unknown) =>
  raw === null || raw === undefined || (Array.isArray(raw) ? raw.length === 0 : String(raw).trim() === '');

/**
 * Typed values from the draft, exactly over the form's items: an empty optional cell is left
 * out, a required one is `Pflichtfeld`, and a key nobody put on the form never reaches the
 * body — the subset the server enforces, enforced here first.
 */
export function buildFormValues(items: FormItem[], drafts: Drafts): FormValues {
  const values: Record<string, unknown> = {};
  const errors: Record<string, string> = {};
  for (const item of items) {
    const key = item.field.key;
    const raw = drafts[key];
    if (isUnanswered(raw)) {
      if (item.required) errors[key] = 'Pflichtfeld';
      continue;
    }
    const parsed = fromRaw(item.field.kind, item.field.config, raw as string | string[]);
    if (!parsed.ok) errors[key] = parsed.error;
    else if (parsed.value !== null) values[key] = parsed.value;
  }
  return Object.keys(errors).length > 0 ? { ok: false, errors } : { ok: true, values };
}

/** One POST `{values, view}` to the rows endpoint; 201 is saved, a 4xx says why in words. */
export async function submitForm(
  send: Send,
  path: string,
  viewId: string,
  items: FormItem[],
  drafts: Drafts
): Promise<FormResult> {
  const built = buildFormValues(items, drafts);
  if (!built.ok) return { kind: 'invalid', errors: built.errors };
  const answer = await send('POST', rowsApiPath(path, {}), { values: built.values, view: viewId });
  if (answer.status === 201) return { kind: 'saved' };
  return { kind: 'failed', message: describeAddFailure(answer.status, answer.message) };
}
