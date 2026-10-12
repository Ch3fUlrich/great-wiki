import { apiGet } from '$lib/api';
import { attachmentsApiPath } from '$lib/attachments';
import { labelMap, relationConfig } from './cells/relation';
import type { GalleryAttachment } from './gallery';
import {
  describeRowsFailure,
  fieldsApiPath,
  filterFrom,
  rowsApiPath,
  sortFrom,
  type Field,
  type FilterState,
  type RowPage,
  type SortState
} from './table';
import {
  BOARD_ROW_LIMIT,
  boardGroupKey,
  calendarConfig,
  galleryConfig,
  pickView,
  viewsApiPath,
  VIEW_PARAM,
  type DatasetView
} from './views';

export interface DatasetLoad {
  fields: Field[];
  page: RowPage | null;
  error: string | null;
  sort: SortState | null;
  filter: FilterState | null;
  /** `fieldKey -> target rowId -> label`, one page per relation column; absent on failure. */
  labels: Record<string, Record<string, string>>;
  /** The dataset's saved views (M8c C4); `[]` when the list could not be read. */
  views: DatasetView[];
  /** The view the address names, or `null` for the table. An unknown id is `null`. */
  view: DatasetView | null;
  /**
   * The page's attachment listing, read only for a picked gallery view (M8d D4b) — it is what
   * decides whether a cover file is a picture at all. `[]` for any other view, and `[]` when
   * the listing could not be read: the gallery then states a placeholder on every card rather
   * than the page failing.
   */
  attachments: GalleryAttachment[];
}

/**
 * A dataset page's schema, saved views and first page of rows, for the view, sort and
 * filter in the address.
 *
 * Sort and filter are validated against the dataset's own columns before they are sent, so
 * an address cannot name a key the API would have to refuse. A failed read is STATED
 * (`error`), never rendered as an empty table.
 *
 * A picked BOARD or CALENDAR view changes the rows request — all its rows at once, and the
 * address's sort and filter are the table's, not it — and for that the view list has to be read
 * BEFORE the rows. With no `?view` in the address nothing the list can say changes the request
 * (`pickView` of null is null), so it is read after the rows instead.
 */
export async function loadDataset(
  fetchFn: typeof fetch,
  path: string,
  params: URLSearchParams,
  cookie: string | null
): Promise<DatasetLoad> {
  let status = 0;
  let fields: Field[] = [];
  try {
    const schema = await apiGet<{ fields: Field[] }>(fetchFn, fieldsApiPath(path), cookie);
    status = schema.status;
    fields = schema.data?.fields ?? [];
    if (!schema.data) return fail(status);
  } catch {
    return fail(0);
  }
  const requested = params.get(VIEW_PARAM);
  const early = requested ? await readViews(fetchFn, path, cookie) : [];
  const view = pickView(early, requested);
  // A board, a calendar and a gallery are all drawn from every row at once: a page of a board
  // is no board, a page of a calendar is a month with holes in it, and a page of a gallery is
  // half the pictures. Their address carries neither a sort nor a filter — those are the
  // table's controls, and the rows they picked are not on this page.
  const whole =
    boardGroupKey(view) !== null || calendarConfig(view) !== null || galleryConfig(view) !== null;
  const sort = whole ? null : sortFrom(params.get('sort'), params.get('desc'), fields);
  const raw =
    params.get('f') ??
    (params.get('fk') && params.get('fv') ? `${params.get('fk')}:${params.get('fv')}` : null);
  const filter = whole ? null : filterFrom(raw, fields);
  try {
    const rows = await apiGet<RowPage>(
      fetchFn,
      rowsApiPath(path, whole ? { limit: BOARD_ROW_LIMIT } : { sort: sort?.sort, desc: sort?.desc, filter }),
      cookie
    );
    if (!rows.data) return { ...fail(rows.status), fields, sort, filter, views: early, view };
    const labels = await relationLabels(fetchFn, fields, cookie);
    const views = requested ? early : await readViews(fetchFn, path, cookie);
    // A gallery's covers are decided by the page's attachment listing (M8d D4b), and nothing
    // else on this page asks for it — the ordinary `Anhänge` list is read by the document's
    // own loader, not by the dataset's.
    const attachments =
      galleryConfig(view) !== null ? await readAttachments(fetchFn, path, cookie) : [];
    return { fields, page: rows.data, error: null, sort, filter, labels, views, view, attachments };
  } catch {
    return { ...fail(0), fields, sort, filter, views: early, view };
  }
}

/**
 * The saved views. A list that cannot be read is simply NO list: the table this page always
 * was must not go missing because the view endpoint did.
 */
async function readViews(
  fetchFn: typeof fetch,
  path: string,
  cookie: string | null
): Promise<DatasetView[]> {
  try {
    const res = await apiGet<{ views?: DatasetView[] }>(fetchFn, viewsApiPath(path), cookie);
    const list = res.data?.views;
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

/**
 * The page's attachment listing, for a picked gallery's covers (M8d D4b). A listing that
 * cannot be read is simply NO listing — the gallery then states a placeholder on every card,
 * which is a smaller claim than the page failing over a picture.
 */
async function readAttachments(
  fetchFn: typeof fetch,
  path: string,
  cookie: string | null
): Promise<GalleryAttachment[]> {
  try {
    const res = await apiGet<{ attachments?: GalleryAttachment[] }>(
      fetchFn,
      attachmentsApiPath(path),
      cookie
    );
    const list = res.data?.attachments;
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

function fail(status: number): DatasetLoad {
  return {
    fields: [],
    page: null,
    error: describeRowsFailure(status),
    sort: null,
    filter: null,
    labels: {},
    views: [],
    view: null,
    attachments: []
  };
}

/**
 * Labels for the rows the relation columns point at: each visible target's first page, read
 * once — two columns onto one target share the read. A target that cannot be read simply gets
 * no map, and the table falls back to counting ids; the page is not an error because of it.
 * The column itself is only in the schema when the caller may read its target, so this asks
 * nothing the caller could not already see.
 */
async function relationLabels(
  fetchFn: typeof fetch,
  fields: Field[],
  cookie: string | null
): Promise<DatasetLoad['labels']> {
  const labels: DatasetLoad['labels'] = {};
  const byPath = new Map<string, Record<string, string> | null>();
  for (const f of fields) {
    if (f.kind !== 'relation') continue;
    const cfg = relationConfig(f.config);
    if (!cfg) continue;
    if (!byPath.has(cfg.targetPath)) {
      try {
        const res = await apiGet<RowPage>(fetchFn, rowsApiPath(cfg.targetPath, {}), cookie);
        byPath.set(cfg.targetPath, res.data ? labelMap(res.data.rows) : null);
      } catch {
        byPath.set(cfg.targetPath, null);
      }
    }
    const m = byPath.get(cfg.targetPath);
    if (m) labels[f.key] = m;
  }
  return labels;
}
