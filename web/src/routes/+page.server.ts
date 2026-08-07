import { apiGet, type TreeNode } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, request }) => {
  const { data } = await apiGet<TreeNode[]>(fetch, '/api/tree', request.headers.get('cookie'));
  return { tree: data ?? [] };
};
