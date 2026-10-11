import { describe, expect, it } from 'vitest';
import { buildFormValues, formFields, formKeyOf, submitForm, type Drafts, type Send } from './form';
import type { Field } from './table';
import type { DatasetView } from './views';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'ok', label: 'Fertig', kind: 'bool', config: {}, position: 2 },
  { key: 'sel', label: 'Stufe', kind: 'select', config: { options: [{ id: 'hi', label: 'Hoch' }] }, position: 3 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 4 },
  { key: 'wer', label: 'Wer', kind: 'relation', config: { multiple: false, target_path: '/d2' }, position: 5 }
];

const form = (config: unknown, over: Partial<DatasetView> = {}): DatasetView => ({
  id: 'v1',
  name: 'Formular',
  kind: 'form',
  config,
  position: 0,
  ...over
});

describe('formKeyOf', () => {
  it('names only a form view, by its id', () => {
    expect(formKeyOf(form({ fields: [{ key: 'name' }] }))).toBe('v1');
    expect(formKeyOf(form({}, { kind: 'table' }))).toBeNull();
    expect(formKeyOf(null)).toBeNull();
  });
});

describe('formFields', () => {
  it('follows the view order', () => {
    const items = formFields(form({ fields: [{ key: 'ok' }, { key: 'name' }, { key: 'alter' }] }), fields);
    expect(items.map((i) => i.field.key)).toEqual(['ok', 'name', 'alter']);
  });

  it('drops keys the schema does not have and kinds nobody can edit', () => {
    const items = formFields(form({ fields: [{ key: 'name' }, { key: 'ghost' }, { key: 'calc' }] }), fields);
    expect(items.map((i) => i.field.key)).toEqual(['name']);
  });

  it('uses the form label when it says one, the column label otherwise; carries required', () => {
    const items = formFields(form({ fields: [{ key: 'name', label: 'Titel', required: true }, { key: 'alter' }] }), fields);
    expect(items[0]).toMatchObject({ label: 'Titel', required: true });
    expect(items[1]).toMatchObject({ label: 'Alter', required: false });
  });

  it('is empty for a broken config or a non-form view', () => {
    expect(formFields(form({ fields: 'nicht lista' }), fields)).toEqual([]);
    expect(formFields(form({}, { kind: 'board' }), fields)).toEqual([]);
    expect(formFields(null, fields)).toEqual([]);
  });
});

describe('buildFormValues', () => {
  const items = formFields(form({ fields: [{ key: 'name' }, { key: 'alter' }, { key: 'ok', required: true }] }), fields);

  it('sends typed values and leaves empty optional cells out', () => {
    const r = buildFormValues(items, { name: 'Ada', alter: '36,5', ok: 'true' });
    expect(r).toEqual({ ok: true, values: { name: 'Ada', alter: 36.5, ok: true } });
  });

  it('a false bool is an answer, not an empty field', () => {
    expect(buildFormValues(items, { name: '', alter: '', ok: 'false' })).toEqual({ ok: true, values: { ok: false } });
  });

  it('required: empty string, whitespace, null and a missing draft are all unanswered', () => {
    for (const drafts of [{ name: 'x' }, { name: 'x', ok: '  ' }, { name: 'x', ok: null }, { name: 'x', ok: undefined }]) {
      const r = buildFormValues(items, drafts as unknown as Drafts);
      expect(r).toEqual({ ok: false, errors: { ok: 'Pflichtfeld' } });
    }
  });

  it('kind parse errors are reported per field', () => {
    expect(buildFormValues(items, { name: 'x', alter: 'keine Zahl', ok: 'true' })).toEqual({
      ok: false,
      errors: { alter: 'Bitte eine Zahl eingeben.' }
    });
  });

  it('a draft key outside the form never reaches the values', () => {
    const r = buildFormValues(items, { name: 'Ada', ok: 'true', geheim: 'vertraulich', calc: 5 } as unknown as Drafts);
    expect(r.ok && !('geheim' in r.values) && !('calc' in r.values)).toBe(true);
  });
});

function spy(answer: { status: number; message?: string | null }) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  const send: Send = async (method, path, body) => {
    calls.push({ method, path, body });
    return { status: answer.status, message: answer.message ?? null, body: null };
  };
  return { send, calls };
}

describe('submitForm', () => {
  const items = formFields(form({ fields: [{ key: 'name', required: true }] }), fields);

  it('one POST with {values, view}', async () => {
    const { send, calls } = spy({ status: 201 });
    const r = await submitForm(send, '/d', 'v1', items, { name: 'Ada' });
    expect(r).toEqual({ kind: 'saved' });
    expect(calls).toEqual([{ method: 'POST', path: '/api/datasets/rows/d', body: { values: { name: 'Ada' }, view: 'v1' } }]);
  });

  it('a client-side refusal sends nothing', async () => {
    const { send, calls } = spy({ status: 201 });
    const r = await submitForm(send, '/d', 'v1', items, { name: '  ' });
    expect(r).toEqual({ kind: 'invalid', errors: { name: 'Pflichtfeld' } });
    expect(calls).toEqual([]);
  });

  it('the server says why in words, and they carry through', async () => {
    const { send } = spy({ status: 422, message: 'the form has no field "geheim"' });
    const r = await submitForm(send, '/d', 'v1', items, { name: 'Ada' });
    expect(r.kind === 'failed' && r.message).toContain('422');
    expect(r.kind === 'failed' && r.message).toContain('the form has no field "geheim"');
  });

  it('no answer is said in words too', async () => {
    const { send } = spy({ status: 0 });
    const r = await submitForm(send, '/d', 'v1', items, { name: 'Ada' });
    expect(r.kind === 'failed' && r.message).toBe('Die Zeile konnte nicht gespeichert werden: keine Antwort.');
  });
});
