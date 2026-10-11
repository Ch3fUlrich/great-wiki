/**
 * A dataset as a RowSource (M8c): the select field named by group_key gives the columns,
 * a drag is one row PATCH through the same `Send` the row editor uses (ADR 0029).
 * A reader without write gets `refused` before any request is made.
 */
import { optionsOf } from '../datasets/cells/edit';
import { describeSaveFailure, type Send } from '../datasets/rowedit';
import { rowsApiPath, type Field, type Row } from '../datasets/table';
import type { BoardCard, BoardColumn, MoveResult, RowSource } from './RowSource';

export function DatasetRowSource(
  fields: Field[],
  rows: Row[],
  groupKey: string,
  send: Send,
  path: string,
  writable: boolean
): RowSource {
  const group = fields.find((f) => f.key === groupKey);
  if (!group || group.kind !== 'select') throw new Error('group_by must be a select field of the dataset');
  const titleField = fields.filter((f) => f.kind === 'text').sort((a, b) => a.position - b.position)[0];

  const cardOf = (row: Row): BoardCard => {
    const raw = titleField ? row.values?.[titleField.key] : null;
    return {
      id: row.id,
      title: typeof raw === 'string' && raw !== '' ? raw : row.id,
      column: typeof row.values?.[groupKey] === 'string' ? (row.values[groupKey] as string) : null,
      version: row.version
    };
  };

  return {
    columns(): BoardColumn[] {
      return [
        ...optionsOf(group.config).map((o) => ({ id: o.id, label: o.label })),
        { id: null, label: 'Ohne Wert' }
      ];
    },
    cards: () => rows.map(cardOf),
    canMove: () => writable,
    async move(card: BoardCard, column: string | null): Promise<MoveResult> {
      if (!writable) return { kind: 'refused', message: 'Sie dürfen diese Zeile nicht ändern.' };
      const answer = await send('PATCH', rowsApiPath(path, {}), {
        id: card.id,
        version: card.version,
        values: { [groupKey]: column }
      });
      if (answer.status >= 200 && answer.status < 300) {
        const v = (answer.body as { version?: unknown } | null)?.version;
        return { kind: 'moved', card: { ...card, column, version: typeof v === 'number' ? v : card.version } };
      }
      if (answer.status === 409) {
        const cur = (answer.body as { current?: Row } | null)?.current;
        return { kind: 'stale', current: cur ? cardOf(cur) : null };
      }
      return { kind: 'failed', message: describeSaveFailure(answer.status, answer.message) };
    }
  };
}
