import { error } from '@sveltejs/kit';
import { apiGet, parseBody, type StoredDocument, type TreeNode } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, fetch, request }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<StoredDocument>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );

  if (status === 403) error(403, 'You do not have access to this page.');
  if (!data) error(404, 'Page not found.');

  const { data: tree } = await apiGet<TreeNode[]>(fetch, '/api/tree', cookie);
  return { doc: data, body: parseBody(data), tree: tree ?? [] };
};
