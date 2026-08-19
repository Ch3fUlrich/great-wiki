/**
 * Sorting and filtering for a rendered table — the logic only, no markup.
 *
 * It lives apart from `TableView.svelte` for two reasons. The first is that every rule
 * below is a JUDGEMENT about mixed content ("what is `>1200 ppm` next to `k. A.`?"), and a
 * judgement that cannot be tested cheaply gets changed by accident. The second is that the
 * component is a progressive enhancement — it renders the whole table on the server and
 * only then adds controls — so the logic has to be callable without a DOM.
 *
 * Everything here works on a GRID of plain strings (`textGrid`) and on ARRAYS OF ROW
 * INDICES, never on the blocks themselves. That is what makes sorting composable: the
 * component keeps one `order` for the whole table and derives the visible rows from it, so
 * a filter never disturbs a sort and a second sort inherits the first one's ties.
 */
import { plainText, type Block } from './render';

export type SortDirection = 'ascending' | 'descending';

export interface SortState {
  /** Column index into the header row. */
  column: number;
  direction: SortDirection;
}

export interface Column {
  index: number;
  /** What the sort button and the column's filter are named after. Never empty. */
  label: string;
  align: 'left' | 'center' | 'right' | undefined;
}

export interface FilterState {
  /** Searches every cell of a row. */
  query: string;
  /** One needle per column; an empty or blank entry constrains nothing. */
  columns: string[];
}

export interface TableSections {
  head: Block[];
  body: Block[];
}

/**
 * The number of body rows from which a table gets sort and filter controls.
 *
 * Six, and the reason is what the controls COST rather than what they give. A search box,
 * a reset button, a row count and one filter input per column is roughly a header's worth
 * of interface; on a table you can take in without scrolling it is noise in front of the
 * content, and this corpus is full of three-row tables that exist to state three facts.
 * Six rows is about where a reader stops holding the whole table in view at this type size
 * and starts hunting — sorting a table you can already see whole is work with no payoff.
 *
 * Measured against the corpus this was built for (21 tables, 3 to 26 body rows) it splits
 * them roughly in half, which is the outcome to aim for: neither "every table sprouts a
 * toolbar" nor "the feature only ever appears once".
 */
export const MIN_INTERACTIVE_ROWS = 6;

/// A row belongs in the `thead` when it holds header cells. The kind of the cell decides
/// it, so no row needs to know its position in the table.
const isHeaderRow = (row: Block) => (row.content ?? []).some((c) => c.kind === 'tableHeader');

/**
 * Split the leading run of header rows off the rest. Only the *leading* run: a header row
 * further down (a repeated head in a long table) stays where the author put it, because
 * moving it would reorder the document.
 */
export function sectionRows(rows: Block[]): TableSections {
  const first = rows.findIndex((row) => !isHeaderRow(row));
  return first < 0
    ? { head: rows, body: [] }
    : { head: rows.slice(0, first), body: rows.slice(first) };
}

/// The column's alignment, or `undefined` where the table states none — the renderer then
/// writes no `text-align` at all and the stylesheet's own default stands.
export function alignOf(cell: Block): Column['align'] {
  const value = cell.attrs?.align;
  return value === 'left' || value === 'center' || value === 'right' ? value : undefined;
}

/**
 * The columns, read off the header row.
 *
 * A column with no heading still gets a name ("Spalte 3"). A comparison table routinely
 * leaves its corner cell empty, and a sort button whose only content is that cell is
 * announced as "button" and nothing else.
 */
export function columns(headRow: Block): Column[] {
  return (headRow.content ?? []).map((cell, index) => ({
    index,
    label: plainText(cell).trim() || `Spalte ${index + 1}`,
    align: alignOf(cell)
  }));
}

/**
 * Whether this table gets controls at all.
 *
 * Beyond the row threshold there are two structural conditions, and both are about not
 * producing an unusable control rather than about taste:
 *
 *   - No header row means no column names, so every button and every filter would be
 *     unnamed — and `aria-sort` belongs on a `th` that does not exist.
 *   - Two or more header rows means the columns are grouped, and putting the controls on
 *     one of the rows would be a guess about which row the groups belong to. Rendering the
 *     table plainly is the honest answer.
 */
export function isInteractive({ head, body }: TableSections): boolean {
  return head.length === 1 && body.length >= MIN_INTERACTIVE_ROWS;
}

/**
 * The plain text of every cell, row by row. Sorting and filtering read nothing else.
 *
 * `plainText` separates nested BLOCKS with a space rather than concatenating them — but
 * not the inline leaves within one of them, which are one run of prose. That is not
 * cosmetic in either direction: this repository once produced `LängeMeter` from two cells
 * and the test for it asserted `contains "Feld"`, which `FeldWert` satisfies too; and a
 * cell reading `3-5 %` must not sort as `3 -5 %` because a mark split it in two.
 */
export function textGrid(rows: Block[]): string[][] {
  return rows.map((row) => (row.content ?? []).map((cell) => plainText(cell)));
}

// --- Comparison ---------------------------------------------------------------------
//
// The corpus these rules were written against holds ✅/❌ marks, quantities with units and
// comparators (`>1200 ppm`, `3-5%`, `<0.5%`), German prose with umlauts, and gaps. There
// is no single ordering that is right for all of it, so the rules are written down here
// and tested rather than left to `Array#sort`'s default (which is CODEPOINT order on
// stringified values, and would put every umlaut after Z and `1200` before `980`).

/** Cells that hold nothing to sort by. A lone dash is a table's way of writing "none". */
const NOTHING = new Set(['', '-', '‐', '–', '—', '−']);

/** Ticks and crosses, in the shapes this corpus and its neighbours actually use. */
const YES = new Set(['✅', '✔', '✓', '☑', '🗸']);
const NO = new Set(['❌', '❎', '✖', '✗', '✘', '☒']);

/**
 * German collation, made once. `Intl.Collator` is expensive to construct and this is
 * called O(n log n) times per sort.
 *
 * `de` rather than the default locale: codepoint order puts `Ä` after `Z`, and the machine
 * running this is not necessarily configured in German. `numeric: true` is the backstop
 * that keeps digits inside otherwise-textual cells sensible — "Phase 10" after "Phase 9" —
 * so a column this module declines to treat as numeric still reads correctly.
 */
const collator = new Intl.Collator('de', { numeric: true });

/**
 * The leading number of a cell, or `null`.
 *
 * Leading, deliberately. Taking the first number found ANYWHERE turns a strain name into a
 * number ("BB536"), and there is no way to tell that apart from a quantity by looking at
 * one cell — which is why the decision is made per COLUMN (`columnIsNumeric`) and this
 * function only answers "could this be read as a number".
 *
 * Comparator and approximation prefixes are read and then DISCARDED: `<0,5` and `0,5` both
 * parse as 0.5 and tie, and the stable sort then leaves them in the order they came in.
 * Ranking `<0,5` below `0,5` would be more precise and less predictable — nothing in the
 * cell says how far below.
 */
export function leadingNumber(text: string): number | null {
  const match = /^\s*(?:[<>≤≥≈~±]\s*)?(-|−)?\s*(\d[\d.,  ' ]*\d|\d)/.exec(text);
  if (!match) return null;
  const digits = parseDigitGroup(match[2]);
  if (digits === null) return null;
  return match[1] ? -digits : digits;
}

/**
 * A run of digits and separators, as a number.
 *
 * The hard case is that `.` and `,` swap roles between German and English and this corpus
 * is written in both: `1.200` is twelve hundred to a German author and 1.2 to an English
 * one. The rule below resolves it by SHAPE rather than by locale, which is the only signal
 * actually present in the string:
 *
 *   1.200        groups of exactly three  → thousands separators   → 1200
 *   1.234,56     both characters present  → the LAST one is the decimal point
 *   0.5 / 1,5    exactly one separator    → the decimal point
 *   1.2.3        anything else            → not a number (a version, a date, a citation)
 *
 * Spaces, non-breaking spaces and apostrophes are unambiguous group separators and are
 * simply removed first.
 */
function parseDigitGroup(group: string): number | null {
  const cleaned = group.replace(/[\s  ']/g, '');
  if (/^\d+$/.test(cleaned)) return Number(cleaned);

  if (/^\d{1,3}(?:\.\d{3})+$/.test(cleaned) || /^\d{1,3}(?:,\d{3})+$/.test(cleaned)) {
    return Number(cleaned.replace(/[.,]/g, ''));
  }

  const lastDot = cleaned.lastIndexOf('.');
  const lastComma = cleaned.lastIndexOf(',');
  const decimal = Math.max(lastDot, lastComma);
  const separators = (cleaned.match(/[.,]/g) ?? []).length;
  if (separators === 1) {
    return Number(cleaned.replace(/,/, '.'));
  }
  if (lastDot >= 0 && lastComma >= 0) {
    const head = cleaned.slice(0, decimal).replace(/[.,]/g, '');
    const tail = cleaned.slice(decimal + 1);
    return /^\d+$/.test(head) && /^\d+$/.test(tail) ? Number(`${head}.${tail}`) : null;
  }
  return null;
}

/** A ✅/❌ cell's value, or `null`. Only the first glyph is read, so `✅ (nur X)` counts. */
function tickValue(text: string): number | null {
  const first = Array.from(text.trim())[0];
  if (first === undefined) return null;
  if (YES.has(first)) return 1;
  if (NO.has(first)) return 0;
  return null;
}

/**
 * Whether a column should be compared as numbers.
 *
 * Decided per column and not per cell, because one cell cannot tell "5-HTP" from "5 mg".
 * More than half of the cells that hold anything at all must parse; that tolerates the
 * "k. A." or "siehe unten" that a real table always has somewhere, without letting a
 * single molecule name at the top of a prose column drag the whole column into numeric
 * order.
 *
 * Cells that then fail to parse are not lost — they fall through to text, and text sorts
 * after numbers (see `compareKeys`).
 */
export function columnIsNumeric(cells: string[]): boolean {
  let filled = 0;
  let numeric = 0;
  for (const cell of cells) {
    const text = cell.trim();
    if (NOTHING.has(text) || tickValue(text) !== null) continue;
    filled += 1;
    if (leadingNumber(text) !== null) numeric += 1;
  }
  return filled > 0 && numeric * 2 > filled;
}

type Key =
  | { kind: 'empty' }
  | { kind: 'number'; value: number }
  | { kind: 'text'; value: string };

function keyOf(text: string, numericColumn: boolean): Key {
  const trimmed = text.trim();
  if (NOTHING.has(trimmed)) return { kind: 'empty' };

  const tick = tickValue(trimmed);
  // Ticks are read in every column, numeric or not: a ✅ has no textual order worth
  // preserving, and ❌ < ✅ is the same convention a spreadsheet uses for FALSE < TRUE.
  // One click therefore groups the crosses, the other groups the ticks.
  if (tick !== null) return { kind: 'number', value: tick };

  if (numericColumn) {
    const value = leadingNumber(trimmed);
    if (value !== null) return { kind: 'number', value };
  }
  return { kind: 'text', value: trimmed };
}

/**
 * Ascending order between two keys of possibly different kinds.
 *
 * Numbers before text, because a quantity among labels is the exception and grouping the
 * exceptions together is more useful than interleaving them by their string form.
 * Emptiness is NOT handled here — see `compareCells`.
 */
function compareKeys(a: Key, b: Key): number {
  if (a.kind === 'number' && b.kind === 'number') return a.value - b.value;
  if (a.kind === 'number') return -1;
  if (b.kind === 'number') return 1;
  return collator.compare(
    a.kind === 'text' ? a.value : '',
    b.kind === 'text' ? b.value : ''
  );
}

/**
 * Compare two cells of one column, in the requested direction.
 *
 * EMPTY CELLS SORT LAST IN BOTH DIRECTIONS, which is why direction is applied here instead
 * of by negating the result outside. Reversing the whole comparator would put the gaps
 * first on the descending click, and a screenful of blank rows reads as "these are the
 * matches" — it pushes the rows the reader is actually hunting for off the bottom of the
 * table without saying so.
 */
export function compareCells(a: string, b: string, direction: SortDirection, numericColumn: boolean): number {
  const ka = keyOf(a, numericColumn);
  const kb = keyOf(b, numericColumn);
  if (ka.kind === 'empty' || kb.kind === 'empty') {
    if (ka.kind === kb.kind) return 0;
    return ka.kind === 'empty' ? 1 : -1;
  }
  return compareKeys(ka, kb) * (direction === 'ascending' ? 1 : -1);
}

/**
 * Re-order `order` by one column.
 *
 * Takes the CURRENT order rather than document order and sorts stably, so sorting by a
 * second column keeps the first column's order inside its ties — that is how a reader
 * builds a two-column sort without a multi-sort interface. `Array#sort` has been required
 * to be stable since ES2019, so nothing here has to decorate the rows to achieve it.
 */
export function sortOrder(
  grid: string[][],
  order: number[],
  column: number,
  direction: SortDirection
): number[] {
  const numericColumn = columnIsNumeric(grid.map((row) => row[column] ?? ''));
  return [...order].sort((a, b) =>
    compareCells(grid[a]?.[column] ?? '', grid[b]?.[column] ?? '', direction, numericColumn)
  );
}

/**
 * What the next activation of a column's sort button should produce.
 *
 * Ascending → descending → OFF, and off restores document order. A two-state toggle would
 * leave a reader with no way back to the order the author wrote, which in a document (as
 * opposed to a database view) is itself information.
 */
export function sortStateFor(current: SortState | null, column: number): SortState | null {
  if (current?.column !== column) return { column, direction: 'ascending' };
  if (current.direction === 'ascending') return { column, direction: 'descending' };
  return null;
}

// --- Filtering ----------------------------------------------------------------------

/**
 * The form a string is compared in: lower case, ß as ss, and diacritical marks removed.
 *
 * Folding the marks away is a deliberate asymmetry with SORTING, where the marks matter.
 * A filter is a search aid typed under time pressure, often on a keyboard whose umlauts
 * are awkward to reach, and "praparat" finding "Präparat" is what a reader expects from a
 * search box. Sorting is a presentation of the whole column, where a German reader expects
 * `Apfel` before `Äpfel` — so `compareCells` uses the collator instead.
 */
export function fold(text: string): string {
  return text
    .toLocaleLowerCase('de')
    .replace(/ß/g, 'ss')
    .normalize('NFD')
    .replace(/\p{M}/gu, '');
}

/**
 * Whether `haystack` satisfies `needle`, where the needle's whitespace-separated terms
 * must ALL appear, in any order.
 *
 * Term-wise rather than as one substring, because "rhamnosus ✅" is how a person narrows a
 * comparison table and "✅ rhamnosus" is the same question.
 */
export function matches(haystack: string, needle: string): boolean {
  const terms = fold(needle).split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const hay = fold(haystack);
  return terms.every((term) => hay.includes(term));
}

/**
 * Whether a row survives the filters. The table-wide query searches every cell; each
 * column filter searches only its own column; all of them must be satisfied.
 *
 * The row's cells are joined with a space for the table-wide search, so no term can match
 * across a cell boundary — a match a reader cannot point at on screen is a lie about what
 * was found.
 */
export function rowMatches(row: string[], filter: FilterState): boolean {
  if (!matches(row.join(' '), filter.query)) return false;
  return filter.columns.every(
    (needle, index) => !needle.trim() || matches(row[index] ?? '', needle)
  );
}

/** The rows to show, in the current sort order. */
export function visibleOrder(grid: string[][], order: number[], filter: FilterState): number[] {
  return order.filter((index) => rowMatches(grid[index] ?? [], filter));
}

// --- What the reader is told --------------------------------------------------------

/**
 * The row count, ALWAYS as displayed-out-of-total.
 *
 * Never the bare number of visible rows, and never hidden when nothing is filtered. A
 * table that has silently dropped rows looks exactly like a complete one, which is the
 * class of defect this project keeps finding; the denominator is the only thing that tells
 * a reader the difference.
 */
export function rowCountLabel(visible: number, total: number): string {
  return `${visible} von ${total} Zeilen`;
}

/** "sortiert nach Dosis, aufsteigend" — or "" when the table is in document order. */
export function sortAnnouncement(sort: SortState | null, cols: Column[]): string {
  if (!sort) return '';
  const label = cols[sort.column]?.label ?? `Spalte ${sort.column + 1}`;
  const direction = sort.direction === 'ascending' ? 'aufsteigend' : 'absteigend';
  return `sortiert nach ${label}, ${direction}`;
}

/**
 * What activating a column's sort button will DO, for the button's accessible name.
 *
 * The current state is carried by `aria-sort` on the header cell; this is the other half —
 * a button named only after its column tells a screen-reader user nothing about what
 * pressing it does, and the answer changes with every press.
 */
export function sortActionLabel(sort: SortState | null, column: number): string {
  if (sort?.column !== column) return 'aufsteigend sortieren';
  return sort.direction === 'ascending' ? 'absteigend sortieren' : 'Sortierung aufheben';
}

/** The `aria-sort` value for one header cell. Every sortable column states one. */
export function ariaSort(sort: SortState | null, column: number): SortDirection | 'none' {
  return sort?.column === column ? sort.direction : 'none';
}
