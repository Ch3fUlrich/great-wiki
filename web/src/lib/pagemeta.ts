/**
 * What a reader can be told about a page TODAY, derived only from what the API already
 * returns: `/api/documents/{path}` (a `StoredDocument`) and `/api/tree`.
 *
 * Two rules shape everything here.
 *
 * FIRST: NOTHING IS INVENTED. There is no revisions endpoint yet, so "last edited, by
 * whom" is not available — and a placeholder date, an "unbekannt", or a dash in a column
 * that otherwise holds facts would all be read as data. The slot for it is
 * [`RevisionInfo`] plus [`editedAtLabel`]; both are written and tested so that whoever
 * lands the endpoint has nothing left to decide, and neither is called anywhere yet.
 *
 * SECOND: STRUCTURE COMES FROM THE TREE, NOT FROM THE PATH. `/api/tree` is filtered in the
 * retriever to what the caller may read (AGENTS.md rule 2), so deriving ancestors and
 * children from it inherits that filtering for free. Splitting `doc.path` on `/` instead
 * would produce a breadcrumb of ancestors the reader may not be allowed to know exist, and
 * there would be no title to put in it anyway — only a slug. That is why
 * [`breadcrumb`] falls back to the page alone rather than to invented parents.
 */
import type { TreeNode } from '$lib/api';

/** One step of the path from the root down to a page. */
export interface Crumb {
  /** Root-relative URL, exactly as the tree states it. Never assembled from segments. */
  path: string;
  title: string;
}

/**
 * The chain of tree nodes from the outermost ancestor down to `path`, inclusive.
 *
 * Empty when `path` is not in this tree — which is a real case, not a defect: the tree
 * drops a whole branch whose parent the caller cannot read, while `/api/documents` will
 * still serve a page the caller holds a direct grant on. See [`breadcrumb`].
 */
export function trail(nodes: TreeNode[], path: string): Crumb[] {
  for (const node of nodes) {
    if (node.path === path) return [{ path: node.path, title: node.title }];
    const below = trail(node.children, path);
    if (below.length > 0) return [{ path: node.path, title: node.title }, ...below];
  }
  return [];
}

/**
 * The breadcrumb to render for `doc`: its trail through the tree, or — when the tree does
 * not contain it — the page on its own.
 *
 * The fallback is deliberately barren. The alternative would be to cut `doc.path` into
 * segments and title-case them, which invents a title for every ancestor and does it
 * exactly in the case where the ancestors are the ones the caller was not shown.
 */
export function breadcrumb(nodes: TreeNode[], doc: { path: string; title: string }): Crumb[] {
  const found = trail(nodes, doc.path);
  return found.length > 0 ? found : [{ path: doc.path, title: doc.title }];
}

/**
 * The children of `path` as the tree states them — already permission-filtered, already
 * in `sort_key` order, already carrying titles.
 *
 * Empty both when the page has no children and when it is not in the tree; the caller
 * renders nothing in either case, so the distinction would buy nothing.
 */
export function childrenOf(nodes: TreeNode[], path: string): TreeNode[] {
  for (const node of nodes) {
    if (node.path === path) return node.children;
    const below = childrenOf(node.children, path);
    if (below.length > 0) return below;
  }
  return [];
}

/** How many further levels hang below a subpage — the tree knows, so say it. */
export function subpageCount(n: number): string {
  return n === 1 ? '1 Unterseite' : `${n} Unterseiten`;
}

export type VisibilityTone = 'public' | 'internal' | 'restricted';

export interface VisibilityStatement {
  tone: VisibilityTone;
  /** The word, short enough to sit in a chip and still be unambiguous on its own. */
  label: string;
  /** Who that actually means, in a full sentence. */
  detail: string;
}

/**
 * The three levels, in German a reader can act on.
 *
 * "ÖFFENTLICH" IS THE ONE THAT HAD TO BE GOT RIGHT. This wiki answers on the public
 * internet, and in an intranet the bare word "öffentlich" is routinely used to mean
 * "everyone in the organisation" — precisely the wrong reading here. So the label says
 * "Öffentlich im Internet" and the sentence says "ohne Anmeldung", because a reader who
 * misjudges this one writes something into a page that strangers can read.
 *
 * The other two are worded against what `permits()` in gw-store/src/acl.rs actually does,
 * not against what the words suggest:
 *   - `internal` is NOT "anyone signed in" — it needs the Internal baseline, which comes
 *     from a verified group, so "angemeldete Mitglieder" rather than "alle Angemeldeten";
 *   - `restricted` is reachable by an explicit grant on any visibility, and by the Admin
 *     baseline for reads. "Ausdrücklich berechtigte Personen und Teams" covers both
 *     without claiming the page is invisible to administrators, which would be false.
 */
const VISIBILITY: Record<VisibilityTone, VisibilityStatement> = {
  public: {
    tone: 'public',
    label: 'Öffentlich im Internet',
    detail: 'Jede Person kann diese Seite ohne Anmeldung lesen.'
  },
  internal: {
    tone: 'internal',
    label: 'Intern',
    detail: 'Nur angemeldete Mitglieder können diese Seite lesen.'
  },
  restricted: {
    tone: 'restricted',
    label: 'Eingeschränkt',
    detail: 'Nur ausdrücklich berechtigte Personen und Teams können diese Seite lesen.'
  }
};

/**
 * What to say about `raw`, failing closed.
 *
 * An unrecognised value reports the STRICTEST level, and that is not defensive
 * hand-waving: `Visibility::from_str(…).unwrap_or_default()` in gw-store gives
 * `Restricted` for anything it cannot parse, so "Eingeschränkt" is a true statement about
 * how the permission engine will treat this page. Reporting "unbekannt" would be a
 * different claim, and a worse one — it invites the reader to assume the page is more open
 * than the server will actually allow.
 */
export function visibilityStatement(raw: string): VisibilityStatement {
  const key = raw.trim().toLowerCase();
  return key === 'public' || key === 'internal' ? VISIBILITY[key] : VISIBILITY.restricted;
}

/** `DocumentType`, minus `page` — the default, which says nothing worth a row. */
const DOCUMENT_TYPES: Record<string, string> = {
  research: 'Recherche',
  project: 'Projekt',
  dataset: 'Datensatz'
};

/**
 * The German name of a document type, or `null` when there is nothing to report.
 *
 * A type this table does not know is returned unchanged rather than dropped or renamed:
 * it is what the API said, and showing it is honest, whereas silently hiding it would lose
 * the one clue that something new exists on the server.
 */
export function documentTypeLabel(raw: string): string | null {
  const key = raw.trim().toLowerCase();
  if (key === '' || key === 'page') return null;
  return DOCUMENT_TYPES[key] ?? raw.trim();
}

/**
 * The German name of a language tag, via the platform's own CLDR data rather than a table
 * this repository would have to maintain. Falls back to the tag itself — which is what
 * `Intl.DisplayNames` already does for a tag it does not know, and what the `catch`
 * handles for a tag too malformed to look up at all.
 */
export function languageName(tag: string, inLanguage = 'de'): string {
  const primary = tag.trim().toLowerCase().split(/[-_]/)[0];
  if (primary === '') return tag.trim();
  try {
    return new Intl.DisplayNames([inLanguage], { type: 'language' }).of(primary) ?? primary;
  } catch {
    return primary;
  }
}

/**
 * The language to announce, or `null` when announcing it would be noise.
 *
 * WHAT "DIFFERS FROM ITS NEIGHBOURS" HAD TO BECOME, and why. The intent is to mark a page
 * whose language is not the one around it. `/api/tree` carries `path`, `slug`, `title`,
 * `doc_type` and `visibility` — no language — so the siblings' languages are not knowable
 * without one `/api/documents` request per sibling on every page render, which is a
 * different load contract than this feature is worth.
 *
 * The comparison that IS available is against the interface, which is German throughout
 * (and is also the `lang` the reader's browser has been told the chrome is in). That is
 * the mismatch a reader actually experiences: German navigation, German metadata, English
 * body. A German page therefore says nothing, because there is nothing to warn about.
 */
export function foreignLanguage(raw: string, interfaceLanguage = 'de'): string | null {
  const primary = raw.trim().toLowerCase().split(/[-_]/)[0];
  if (primary === '' || primary === interfaceLanguage) return null;
  return languageName(primary, interfaceLanguage);
}

/**
 * WHO PUBLISHED THE CURRENT REVISION AND WHEN — THE SLOT, NOT THE DATA.
 *
 * Nothing constructs one of these today. `/api/documents/{path}` returns a
 * `StoredDocument`, which has no revision fields, and there is no revisions endpoint yet;
 * the store side is being built separately. `PageMeta` takes this as an optional prop and
 * renders the row only when it is present, so filling the slot is a change to the loader
 * and to nothing else.
 */
export interface RevisionInfo {
  /** RFC 3339 instant, as SQLite stores timestamps. */
  edited_at: string;
  /** Display name of whoever published it; `null` where the API cannot name a person. */
  edited_by: string | null;
  /** 1-based revision number, when the API reports one. */
  number?: number | null;
}

/**
 * A stored instant as a German date, or `null` when it cannot be parsed — never the string
 * "Invalid Date", which is what `Intl` produces if this check is skipped and which looks
 * exactly like a bug in the page rather than in the data.
 *
 * The zone is explicit rather than the host's. Server rendering means the host's zone
 * decides what every reader sees, so leaving it implicit makes the rendered date depend on
 * a machine setting nobody thinks about — and makes this function untestable, because the
 * expected string would change with `TZ`.
 */
export function editedAtLabel(
  iso: string,
  timeZone = 'Europe/Berlin',
  locale = 'de-DE'
): string | null {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return null;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: 'long',
    timeStyle: 'short',
    timeZone
  }).format(at);
}
