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

// --- Placing a file in the prose (D-15) ---------------------------------------------------

/**
 * The row an inline placement refers to, or `null` when this page carries no such file.
 *
 * **The list is the authority (D-15), and this is that rule in one function.** A block in the
 * body is a *reference*; whether the file is attached is a fact about the `attachments` table
 * and about nothing else. So a placement is resolved against the list the page already
 * fetched — not against a second request, and above all not against an address built from the
 * name. Three things follow from doing it this way, and each of them is the point:
 *
 * - **The address is the API's own.** `Attachment.href` names the page and does not name the
 *   bytes, which is the whole of D-16. This interface never assembles one.
 * - **What the file IS comes from the bytes.** `media_type` was sniffed by
 *   `gw_store::blobs::sniff`, so whether a placement renders as a picture is decided by the
 *   file's contents and never by its extension. Renaming a PDF to `.png` changes nothing.
 * - **"Not attached" is answerable at all.** Detaching a file leaves any block that named it
 *   exactly where it was — that is D-15's consequence, stated from the other side — and this
 *   returning `null` is how the reader gets to say so rather than drawing a broken picture.
 *
 * Matched on the exact name the store recorded. `canonical_filename` trims and otherwise
 * keeps a name verbatim, and case matters on the mount, so nothing is folded here: two files
 * differing only in case are two files.
 */
export function attachmentNamed(anhaenge: Attachment[], filename: string): Attachment | null {
  return anhaenge.find((anhang) => anhang.filename === filename) ?? null;
}

/**
 * Whether a file is shown where it was placed, rather than offered as a card to download.
 *
 * The owner's decision: **pictures are shown, everything else is a labelled card.** A PDF is
 * not a picture here even though the download serves it inline — a page of prose with a
 * viewer embedded in the middle of it is not what "place a file in the text" was asked for.
 *
 * **`image/svg+xml` is a picture and takes this branch, and that is safe for exactly one
 * reason: the only mechanism this interface ever renders a picture through is `<img src>`.**
 * An SVG is XML that can carry `<script>`, event handlers and external references — the one
 * image format that is also a program — and it is stored exactly as uploaded, because
 * `gw_store::blobs` says why nothing sanitises it. No browser executes script in an `<img>`
 * or in a CSS `background-image`; every other way of showing one does. `<object>`, `<embed>`
 * and `<iframe>` all run it, and putting the markup into this wiki's own DOM runs it **in
 * this origin, with the session cookie in reach**. ADR 0014 and
 * `gw_api::routes::attachments::content_disposition` state the same constraint from the
 * server's side, which is where the file is also given `Content-Disposition: attachment`,
 * `nosniff` and `default-src 'none'; sandbox` — so an `<iframe>` pointed at it would download
 * rather than render even if somebody wrote one.
 *
 * Deliberately NOT a list of image types. The accepted set is the server's and is being
 * widened; a table here would be a second answer that goes stale silently, which is the same
 * argument `kindText` above makes and the same one the upload field makes by carrying no
 * `accept` attribute.
 */
export function isPicture(mediaType: string): boolean {
  return mediaType.startsWith('image/');
}

/**
 * What a placement says when the file it names is not attached to this page.
 *
 * Stated, never drawn as a broken picture. A missing `<img>` renders as an icon and the alt
 * text, which reads as "the network failed" — and the truth is different and actionable: the
 * page still refers to a file, and the `Anhänge` list below does not carry it. That happens
 * for two ordinary reasons, and neither is a fault: somebody detached the file (which
 * deliberately does not touch the prose), or the page was imported from markdown that named
 * a file nobody has uploaded yet.
 */
export function describeMissingPlacement(filename: string, alt: string): string {
  const was = alt.trim() === '' ? '' : ` — ${alt.trim()}`;
  return `»${filename}«${was}: an dieser Seite hängt keine Datei dieses Namens. Sie wurde entfernt oder noch nicht hochgeladen.`;
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
