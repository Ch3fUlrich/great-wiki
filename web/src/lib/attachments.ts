/**
 * The `Anhänge` list (D-15) on the wire and in words.
 *
 * Pure on purpose, exactly like `$lib/trash`, `$lib/topics` and `$lib/board`: this module is
 * imported from `+page.server.ts` **and** from the component that renders the list, so it may
 * not touch `$env/dynamic/private`. Importing a server-only module from a component poisons
 * the client bundle. The calls themselves live in `$lib/api`.
 *
 * # The list is the authority, and the prose is not (D-15)
 *
 * A file is attached because there is a row saying so, never because a paragraph mentions it.
 * That is why this section exists at all rather than being derived from the document: cutting
 * a picture out of a sentence leaves the file exactly where it is, and the only place that
 * fact is visible is here. `gw_store::attachments`'s own header states the same thing from the
 * other side.
 *
 * # No address here is ever built from a digest (D-16)
 *
 * A download is authorised against **the page it was reached through**, which is only true
 * while the page is part of the address. So:
 *
 * - the address that *fetches* a file is {@link Attachment.href}, built by the API and used
 *   verbatim. This interface never assembles one, and there is no field for it to assemble
 *   one from — no response in the API carries a content address, and
 *   `no_response_anywhere_carries_the_content_address` in `gw-api/tests/attachments.rs` is
 *   what keeps it that way;
 * - the addresses this module *does* build — the listing and the upload — both name a page,
 *   and neither has anywhere to put a hash.
 *
 * # The server decides which files it takes, and nothing here repeats the answer
 *
 * The accepted set is `gw_store::blobs::sniff`'s allowlist, the size cap is
 * `MAX_ATTACHMENT_BYTES`, and both live on the server. A list of extensions or a byte ceiling
 * in this file would be a **second answer**: it agrees today, it is not consulted when the
 * allowlist is widened, and the day it disagrees it refuses a file the wiki would have taken —
 * silently, before the request is made, with no server log to find it in. So a refusal is
 * framed in German here and the API's own sentence is carried inside it, which is what
 * `describeRestore` already does for the one refusal only the Papierkorb can explain. The
 * upload field carries no `accept` attribute for the same reason.
 */

import { formatInstant } from '$lib/adminApi';

export { formatInstant };

// --- What comes off the wire ---------------------------------------------------------------

/**
 * One file on a page. Mirrors `gw_api::routes::attachments::AttachmentView`.
 *
 * **There is no digest here and there must not be one.** The store's `Attachment` carries
 * none, the API's view carries none, and this is the third statement of the same rule: a
 * reader holding a content address could go looking for the same bytes under a page they may
 * not read, and D-16 exists to make that impossible rather than merely unlikely.
 */
export interface Attachment {
  /** What the file is called on this page. One path segment, at most 255 characters. */
  filename: string;
  /** What the bytes **are**, sniffed from the file itself — never what an upload claimed. */
  media_type: string;
  byte_size: number;
  /** As SQLite writes it (`YYYY-MM-DD HH:MM:SS`, UTC). Rendered through `formatInstant`. */
  uploaded_at: string;
  /** Who attached it, as they were called then. A snapshot, exactly as a byline is. */
  uploaded_by_name: string;
  /**
   * Where to fetch it. **Used verbatim, never parsed and never rebuilt.**
   *
   * The API composes it so there is one shape of address in the system and no client is ever
   * in a position to assemble a different one. It names the page and does not name the bytes,
   * which is the whole of D-16 in one field.
   */
  href: string;
}

/**
 * `GET /api/attachments/{path}` — a page's list, **and what this caller may do to it.**
 *
 * One value, because it is one answer (ADR 0010): the read that authorised the list is what
 * produced the write verdict beside it, so a control offered on that bit and the refusal that
 * would follow pressing it cannot come apart.
 */
export interface AttachmentsResponse {
  attachments: Attachment[];
  /** Whether this caller may attach here. The store's own verdict, carried not recomputed. */
  may_write: boolean;
}

// --- Where things are ------------------------------------------------------------------------

/**
 * The region a finished upload comes back to — the anchor that makes it *announced* rather
 * than merely drawn.
 *
 * A `role="status"` region that is already in the document when it loads announces nothing; a
 * live region announces what changes. So a finished upload redirects to this id, the section
 * carries `tabindex="-1"`, and the browser moves focus there: the reader lands on the sentence
 * saying which file arrived and it is read out. No JavaScript is involved, which is the
 * requirement — the same mechanism, and the same reason, as `TRASH_REGION_ID`.
 */
export const ATTACHMENTS_REGION_ID = 'gw-anhaenge';

/**
 * What has just been attached, echoed so the reader is told rather than left to notice.
 *
 * In the address rather than in a store, for the reasons `$lib/trash` lists about its own
 * parameters: it survives a reload, the back button walks out of it, and — because there is no
 * DOM environment in this project — the state can be rendered and asserted by a test. German,
 * like `?entfernen=`, `?wurzel=` and `?seitenleiste=`.
 */
export const UPLOADED_PARAM = 'hochgeladen';

/**
 * Percent-encode each segment of a path, and nothing else.
 *
 * Its own copy rather than an import from `$lib/trash`, which is the convention every pure
 * module in this directory already follows (`$lib/topics` has a third). Three lines duplicated
 * is cheaper than a module every path-building file has to depend on.
 */
function encodeSegments(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

function withLeadingSlash(path: string): string {
  return path.startsWith('/') ? path : `/${path}`;
}

/** `/rundgang/tabellen` → `/api/attachments/rundgang/tabellen`. The list; needs read. */
export function attachmentsApiPath(path: string): string {
  return `/api/attachments${encodeSegments(withLeadingSlash(path))}`;
}

/**
 * `/rundgang/tabellen` + `befund.pdf` → `/api/attachment/befund.pdf/rundgang/tabellen`.
 * `POST` attaches, `GET` downloads, `DELETE` detaches.
 *
 * **The filename comes before the page**, which reads backwards and is not a choice: a
 * `{*path}` catch-all must be the last segment of a route, so anything else in the address has
 * to precede it. `gw_api::routes::attachments` records the same thing from the other side.
 *
 * **Singular here, plural above**, and they are two prefixes rather than one with a literal
 * segment inside it — `/api/attachments/file/{name}/{*path}` would be preferred over the
 * catch-all for a page whose first segment is `file`.
 */
export function attachmentApiPath(path: string, filename: string): string {
  return `/api/attachment/${encodeURIComponent(filename)}${encodeSegments(withLeadingSlash(path))}`;
}

// --- Words ------------------------------------------------------------------------------------

/** The units, smallest first. 1024 to a step, spelled as `$lib/history` already spells it. */
const UNITS = ['kB', 'MB', 'GB'] as const;

/**
 * How large a file is, in words.
 *
 * Its own function rather than `$lib/history`'s, which is private there and stops at `kB` —
 * right for the growth of one revision, wrong here: D-17 allows 250 MB per file and
 * `256000,0 kB` is a number nobody can read. German decimal comma, because this is a German
 * interface and `1.2 MB` reads as twelve hundred kilobytes to a German reader.
 */
export function sizeText(bytes: number): string {
  const size = Number.isFinite(bytes) && bytes > 0 ? bytes : 0;
  if (size < 1024) return `${Math.round(size)} B`;
  let value = size / 1024;
  let unit: (typeof UNITS)[number] = UNITS[0];
  for (const next of UNITS.slice(1)) {
    if (value < 1024) break;
    value /= 1024;
    unit = next;
  }
  return `${value.toFixed(1).replace('.', ',')} ${unit}`;
}

/**
 * What kind of file this is, in one German word — **derived from the type, never looked up.**
 *
 * The allowlist is the server's and is being widened right now. A table of the types this wiki
 * accepts would be a second answer that goes stale silently: a newly accepted format would
 * upload fine and then be described by whatever the table's fallback happened to be, or worse,
 * tempt somebody into using the table to decide what to offer. Reading the top-level type
 * cannot go stale — a format nobody here has heard of reads as `Datei`, which is a worse label
 * and never a false one, and the exact media type is rendered beside it either way.
 */
export function kindText(mediaType: string): string {
  if (mediaType === 'application/pdf') return 'PDF';
  const top = mediaType.split('/')[0];
  if (top === 'image') return 'Bild';
  if (top === 'video') return 'Video';
  if (top === 'audio') return 'Audio';
  return 'Datei';
}

/** The sentence for a status nothing else has wording for. Nothing the server said is lost. */
function generic(status: number, clause: string, server: string | null): string {
  const detail = server ? ` Der Server meldet: ${server}` : '';
  if (status === 0) return `${clause}: Die Anwendung antwortet nicht.`;
  return `${clause} (Fehler ${status}).${detail}`;
}

/**
 * Why the `Anhänge` list is not there.
 *
 * **Never conflated with "there are none".** A list that failed to load and a page nobody has
 * attached anything to are different things, and »Keine Anhänge« about a request that did not
 * come back is a claim about what a page carries — which is exactly the claim this section
 * exists to make truthfully.
 */
export function describeAttachments(status: number): string {
  return generic(status, 'Die Anhänge dieser Seite konnten nicht geladen werden', null);
}

/**
 * Why a file was not attached — and, in every branch, that it was not.
 *
 * The promise is the point and the API lets this interface make it: `Store::attach` settles
 * the caller, the name and the permission **before** it publishes a byte, so a refusal at any
 * of them leaves the `PendingBlob` to drop its temporary file. There is no half-attached
 * state to be vague about.
 *
 * **Four statuses are quoted rather than explained**, and it is the same judgement
 * `describeRestore` makes about a 409: the API is the only side that knows which of several
 * shapes the refusal has, and two of them name the way out.
 *
 * - **415** — the type is not one this wiki stores. The allowlist is being widened; a sentence
 *   here naming formats would be wrong the day it changed, and wrong invisibly.
 * - **413** — too large. The cap is `MAX_ATTACHMENT_BYTES`; repeating the number would be a
 *   second answer to a question only the server owns.
 * - **409** — the name is already taken on this page, or could not be an address at all. Only
 *   the API knows which, and only the API knows the name.
 * - **400** — the body was not something to attach.
 */
export function describeUpload(status: number, server: string | null = null): string {
  const nothing = 'Es wurde nichts angehängt.';
  const grund = server ? `Der Server nennt den Grund: ${server}` : 'Der Server nennt keinen Grund.';

  if (status === 0) return `Die Anwendung antwortet nicht. ${nothing}`;
  if (status === 401) return `Nicht angemeldet — bitte erneut anmelden. ${nothing}`;
  if (status === 403) {
    // Both halves, because they are one refusal here and the reader cannot tell which applies:
    // `Store::attach` demands a signed-in, active account BEFORE it consults a single grant,
    // for the reason a revision needs an author — the row records who put the file there. A
    // path carrying `anyone: write` makes a page editable by somebody who has not said who
    // they are, and putting a quarter of a gigabyte on the mount through one is not the same
    // act as editing a paragraph.
    return (
      'Zum Anhängen braucht es das Schreibrecht auf dieser Seite und ein angemeldetes Konto — ' +
      `ein Anhang hält fest, wer ihn hochgeladen hat. ${nothing}`
    );
  }
  if (status === 404) return `Diese Seite gibt es nicht (mehr). ${nothing}`;
  if (status === 409) {
    return `So kann diese Datei hier nicht liegen. ${grund} ${nothing}`;
  }
  if (status === 413) {
    return `Die Datei ist zu groß für dieses Wiki. ${grund} ${nothing}`;
  }
  if (status === 415) {
    return `Dieser Dateityp wird hier nicht gespeichert. ${grund} ${nothing}`;
  }
  if (status === 400) {
    return `Die Datei wurde so nicht angenommen. ${grund} ${nothing}`;
  }
  if (status === 503) {
    // The mount, not the wiki: `/mnt/cloud` really does answer `Stale file handle` inside a
    // container while the host is fine, and it recovers. "Try again" is the honest advice.
    return (
      'Der Speicher für Anhänge ist gerade nicht erreichbar. Das geht meist von selbst ' +
      `vorüber — bitte später noch einmal versuchen. ${nothing}`
    );
  }
  return `${generic(status, 'Die Datei konnte nicht angehängt werden', server)} ${nothing}`;
}
