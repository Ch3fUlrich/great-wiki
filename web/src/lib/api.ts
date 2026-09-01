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

/**
 * One page that links to the page being read. Mirrors
 * `gw_api::routes::links::BacklinkView` — `path` and `title` only, never the linking
 * document's id, which has no reason to leave the API.
 */
export interface Backlink {
  path: string;
  title: string;
}

/**
 * One page in the graph. Mirrors `gw_api::routes::links::GraphNodeView`.
 *
 * `path` is the identity: the API translates the store's document ids into paths on the way
 * out, so nothing here needs a lookup table and no internal id reaches the browser.
 */
export interface GraphNode {
  path: string;
  title: string;
}

/** One link, naming both ends by path. Mirrors `gw_api::routes::links::GraphEdgeView`. */
export interface GraphEdge {
  from: string;
  to: string;
}

/**
 * `/api/links/graph`. Already filtered to what the caller may read — an edge is on the wire
 * only when BOTH its ends are readable, and a node only when an edge survived to touch it.
 * See `gw-store/src/links.rs`. Nothing in this interface filters it again.
 */
export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
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

/**
 * One page as `GET /api/documents/{path}` answers it: the stored document, **and whether the
 * caller may write it.**
 *
 * Mirrors `gw_api::routes::docs::DocumentView`, which flattens the document into the response
 * rather than nesting it — so this is `StoredDocument` plus one key, and every reader that
 * only wanted the document is unchanged.
 *
 * `may_write` is not computed anywhere in this interface. It is the verdict the very
 * authorisation that produced this response reached — the same `permits()` answer a write
 * would get — so a control offered on it and the refusal it would receive cannot come apart.
 * It is optional here for the reason `BoardTask::may_write` is: a response from an older API
 * carries no such field, and each reader below says what it does with that absence.
 *
 * **What it licenses**: opening the editor and saving what is typed, making the page a
 * project's home, changing a card the page governs, and re-filing the page under a topic.
 * **Filing a revision needs one thing more** — a signed-in, active account, because a revision
 * records an author — so a control that publishes composes this with `authenticated` from
 * `/api/me`. Re-filing does not: `Store::set_document_topics` writes no revision. See
 * ADR 0010.
 */
export interface DocumentView extends StoredDocument {
  may_write?: boolean;
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

/**
 * What a refused write said about itself.
 *
 * `gw_api::error::ApiError` renders every refusal as `{"error": "…"}`, and dropping that
 * string is how a 409 that names a way out becomes a bare "Fehler". Callers that have their
 * own wording for a status ignore it; callers that do not append it rather than lose it.
 */
export interface ApiFailure {
  status: number;
  message: string | null;
}

/**
 * The writing half of {@link apiGet}: a server-side POST, PATCH or DELETE, carrying the same
 * cookie and the same proxy attestation.
 *
 * It shares `base()` and `proxySecret()` with `apiGet` deliberately and must go on doing so.
 * A second spelling of the attestation is precisely the defect `api.test.ts` was written
 * about — every server-side call refused with 403, every page rendering as "nobody signed
 * in", and nothing in any log a reader would see to say why.
 *
 * Used from **form actions**, where it is the whole point: a form that posts to the server
 * works before hydration, and the browser's own submission is what carries the cookie.
 */
export async function apiSend<T>(
  fetchFn: typeof fetch,
  method: 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  path: string,
  cookie: string | null,
  body?: unknown
): Promise<{ status: number; data: T | null; failure: ApiFailure | null }> {
  const headers: Record<string, string> = {};
  if (cookie) headers.cookie = cookie;
  const secret = proxySecret();
  if (secret) headers['X-GW-Proxy'] = secret;
  if (body !== undefined) headers['content-type'] = 'application/json';

  let res: Response;
  try {
    res = await fetchFn(`${base()}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  } catch {
    // Status 0 is "no answer at all" — offline, DNS, a dead proxy. Kept apart from 5xx
    // because "not reachable" and "answered with 500" send somebody to different places,
    // the same split `$lib/adminApi` makes.
    return { status: 0, data: null, failure: { status: 0, message: null } };
  }

  const text = await res.text();
  if (!res.ok) {
    let message: string | null = null;
    try {
      message = (JSON.parse(text) as { error?: string }).error ?? null;
    } catch {
      // A refusal that is not JSON says nothing this can use, and inventing one would be
      // worse than the status code on its own.
    }
    return { status: res.status, data: null, failure: { status: res.status, message } };
  }

  // A 204 and an empty body are both legitimate for a DELETE.
  const data = text ? (JSON.parse(text) as T) : null;
  return { status: res.status, data, failure: null };
}

export function parseBody(doc: StoredDocument): Block {
  return JSON.parse(doc.body) as Block;
}
