import { env } from '$env/dynamic/private';
import type { Block } from '$lib/blocks/render';

export interface TreeNode {
  path: string;
  slug: string;
  title: string;
  doc_type: string;
  visibility: string;
  children: TreeNode[];
}

export interface StoredDocument {
  id: string;
  path: string;
  parent_path: string | null;
  slug: string;
  doc_type: string;
  title: string;
  language: string;
  visibility: string;
  body: string; // JSON-encoded Block tree
  sort_key: number;
}

// `$env/dynamic/private` is server-only, which is correct: this module is imported only
// from `+page.server.ts` files and must never end up in a client bundle.
const BASE = env.GW_API ?? 'http://127.0.0.1:8092';

/**
 * Server-side fetch. Forwards the caller's cookie so the API sees the same session the
 * browser has — and forwards nothing else from the client request.
 */
export async function apiGet<T>(
  fetchFn: typeof fetch,
  path: string,
  cookie: string | null
): Promise<{ status: number; data: T | null }> {
  const res = await fetchFn(`${BASE}${path}`, {
    headers: cookie ? { cookie } : {}
  });
  if (!res.ok) return { status: res.status, data: null };
  return { status: res.status, data: (await res.json()) as T };
}

export function parseBody(doc: StoredDocument): Block {
  return JSON.parse(doc.body) as Block;
}
