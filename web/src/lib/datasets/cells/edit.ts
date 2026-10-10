/**
 * The pure half of cell editing (ADR 0029): a form entry (`raw`) becomes the value gw-core's
 * `validate_value` accepts for the kind, or a German reason it would not. The server stays the
 * authority; this only spares a round trip and keeps the draft shaped right.
 *
 * `raw` is a string for every kind except `multi_select` (a string array). An empty entry
 * clears the cell (`null`).
 */
export type Raw = string | string[];
export type Parsed = { ok: true; value: unknown } | { ok: false; error: string };

const done = (value: unknown): Parsed => ({ ok: true, value });
const fail = (error: string): Parsed => ({ ok: false, error });

/** Kinds a person can edit here; the rest are computed or wait for M8b. */
export const EDITABLE = ['text', 'number', 'bool', 'date', 'url'];

const isEmpty = (raw: Raw) => (Array.isArray(raw) ? raw.length === 0 : raw.trim() === '');

export function fromRaw(kind: string, config: unknown, raw: Raw): Parsed {
  if (!EDITABLE.includes(kind)) return fail('Dieses Feld lässt sich nicht bearbeiten.');
  if (isEmpty(raw)) return done(null);
  const s = typeof raw === 'string' ? raw.trim() : '';
  switch (kind) {
    case 'text':
      return done(raw as string);
    case 'number': {
      const n = Number(s.replace(',', '.'));
      return Number.isFinite(n) ? done(n) : fail('Bitte eine Zahl eingeben.');
    }
    case 'bool':
      return s === 'true' ? done(true) : s === 'false' ? done(false) : fail('Bitte Ja oder Nein wählen.');
    case 'date':
      return /^\d{4}-\d{2}-\d{2}$/.test(s) && !Number.isNaN(Date.parse(s))
        ? done(s)
        : fail('Bitte ein Datum im Format JJJJ-MM-TT eingeben.');
    case 'url':
      return /^(https?:\/\/|mailto:)\S+$/i.test(s)
        ? done(s)
        : fail('Bitte eine Adresse mit http://, https:// oder mailto: eingeben.');
  }
  return fail('Dieses Feld lässt sich nicht bearbeiten.');
}

/** A stored value as the entry that would produce it again. */
export function toRaw(kind: string, value: unknown): Raw {
  if (value === null || value === undefined) return '';
  if (kind === 'number' && typeof value === 'number') return String(value);
  if (kind === 'bool') return value === true ? 'true' : value === false ? 'false' : '';
  return typeof value === 'string' ? value : String(value);
}
