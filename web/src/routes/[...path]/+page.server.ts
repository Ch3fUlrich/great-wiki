import { error } from '@sveltejs/kit';
import { apiGet, parseBody, type Backlink, type StoredDocument, type TreeNode } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, fetch, request, url }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<StoredDocument>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );

  if (status === 403) error(403, 'You do not have access to this page.');
  if (!data) error(404, 'Page not found.');

  const { data: tree } = await apiGet<TreeNode[]>(fetch, '/api/tree', cookie);

  // Own endpoint, own prefix (`/api/links/backlinks/{*path}`, not a suffix under
  // `/api/documents`) — see `gw-api/src/routes/links.rs` for why. Already filtered to what
  // THIS caller may read; `data` ?? [] only for a request that failed outright (network,
  // 5xx), never as a second permission filter — the store already carried that out.
  const { data: backlinks } = await apiGet<{ backlinks: Backlink[] }>(
    fetch,
    `/api/links/backlinks/${params.path}`,
    cookie
  );

  // Read here rather than from `$app/state` in the component, for two reasons: the flag is
  // then part of the page's data and a server-render test can set it, and the component
  // does not have to reach for a SvelteKit runtime that only exists inside a request.
  //
  // It asks for the editor; it does not decide anything. Whether this caller may actually
  // write is settled by the collaboration socket, which is the only thing that knows.
  return {
    doc: data,
    body: parseBody(data),
    tree: tree ?? [],
    backlinks: backlinks?.backlinks ?? [],
    edit: url.searchParams.get('edit') === '1'
  };
};
