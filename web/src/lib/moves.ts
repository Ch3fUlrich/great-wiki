/**
 * Renaming and moving a page, on the wire and in words (roadmap 2026-09-24, ADR 0023).
 *
 * Pure, like `$lib/trash`: imported from the page's `+page.server.ts` **and** its component,
 * so it may not touch `$env/dynamic/private`. The calls themselves live in `$lib/api`.
 *
 * **Nothing here decides who may move anything, or who a move lets in.** The API measures the
 * move by carrying it out and rolling it back (`gw_store::Store::move_document`), and says in
 * `refusal` whether this caller could make it. This module words that answer; it recomputes
 * none of it. A second opinion about who may read a page, computed in a browser, is the
 * second answer this project keeps refusing to have.
 */

import type { TreeNode } from '$lib/api';
import { encodeSegments } from '$lib/trash';

// --- What comes off the wire ---------------------------------------------------------------

/** One person whose reading access a move changes. Mirrors `gw_store::ReaderChange`. */
export interface ReaderChange {
  /** Everybody who has not signed in. */
  anonymous: boolean;
  name: string;
  username: string | null;
  /** On how many of the moved pages. */
  pages: number;
}

/** A move, measured. Mirrors `gw_store::MovePlan`. */
export interface MovePlan {
  from: string;
  to: string;
  title: string;
  /** Live pages that move: the page and everything under it. */
  pages: number;
  gains: ReaderChange[];
  losses: ReaderChange[];
  /** Why this caller may not carry the move out, when the plan is complete but refused. */
  refusal: string | null;
  committed: boolean;
}

// --- Where things are ------------------------------------------------------------------------

/**
 * The question rides in the address, like `?loeschen=1`: server-rendered in the first
 * response, survives a reload, the back button walks out of it, and every state can be
 * asserted by a test without a DOM.
 */
export const MOVE_PARAM = 'verschieben';
export const MOVE_REGION_ID = 'gw-verschieben';

/** The three fields of the dialog, named once so the GET form and the POST form agree. */
export const PARENT_FIELD = 'ziel';
export const TITLE_FIELD = 'titel';
export const SLUG_FIELD = 'adresse';

/** What the dialog asks for. `ziel` is a path, or `''` for the top level. */
export interface MoveFields {
  ziel: string;
  titel: string;
  adresse: string;
}

/** `/handbuch` → `/api/move/handbuch`. `POST` moves it. */
export function moveApiPath(path: string): string {
  return `/api/move${encodeSegments(withLeadingSlash(path))}`;
}

/** The same address asked with `GET`: the move, measured and not made. */
export function movePreviewApiPath(path: string, fields: MoveFields): string {
  const query = new URLSearchParams({
    parent: fields.ziel,
    title: fields.titel,
    slug: fields.adresse
  });
  return `${moveApiPath(path)}?${query}`;
}

/** The body `POST /api/move/{path}` takes. */
export function moveBody(fields: MoveFields): { parent: string; title: string; slug: string } {
  return { parent: fields.ziel, title: fields.titel, slug: fields.adresse };
}

/** `/alt` → `/api/forwards/alt`: where the page that lived there is now, for a reader. */
export function forwardApiPath(path: string): string {
  return `/api/forwards${encodeSegments(withLeadingSlash(path))}`;
}

/** The link that opens the dialog, focused by its fragment — see `deleteHref`. */
export function moveHref(path: string): string {
  return `${path}?${MOVE_PARAM}=1#${MOVE_REGION_ID}`;
}

/**
 * The dialog's fields out of an address or a submitted form. `null` until a title has been
 * submitted at all, which is how the loader tells "the dialog was opened" from "show me what
 * this would do".
 */
export function readFields(source: URLSearchParams | FormData): MoveFields | null {
  const titel = source.get(TITLE_FIELD);
  if (titel === null) return null;
  return {
    ziel: String(source.get(PARENT_FIELD) ?? '').trim(),
    titel: String(titel).trim(),
    adresse: String(source.get(SLUG_FIELD) ?? '').trim()
  };
}

/** One place a page could be put under, as the picker offers it. */
export interface Destination {
  path: string;
  title: string;
  /** How deep in the tree, for the indentation of the option. 0 is the top level. */
  depth: number;
}

/**
 * Every page the one being moved could go under, in tree order — **the tree this caller was
 * already given**, which `Store::tree_for` has filtered per document. There is no second
 * listing of "which pages are there" (ADR 0019's picker argument), so the picker cannot offer
 * a page the reader may not see.
 *
 * The page itself and everything under it are left out: a page cannot move into itself, and
 * the API would refuse it anyway — offering it would be a control that lies.
 */
export function destinations(tree: TreeNode[], moving: string): Destination[] {
  const out: Destination[] = [];
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const node of nodes) {
      if (node.path === moving || node.path.startsWith(`${moving}/`)) continue;
      out.push({ path: node.path, title: node.title, depth });
      walk(node.children ?? [], depth + 1);
    }
  };
  walk(tree, 0);
  return out;
}

/** The parent a page sits under now: its path without the last segment, `''` at the top. */
export function parentOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut <= 0 ? '' : path.slice(0, cut);
}

// --- Words ------------------------------------------------------------------------------------

/** `1 Seite`, `3 Seiten`. */
export function pagesText(count: number): string {
  return count === 1 ? '1 Seite' : `${count} Seiten`;
}

/**
 * One person in the preview, in words: who, and on how many of the moved pages.
 *
 * "Alle ohne Anmeldung" for the anonymous entry — that is what an `anyone` grant reaches,
 * and it is the one a reader most needs to see named.
 */
export function readerText(change: ReaderChange, total: number): string {
  const who = change.anonymous
    ? 'Alle ohne Anmeldung'
    : change.username && change.username !== change.name
      ? `${change.name} (${change.username})`
      : change.name;
  const wie =
    total <= 1 ? '' : change.pages >= total ? ' — alle Seiten' : ` — ${change.pages} von ${total} Seiten`;
  return `${who}${wie}`;
}

/**
 * Why a move did not happen — and, in every branch, that nothing moved.
 *
 * **The 409 is quoted**, for `describeRestore`'s reason: it has many shapes (an occupied
 * address, a subpage somebody else governs, a destination that is not there, access that
 * would widen) and several of them name a path only the API knows. Matching English sentences
 * in a German interface fails silently, so the refusal is framed in German and the API's own
 * words are carried inside it.
 */
export function describeMove(status: number, server: string | null = null): string {
  const nothing = 'Es wurde nichts verschoben.';
  if (status === 0) return `Die Anwendung antwortet nicht. ${nothing}`;
  if (status === 401) return `Nicht angemeldet — bitte erneut anmelden. ${nothing}`;
  if (status === 403) return `Dafür fehlt das Schreibrecht auf dieser Seite. ${nothing}`;
  if (status === 404) return `Diese Seite gibt es nicht (mehr). ${nothing}`;
  if (status === 409) {
    return `So lässt sich die Seite nicht verschieben${server ? ` (${server})` : ''}. ${nothing}`;
  }
  return `Die Seite konnte nicht verschoben werden (Fehler ${status}${server ? `: ${server}` : ''}). ${nothing}`;
}

function withLeadingSlash(path: string): string {
  return path.startsWith('/') ? path : `/${path}`;
}
