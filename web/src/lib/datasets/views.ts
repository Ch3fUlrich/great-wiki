/**
 * The saved views of a dataset page (M8c C4), as the page reads them: the list from
 * `GET /api/datasets/views/{path}`, which address selects one, which field a board view groups
 * its cards by, and which columns a calendar view places its rows by. Pure — no fetch, no DOM.
 */
import type { CalendarConfig } from './calendar';
import type { GalleryConfig } from './gallery';

/** Mirrors one entry of the API's `{views: [...]}`: `config` is per-kind and stays unknown here. */
export interface DatasetView {
  id: string;
  name: string;
  kind: string;
  config: unknown;
  position: number;
}

export const VIEW_PARAM = 'view';

/** A board asks for all its cards in one read — it groups them, a page of them is no board. */
export const BOARD_ROW_LIMIT = 500;

const strip = (path: string) => path.replace(/^\/+/, '');

export const viewsApiPath = (path: string) => `/api/datasets/views/${strip(path)}`;

/**
 * The address of one view: `?view=ID`, or the bare path for the table. The table's href is
 * `'?'`, not `''`: an empty relative reference keeps the CURRENT query (RFC 3986 §5.4), so
 * `''` could never leave an active view. The same convention as `withoutFilterHref`.
 */
export function viewHref(id: string | null): string {
  return id ? `?${VIEW_PARAM}=${encodeURIComponent(id)}` : '?';
}

/** The view the address names; an unknown id is `null`, which is the table. */
export function pickView(views: DatasetView[], id: string | null): DatasetView | null {
  if (!id) return null;
  return views.find((v) => v.id === id) ?? null;
}

/** The field a picked board view groups by; `null` for a table view, a broken config, or no view. */
export function boardGroupKey(view: DatasetView | null): string | null {
  if (!view || view.kind !== 'board') return null;
  const raw = (view.config as { group_by?: unknown } | null)?.group_by;
  return typeof raw === 'string' && raw !== '' ? raw : null;
}

/** A config member that names a field, or nothing at all. */
const fieldKey = (value: unknown): string | null => (typeof value === 'string' && value !== '' ? value : null);

/**
 * The columns a picked calendar view places rows by; `null` for any other kind, a config with
 * nothing to place by, or no view. `end` and `title` are optional; `date` is not, and whether
 * the keys name real columns is the renderer's question, not this one.
 */
export function calendarConfig(view: DatasetView | null): CalendarConfig | null {
  if (!view || view.kind !== 'calendar') return null;
  const config = view.config as { date?: unknown; end?: unknown; title?: unknown } | null;
  const date = fieldKey(config?.date);
  if (!date) return null;
  return { date, end: fieldKey(config?.end), title: fieldKey(config?.title) };
}

/**
 * The columns a picked gallery view draws by (M8d D4b); `null` for any other kind, a config
 * with nothing to cover cards with, or no view. `cover` is required; whether the file it names
 * is a picture at all is `coverOf`'s question, decided from the attachment listing at read
 * time. A `card_fields` that is not a list, or names nothing usable, is simply no extra lines.
 */
export function galleryConfig(view: DatasetView | null): GalleryConfig | null {
  if (!view || view.kind !== 'gallery') return null;
  const config = view.config as { cover?: unknown; title?: unknown; card_fields?: unknown } | null;
  const cover = fieldKey(config?.cover);
  if (!cover) return null;
  const rawCards = config?.card_fields;
  const cardFields = Array.isArray(rawCards)
    ? rawCards.map(fieldKey).filter((key): key is string => key !== null)
    : [];
  return { cover, title: fieldKey(config?.title), card_fields: cardFields };
}
