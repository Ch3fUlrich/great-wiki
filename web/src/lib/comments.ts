import * as Y from 'yjs';

/**
 * Client for page comments (`gw_api::routes::comments`, ADR 0025).
 *
 * Anyone who may read a page may read and write its comments, so nothing here decides
 * permission; the API answers 404 for a page the caller may not read. There is no delete:
 * a comment is resolved (collapsed, kept) or orphaned (kept, flagged), never removed.
 *
 * Browser-safe: it must not import `$lib/api` (which reads a server-only env). The page
 * loader reads the list with `apiGet` and {@link commentsApiPath}.
 */
export interface CommentAnchor {
  start: string;
  end: string;
  quote: string;
}

export interface Comment {
  id: string;
  parent_id: string | null;
  author_name: string;
  body: string;
  anchor: CommentAnchor | null;
  orphaned: boolean;
  resolved: boolean;
  resolved_at: string | null;
  created_at: string;
}

export interface Thread extends Comment {
  replies: Comment[];
}

export const MAX_QUOTE_CHARS = 200;
export const MAX_BODY_CHARS = 8000;

type Fetch = typeof fetch;

function encodeSegments(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

/** `/rundgang/tabellen` → `/api/comments/document/rundgang/tabellen`. */
export function commentsApiPath(path: string): string {
  return `/api/comments/document/${encodeSegments(path.replace(/^\/+/, ''))}`;
}

export function commentActionPath(id: string, action: 'resolve' | 'reopen'): string {
  return `/api/comments/${encodeURIComponent(id)}/${action}`;
}

/** German line for a failed list request. */
export function describeCommentsFailure(status: number): string {
  if (status === 0) return 'Die Kommentare konnten nicht geladen werden: keine Antwort.';
  return `Die Kommentare konnten nicht geladen werden (Fehler ${status}).`;
}

export interface Grouped {
  /** Not resolved. Orphaned ones stay here: the thread is open, only its passage is gone. */
  open: Thread[];
  /** Resolved; shown collapsed. */
  resolved: Thread[];
}

/** Open and resolved threads, each in the order the API returned them. */
export function groupThreads(threads: Thread[]): Grouped {
  const open: Thread[] = [];
  const resolved: Thread[] = [];
  for (const t of threads) (t.resolved ? resolved : open).push(t);
  return { open, resolved };
}

export const ORPHAN_NOTE = 'Textstelle nicht mehr vorhanden';

/** The quote to show with a thread, and whether the passage is gone. */
export function passageOf(t: Comment): { quote: string; orphaned: boolean } | null {
  if (!t.anchor) return null;
  return { quote: t.anchor.quote, orphaned: t.orphaned };
}

/** A body that is worth sending: trimmed, non-empty, within the server's limit. */
export function cleanBody(raw: string): string | null {
  const body = raw.trim();
  if (!body || Array.from(body).length > MAX_BODY_CHARS) return null;
  return body;
}

/** The selected text as a quote: whitespace collapsed, cut at the server's limit. */
export function quoteOf(selected: string): string {
  return Array.from(selected.replace(/\s+/g, ' ').trim()).slice(0, MAX_QUOTE_CHARS).join('');
}

function toBase64(bytes: Uint8Array): string {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
}

export function fromBase64(text: string): Uint8Array {
  const s = atob(text);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

/**
 * The anchor's wire form (`gw-collab/src/anchor.rs`): two Yjs v1 relative positions in
 * base64. Start sticks to the character after (assoc 0), end to the one before (assoc -1),
 * so text typed at either edge falls outside the passage. This is the whole Yjs part.
 *
 * The positions given may carry any association (the editor binding picks its own), so each
 * is resolved to its (type, index) and minted again with the association the wire needs;
 * changing `assoc` on the old value would move it to a different character.
 */
export function encodeAnchor(
  doc: Y.Doc,
  start: Y.RelativePosition,
  end: Y.RelativePosition,
  quote: string
): CommentAnchor | null {
  const s = Y.createAbsolutePositionFromRelativePosition(start, doc);
  const e = Y.createAbsolutePositionFromRelativePosition(end, doc);
  if (!s || !e) return null;
  return {
    start: toBase64(
      Y.encodeRelativePosition(Y.createRelativePositionFromTypeIndex(s.type, s.index, 0))
    ),
    end: toBase64(
      Y.encodeRelativePosition(Y.createRelativePositionFromTypeIndex(e.type, e.index, -1))
    ),
    quote: quoteOf(quote)
  };
}

export interface Sent<T = unknown> {
  status: number;
  data: T | null;
}

async function send<T>(
  fetchFn: Fetch,
  method: 'GET' | 'POST',
  url: string,
  body?: unknown
): Promise<Sent<T>> {
  try {
    const res = await fetchFn(url, {
      method,
      headers: body === undefined ? undefined : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    let data: T | null = null;
    if (res.ok) data = (await res.json().catch(() => null)) as T | null;
    return { status: res.status, data };
  } catch {
    return { status: 0, data: null };
  }
}

/** Browser-side: re-read the threads. */
export const fetchThreads = (fetchFn: Fetch, path: string) =>
  send<{ threads: Thread[] }>(fetchFn, 'GET', commentsApiPath(path));

/** Browser-side: a page-level comment, a reply (`parentId`) or a passage comment (`anchor`). */
export const postComment = (
  fetchFn: Fetch,
  path: string,
  input: { body: string; parentId?: string; anchor?: CommentAnchor }
) =>
  send(fetchFn, 'POST', commentsApiPath(path), {
    body: input.body,
    parent_id: input.parentId ?? null,
    anchor: input.anchor ?? null
  });

export const setResolved = (fetchFn: Fetch, id: string, resolved: boolean) =>
  send(fetchFn, 'POST', commentActionPath(id, resolved ? 'resolve' : 'reopen'));

/** German line for a failed write. */
export function describeWriteFailure(status: number): string {
  if (status === 401) return 'Bitte melden Sie sich an, um zu kommentieren.';
  if (status === 0) return 'Keine Antwort vom Server. Ihr Kommentar wurde nicht gesendet.';
  if (status === 404) return 'Diese Seite ist nicht mehr verfügbar.';
  return `Der Kommentar konnte nicht gespeichert werden (Fehler ${status}).`;
}
