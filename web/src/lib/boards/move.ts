/**
 * The board's move logic, pure (M8c): drop and select both come here, so there is one code
 * path. Never mutates the given view; a refused, failed or stale move changes no column.
 */
import type { BoardCard, MoveResult, RowSource } from './RowSource';

export type BoardView = {
  /** card id -> current column id; `null` is the "no value" column. */
  columnOf: Map<string, string | null>;
  message: string | null;
};

export function viewFromCards(cards: BoardCard[]): BoardView {
  return { columnOf: new Map(cards.map((c) => [c.id, c.column])), message: null };
}

export async function applyMove(
  source: RowSource,
  view: BoardView,
  card: BoardCard,
  column: string | null,
  hooks?: { onMoved?(c: BoardCard): void; onStale?(cur: BoardCard | null): void }
): Promise<BoardView> {
  const current = view.columnOf.get(card.id) ?? null;
  if (column === current || !source.canMove(card)) return view;
  const result: MoveResult = await source.move(card, column);
  switch (result.kind) {
    case 'moved': {
      const columnOf = new Map(view.columnOf);
      columnOf.set(card.id, result.card.column);
      hooks?.onMoved?.(result.card);
      return { columnOf, message: null };
    }
    case 'stale':
      hooks?.onStale?.(result.current);
      return { ...view, message: 'Die Zeile wurde inzwischen geändert; bitte neu laden.' };
    default:
      return { ...view, message: result.message };
  }
}
