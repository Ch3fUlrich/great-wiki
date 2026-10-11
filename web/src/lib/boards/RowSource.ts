/**
 * The seam one generic board is fed through (M8c, spec §5): dataset rows (DatasetRowSource)
 * and task cards (TaskRowSource, C3) both become BoardCards, so Board.svelte has one
 * component and two sources. Pure types plus `groupCards`; no fetch, no DOM.
 */

export interface BoardColumn {
  /** `null` is the "no value" column; sources carry it last. */
  id: string | null;
  label: string;
}

export interface BoardCard {
  id: string;
  title: string;
  column: string | null;
  version: number | null;
  meta?: Record<string, unknown>;
}

export type MoveResult =
  | { kind: 'moved'; card: BoardCard }
  | { kind: 'stale'; current: BoardCard | null }
  | { kind: 'refused'; message: string }
  | { kind: 'failed'; message: string };

export interface RowSource {
  columns(): BoardColumn[];
  cards(): BoardCard[];
  canMove(card: BoardCard): boolean;
  move(card: BoardCard, column: string | null): Promise<MoveResult>;
}

/** Cards per column: every column present (empty if no card), source order kept, a card
 * whose column id the source does not list lands in the `null` column — never dropped. */
export function groupCards(source: RowSource): Map<string | null, BoardCard[]> {
  const groups = new Map<string | null, BoardCard[]>();
  for (const col of source.columns()) groups.set(col.id, []);
  for (const c of source.cards()) {
    const key = c.column !== null && groups.has(c.column) ? c.column : null;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key)!.push(c);
  }
  return groups;
}
