/**
 * The vocabulary and the arithmetic of the revision history.
 *
 * The wire types mirror `gw_api::routes::revisions` exactly; everything else here is the
 * German the API does not speak and the small calculations the timeline needs. All of it is
 * pure, and that is deliberate rather than tidy: this project has no DOM environment, so a
 * component's behaviour can only be asserted through what it renders. Anything that has to
 * be *right* — how old a revision is, how much bigger it made the page, what a block kind is
 * called — is lifted out here where a test can reach it, and `+page.svelte` is left holding
 * markup.
 *
 * Imported from the browser as well as from `+page.server.ts`, so it must stay free of
 * `$env/dynamic/private` — see `$lib/adminApi` for what that rule is protecting.
 */

/** What happened to one thing. Mirrors `gw_core::diff::ChangeKind`. */
export type ChangeKind = 'added' | 'removed' | 'moved' | 'changed';

/**
 * One entry in the timeline. Mirrors `gw_api::routes::revisions::RevisionView`.
 *
 * No body and no author id: the list shows neither, and thirty-four bodies on the wire for a
 * list that renders none of them would be thirty-four copies of the page.
 */
export interface RevisionSummary {
  id: string;
  /** What this was published on top of. `null` for the first revision of the page. */
  parent_id: string | null;
  summary: string | null;
  /** The display name as it was when this was published — never resolved again later. */
  author_name: string;
  /** `false` for the import that ran with no account. Never render that as a person. */
  author_is_account: boolean;
  byte_size: number;
  created_at: string;
}

export interface ProseChange {
  kind: ChangeKind;
  text: string;
}

export interface StructureChange {
  kind: ChangeKind;
  /** A `BlockKind` as the document model spells it: `paragraph`, `bulletList`, … */
  block: string;
  text: string;
  from_index: number | null;
  to_index: number | null;
}

export interface DesignChange {
  block: string;
  text: string;
  attribute: string;
  before: string | null;
  after: string | null;
}

/** `GET /api/revisions/{a}/diff/{b}`. All three lists are always present. */
export interface RevisionDiff {
  from: RevisionSummary;
  to: RevisionSummary;
  prose: ProseChange[];
  structure: StructureChange[];
  design: DesignChange[];
}

/** `GET /api/revisions/{id}/source?path=…` — the export triple for one version. */
export interface RevisionSource {
  revision: RevisionSummary;
  /** `null` when this tree cannot be written as markdown faithfully; see `problem`. */
  markdown: string | null;
  problem: string | null;
  meta: string | null;
  design: string;
}

// --- The four views -------------------------------------------------------------------

/**
 * Which tab is showing. German, because it is in the address bar and this is a German
 * interface — the same choice `/graph` makes with `?wurzel=`.
 */
export const VIEWS = ['prosa', 'struktur', 'design', 'quelle'] as const;
export type View = (typeof VIEWS)[number];

export const VIEW_LABEL: Record<View, string> = {
  prosa: 'Prosa',
  struktur: 'Struktur',
  design: 'Design',
  quelle: 'Quelltext'
};

/** What each tab is for, in one line, shown under it. */
export const VIEW_HINT: Record<View, string> = {
  prosa: 'Welche Wörter sich geändert haben.',
  struktur: 'Welche Blöcke dazugekommen, verschwunden, verschoben oder ersetzt worden sind.',
  design: 'Was sich am Aussehen geändert hat: Ebenen, Ausrichtungen, Formatierungen.',
  quelle: 'Diese Fassung als Datei: Text, Metadaten und der gespeicherte Baum.'
};

export function isView(value: string | null): value is View {
  return value !== null && (VIEWS as readonly string[]).includes(value);
}

// --- The vocabulary -------------------------------------------------------------------

/**
 * What each kind of change is called.
 *
 * `moved` and `changed` are the two that earn the diff its keep: without them a reorder and
 * a corrected word both read as "entfernt" plus "hinzugefügt", which is how a tidy-up comes
 * to look like a rewrite.
 */
export const CHANGE_LABEL: Record<ChangeKind, string> = {
  added: 'Hinzugefügt',
  removed: 'Entfernt',
  moved: 'Verschoben',
  changed: 'Geändert'
};

/**
 * The marker printed beside each change.
 *
 * Not decoration: additions and removals must be distinguishable **without colour**, so
 * every change carries a character and a word as well as a background. A reader with any
 * form of colour blindness, a printed page and a black-and-white screenshot all get the same
 * information as everybody else.
 */
export const CHANGE_MARK: Record<ChangeKind, string> = {
  added: '+',
  removed: '−',
  moved: '↕',
  changed: '≠'
};

/** German for every `BlockKind` in `gw_core::block`. */
export const BLOCK_LABEL: Record<string, string> = {
  doc: 'Dokument',
  paragraph: 'Absatz',
  heading: 'Überschrift',
  bulletList: 'Aufzählung',
  orderedList: 'Nummerierte Liste',
  listItem: 'Listenpunkt',
  taskList: 'Aufgabenliste',
  taskItem: 'Aufgabe',
  blockquote: 'Zitat',
  codeBlock: 'Codeblock',
  table: 'Tabelle',
  tableRow: 'Tabellenzeile',
  tableHeader: 'Kopfzelle',
  tableCell: 'Tabellenzelle',
  // A file placed in the prose (D-15). »Datei« rather than »Bild«: a placement carries a
  // name and a description and says nothing about what the file is — whether it shows as a
  // picture is decided when it is read, from the media type the bytes were sniffed as.
  attachment: 'Datei',
  text: 'Text'
};

/**
 * German for the attributes a design diff reports.
 *
 * `marks` is `gw_core::diff::MARKS_ATTRIBUTE` — inline formatting, which is not an attribute
 * at all in the data model but is the only kind of change the other two modes cannot see.
 */
export const ATTRIBUTE_LABEL: Record<string, string> = {
  level: 'Ebene',
  language: 'Sprache',
  alignment: 'Ausrichtung',
  start: 'Beginnt bei',
  checked: 'Erledigt',
  // The two a placement carries. A changed `filename` is the whole of "this shows a
  // different file now", and it is the only place a revision can say so: a placement has no
  // text, so `diff_structure` fingerprints every one of them alike and only the design diff
  // can tell two apart.
  filename: 'Datei',
  alt: 'Bildbeschreibung',
  marks: 'Formatierung'
};

/**
 * The label, or the raw name when there is none.
 *
 * The fallback is not laziness. A kind this table does not know is information — the content
 * model grew one and this interface has not caught up — and rendering an empty cell would
 * hide the very row somebody is trying to read.
 */
export function blockLabel(kind: string): string {
  return BLOCK_LABEL[kind] ?? kind;
}

export function attributeLabel(name: string): string {
  return ATTRIBUTE_LABEL[name] ?? name;
}

// --- Time -----------------------------------------------------------------------------

/**
 * A stored timestamp as milliseconds, or `null` when it is not one.
 *
 * SQLite's `datetime('now')` writes `YYYY-MM-DD HH:MM:SS` with no zone, and it is **UTC** —
 * so it is read as UTC rather than handed to `Date.parse`, which would read it as local time
 * and shift every revision by the reader's offset. A string that states a zone is parsed
 * normally, because then it has already said what it means.
 */
export function parseInstant(at: string): number | null {
  const zoned = /(Z|[+-]\d{2}:?\d{2})$/.test(at.trim());
  if (zoned) {
    const parsed = Date.parse(at);
    return Number.isNaN(parsed) ? null : parsed;
  }
  const m = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2}))?/.exec(at.trim());
  if (!m) return null;
  const [, year, month, day, hour, minute, second] = m;
  return Date.UTC(+year, +month - 1, +day, +hour, +minute, +(second ?? '0'));
}

const UNITS: [ms: number, singular: string, plural: string][] = [
  [365 * 24 * 3_600_000, 'Jahr', 'Jahren'],
  [30 * 24 * 3_600_000, 'Monat', 'Monaten'],
  [7 * 24 * 3_600_000, 'Woche', 'Wochen'],
  [24 * 3_600_000, 'Tag', 'Tagen'],
  [3_600_000, 'Stunde', 'Stunden'],
  [60_000, 'Minute', 'Minuten']
];

/**
 * How long ago, in words.
 *
 * `now` is passed in rather than read from the clock, for two reasons. It makes the buckets
 * testable at all; and the page renders on the server and then hydrates in the browser, so
 * two different clocks would produce two different sentences for the same revision and a
 * hydration mismatch on a value nobody would think to suspect. The loader captures one
 * instant and both renders use it — the same reasoning as `formatInstant` in `$lib/adminApi`,
 * which avoids `Intl` for the same class of bug.
 *
 * A timestamp that cannot be read comes back unchanged. It is nonsense either way, and
 * nonsense a reader can see is better than "vor NaN Tagen".
 */
export function relativeTime(at: string, now: number): string {
  const then = parseInstant(at);
  if (then === null) return at;

  // A revision published "in the future" is a clock difference between two machines, not a
  // fact about the page. Rounding it to the present is the only honest rendering.
  const elapsed = Math.max(0, now - then);
  for (const [ms, singular, plural] of UNITS) {
    if (elapsed >= ms) {
      const count = Math.floor(elapsed / ms);
      return `vor ${count} ${count === 1 ? singular : plural}`;
    }
  }
  return 'gerade eben';
}

// --- Size -----------------------------------------------------------------------------

/**
 * How much bigger this revision made the page, against the one it was published on top of.
 *
 * `null` — rendered as "neu" — for the first revision of a page, and for a parent that is
 * not in the list. Falling back to "the whole size" in either case would report the page's
 * entire length as growth, which is exactly wrong for a restore of an old, short version.
 */
export function sizeDelta(revision: RevisionSummary, all: RevisionSummary[]): number | null {
  if (!revision.parent_id) return null;
  const parent = all.find((candidate) => candidate.id === revision.parent_id);
  return parent ? revision.byte_size - parent.byte_size : null;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1).replace('.', ',')} kB`;
}

/** `+200 B`, `−2,0 kB`, `±0`, or `neu` for the revision that started the page. */
export function formatDelta(delta: number | null): string {
  if (delta === null) return 'neu';
  if (delta === 0) return '±0';
  // U+2212 MINUS SIGN, not a hyphen: it aligns with the `+` and reads as arithmetic.
  return delta > 0 ? `+${formatBytes(delta)}` : `−${formatBytes(-delta)}`;
}

// --- Choosing what to compare ----------------------------------------------------------

/**
 * Which two revisions the page is comparing, given what the URL asked for.
 *
 * `revisions` is newest first, exactly as the API answers, so a larger index is an older
 * version — which is what decides who is `from` and who is `to` when somebody ticks the two
 * boxes the other way round. A diff always reads old → new; letting it read backwards would
 * report every addition as a removal.
 *
 * **An id that is not in this page's history is ignored**, never passed through. The API
 * would refuse a revision of another page anyway, but a frontend that forwards whatever was
 * in the query string is a frontend that asks the question — and "does this id exist" is not
 * a question this page should be putting to the server on a stranger's behalf.
 *
 * With nothing asked for, the newest revision is compared against the one it was published
 * on top of: the most recent change, which is what somebody opening a history came to see.
 */
export function selectPair(
  revisions: RevisionSummary[],
  von: string | null,
  bis: string | null
): { from: RevisionSummary | null; to: RevisionSummary | null } {
  const known = (id: string | null) =>
    id ? (revisions.find((revision) => revision.id === id) ?? null) : null;

  const a = known(von);
  const b = known(bis);

  if (a && b) {
    const older = revisions.indexOf(a) > revisions.indexOf(b) ? a : b;
    const newer = older === a ? b : a;
    return { from: older, to: newer };
  }

  const to = b ?? a ?? revisions[0] ?? null;
  if (!to) return { from: null, to: null };
  return { from: known(to.parent_id), to };
}
