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

/** One person, as `/api/me` names them. Mirrors `gw_api::view_as::PersonRef`. */
export interface PersonRef {
  id: string;
  username: string;
  display_name: string;
}

/**
 * The view-as mode (D-M2-17), when it is active.
 *
 * Both identities, because either alone misleads: `target` without `viewer` does not say
 * whose session this really is, and `viewer` without `target` does not say what is being
 * shown. Every other field of `Me` already describes the TARGET — that is who the
 * permission engine ran as — which is exactly why the banner has to be unmissable.
 */
export interface ViewAs {
  /** The administrator who is really signed in. */
  viewer: PersonRef;
  /** Whose view is being shown. */
  target: PersonRef;
}

/**
 * `/api/me`. Every field is derived server-side from the principal the permission engine
 * would use, so the header cannot show a name the API would not honour — and the reverse:
 * nothing here decides anything, it only reports what was already decided.
 */
export interface Me {
  authenticated: boolean;
  username: string | null;
  display_name: string | null;
  email: string | null;
  groups: string[];
  teams: string[];
  baseline: 'public' | 'internal' | 'admin';
  login_available: boolean;
  /**
   * What established this identity. `dev-shim` is `GW_DEV_IDENTITY` and has no session
   * behind it, so there is nothing for a sign-out to end.
   */
  source: 'session' | 'dev-shim' | 'anonymous';
  /**
   * Whose view is being shown, when an administrator is viewing as somebody else.
   * `null` is the ordinary case.
   */
  view_as: ViewAs | null;
}

/** What the interface shows when the API cannot be reached: nobody, with nothing. */
export const ANONYMOUS: Me = {
  authenticated: false,
  username: null,
  display_name: null,
  email: null,
  groups: [],
  teams: [],
  baseline: 'public',
  login_available: false,
  source: 'anonymous',
  view_as: null
};

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
