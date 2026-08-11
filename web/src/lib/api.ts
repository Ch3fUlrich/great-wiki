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
//
// Read per call rather than once at import. `$env/dynamic/private` exists precisely to be
// read at runtime — that is what separates it from `$env/static/private` — and a value
// captured when the module first loads is a value that cannot be tested, because a test
// cannot import the module twice with two environments. Both readers below are one
// property lookup; nothing here is on a hot path.
const base = () => env.GW_API ?? 'http://127.0.0.1:8092';

/**
 * The proxy attestation, for calls this server makes itself.
 *
 * The API refuses any request that arrives without it whenever it is bound to anything but
 * loopback, because its port is reachable from the LAN and the shared secret is the only
 * thing separating a request that came through the edge from one that did not. That guard
 * applies to *this* server too: rendering a page is an ordinary HTTP call to the API, and
 * nothing about running in the same compose project makes it attested.
 *
 * It was missing until 2026-08-11, and the way it failed is the reason this comment is
 * long. Every server-side call would have been refused with 403, `+layout.server.ts` turns
 * a failed `/api/me` into `ANONYMOUS` on purpose, and the result is a wiki that renders as
 * "nobody signed in" for everybody — no crash, no error in any log the reader would see,
 * just a site that quietly shows the public view to people who are signed in. It never
 * reached production only because nothing had been deployed yet.
 *
 * Empty in development, where the API binds loopback and demands nothing.
 */
const proxySecret = () => env.GW_PROXY_SECRET ?? '';

/**
 * Server-side fetch. Forwards the caller's cookie so the API sees the same session the
 * browser has — and forwards nothing else from the client request.
 */
export async function apiGet<T>(
  fetchFn: typeof fetch,
  path: string,
  cookie: string | null
): Promise<{ status: number; data: T | null }> {
  const headers: Record<string, string> = {};
  if (cookie) headers.cookie = cookie;
  const secret = proxySecret();
  if (secret) headers['X-GW-Proxy'] = secret;

  const res = await fetchFn(`${base()}${path}`, { headers });
  if (!res.ok) return { status: res.status, data: null };
  return { status: res.status, data: (await res.json()) as T };
}

export function parseBody(doc: StoredDocument): Block {
  return JSON.parse(doc.body) as Block;
}
