/**
 * A task board as a RowSource (M8c C3): the three D-9 statuses give the columns, a card is
 * one BoardTask, and a drag is one `submit(taskId, status)` — the caller owns the request.
 * The read-only verdict comes from `readOnly` in `$lib/board`, so a card that cannot be moved
 * here is exactly the card the API would refuse; a move is refused in-source before `submit`
 * is ever called.
 */
import type { Me } from '$lib/api';
import {
  columnsOf,
  isStatus,
  readOnly,
  STATUSES,
  STATUS_LABEL,
  type BoardResponse,
  type BoardTask,
  type TaskStatus
} from '$lib/board';
import type { BoardCard, BoardColumn, MoveResult, RowSource } from './RowSource';

const taskOf = (card: BoardCard): BoardTask => card.meta!.task as BoardTask;

export function TaskRowSource(
  board: BoardResponse,
  me: Me | null,
  submit: (taskId: string, status: TaskStatus) => Promise<void>
): RowSource {
  const canMove = (card: BoardCard): boolean => readOnly(taskOf(card), me) === null;

  return {
    columns(): BoardColumn[] {
      return STATUSES.map((status) => ({ id: status, label: STATUS_LABEL[status] }));
    },
    cards(): BoardCard[] {
      return columnsOf(board).flatMap((column) =>
        column.tasks.map((task) => ({
          id: task.id,
          title: task.title,
          column: task.status,
          version: null,
          meta: { task }
        }))
      );
    },
    canMove,
    async move(card: BoardCard, column: string | null): Promise<MoveResult> {
      if (column === null || !isStatus(column)) {
        return { kind: 'refused', message: 'Zielstatus fehlt.' };
      }
      if (!canMove(card)) {
        return { kind: 'refused', message: 'Diese Karte kann von hier nicht verschoben werden.' };
      }
      try {
        await submit(card.id, column);
        return { kind: 'moved', card: { ...card, column } };
      } catch {
        return { kind: 'failed', message: 'Die Aufgabe konnte nicht verschoben werden.' };
      }
    }
  };
}
