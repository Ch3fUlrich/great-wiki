import { apiGet } from '$lib/api';
import { labelMap, relationConfig } from './cells/relation';
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

export interface DatasetLoad {
  fields: Field[];
  page: RowPage | null;
  error: string | null;
  sort: SortState | null;
  filter: FilterState | null;
  /** `fieldKey -> target rowId -> label`, one page per relation column; absent on failure. */
  labels: Record<string, Record<string, string>>;
}

/**
 * A dataset page's schema and first page of rows, for the sort and filter in the address.
 *
 * Sort and filter are validated against the dataset's own columns before they are sent, so
 * an address cannot name a key the API would have to refuse. A failed read is STATED
 * (`error`), never rendered as an empty table.
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
  const sort = sortFrom(params.get('sort'), params.get('desc'), fields);
  const raw =
    params.get('f') ??
    (params.get('fk') && params.get('fv') ? `${params.get('fk')}:${params.get('fv')}` : null);
  const filter = filterFrom(raw, fields);
  try {
    const rows = await apiGet<RowPage>(
      fetchFn,
      rowsApiPath(path, { sort: sort?.sort, desc: sort?.desc, filter }),
      cookie
    );
    if (!rows.data) return { ...fail(rows.status), fields, sort, filter };
    const labels = await relationLabels(fetchFn, fields, cookie);
    return { fields, page: rows.data, error: null, sort, filter, labels };
  } catch {
    return { ...fail(0), fields, sort, filter };
  }
}

function fail(status: number): DatasetLoad {
  return { fields: [], page: null, error: describeRowsFailure(status), sort: null, filter: null, labels: {} };
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
