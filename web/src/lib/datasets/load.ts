import { apiGet } from '$lib/api';
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
    return { fields, page: rows.data, error: null, sort, filter };
  } catch {
    return { ...fail(0), fields, sort, filter };
  }
}

function fail(status: number): DatasetLoad {
  return { fields: [], page: null, error: describeRowsFailure(status), sort: null, filter: null };
}
