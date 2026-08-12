/**
 * What an editing session can be, what a person is told about it, and what they may do in
 * it. Pure functions only, so every one of them is testable — there is no DOM environment
 * in this project, and this is the part where being wrong costs somebody their work.
 *
 * # The model
 *
 * A session is a WebSocket to `GET /api/collab/{path}`. The server authorises it with
 * `Action::Write` *before* the upgrade, so a refusal is an HTTP status and never a socket —
 * and a browser reports a refused upgrade as close code 1006 with no status attached, which
 * is precisely what a server being down looks like. That ambiguity is resolved by
 * [`diagnoseFirstFailure`], not by guessing.
 *
 * # Where the work actually is
 *
 * Three places, and they are not the same place:
 *
 * 1. **The browser.** Everything typed, immediately.
 * 2. **The room**, in the server's memory. Every update the socket carried, immediately.
 *    Lost if the process dies.
 * 3. **The database**, as a revision. Written by `POST /api/collab/{path}` at once, or by
 *    the janitor's sweep every `snapshot_interval` — thirty seconds in production.
 *
 * So "saved" has to mean something. It means (2): the server has the keystroke and will
 * write it out within the sweep interval whether or not this browser is still here, because
 * eviction persists a room before dropping it. Only the `live` state can claim that, and
 * [`describeSession`] is where that claim is made exactly once.
 */

/** The states a session can be in. Every one is reachable in production. */
export type SessionState =
  /** Asking. No socket yet, nothing typed can go anywhere. */
  | 'connecting'
  /** Connected and synced. Keystrokes reach the room as they are typed. */
  | 'live'
  /**
   * The socket dropped after having worked, and y-websocket is retrying. Still typeable on
   * purpose: a CRDT merges what was written while it was away, so the edits are recoverable
   * — but they are NOT saved until the socket is back, and the words have to say so.
   */
  | 'offline'
  /** The server refused the session. Reading a page is not permission to edit it (D-M2-8). */
  | 'refused'
  /** Permission ended mid-session: `permissionDenied`, then a 1008 close. */
  | 'revoked'
  /** No answer at all. Not a permission decision — the server or the network. */
  | 'unreachable'
  /** The server ended the session under a policy that is not about permission. */
  | 'ended';

/**
 * Every state, for the tests and for the exhaustiveness they enforce.
 *
 * A list rather than a derived thing because it is what the property test iterates: adding
 * a state without deciding whether it may claim to be safe has to fail something.
 */
export const ALL_SESSION_STATES = [
  'connecting',
  'live',
  'offline',
  'refused',
  'revoked',
  'unreachable',
  'ended'
] as const satisfies readonly SessionState[];

/**
 * The sentence the milestone requires to be on the screen while somebody edits, not in
 * documentation they will not read (D-M2-9, D-M3-5, and the M3 plan's "warning at the point
 * of editing").
 *
 * History follows read: anyone who may read this page may read every revision of it. So
 * deleting a sentence is an edit, not a redaction — the earlier revision still has it. The
 * moment that misunderstanding happens is the moment somebody removes something, which is
 * why this is permanent chrome in the editor rather than a toast.
 */
export const HISTORY_WARNING =
  'Jede Fassung bleibt in der Versionsgeschichte lesbar — für alle, die diese Seite lesen ' +
  'dürfen. Etwas zu löschen entfernt es aus der aktuellen Seite, nicht aus dem Verlauf.';

/** What the editor says about itself. */
export interface SessionDescription {
  /** For colour and for the ARIA live-region politeness. */
  tone: 'ok' | 'warn' | 'fail' | 'info';
  headline: string;
  detail: string;
  /**
   * Whether the server has what has been typed. The single claim this module exists to get
   * right; asserted over every state by the tests.
   */
  safe: boolean;
}

export function describeSession(state: SessionState): SessionDescription {
  switch (state) {
    case 'connecting':
      return {
        tone: 'info',
        headline: 'Sitzung wird geöffnet …',
        detail: 'Das Bearbeitungsfeld erscheint, sobald der Server die Sitzung bestätigt hat.',
        safe: false
      };
    case 'live':
      // The one place in this file that says the work is safe, and it has to say two
      // separate true things rather than one comfortable one.
      //
      // SAFE: the update is in the room, the sweep writes the room out about every thirty
      // seconds, and a room nobody is in any more is written out before it is dropped. A
      // crash costs at most one sweep interval, which is why the sentence names the
      // interval instead of saying "gespeichert" and stopping.
      //
      // NOT YET VISIBLE: since `fix(collab): autosave stores CRDT state instead of filing a
      // revision`, an autosave writes `crdt_state` and NOT a revision — so `documents.body`,
      // which is what every reader is served, does not move until somebody publishes. Saved
      // and published are two different things here, and somebody who edits, closes the tab
      // and later finds the page unchanged would otherwise conclude their work was lost.
      return {
        tone: 'ok',
        headline: 'Verbunden — Änderungen sind auf dem Server',
        detail:
          'Der Server sichert sie automatisch, etwa alle 30 Sekunden. Lesende sehen die Seite ' +
          'bis dahin unverändert: erst »Veröffentlichen« macht den aktuellen Stand zur ' +
          'gültigen Fassung und schreibt ihn in die Versionsgeschichte.',
        safe: true
      };
    case 'offline':
      return {
        tone: 'warn',
        headline: 'Keine Verbindung — noch nicht gespeichert',
        detail:
          'Du kannst weiterschreiben. Sobald die Verbindung zurück ist, wird alles übertragen ' +
          'und zusammengeführt. Schließe diesen Tab bis dahin nicht.',
        safe: false
      };
    case 'refused':
      return {
        tone: 'fail',
        headline: 'Du darfst diese Seite nicht bearbeiten',
        detail:
          'Schreiben ist eine eigene Berechtigung; sie zu lesen schließt sie nicht ein. ' +
          'Bitte eine Administratorin oder einen Administrator um Schreibrechte für diese Seite.',
        safe: false
      };
    case 'revoked':
      return {
        tone: 'fail',
        headline: 'Die Sitzung wurde beendet: keine Schreibrechte mehr',
        detail:
          'Was seit der letzten automatischen Sicherung geschrieben wurde, ist nicht gespeichert. ' +
          'Kopiere den Text, bevor du die Seite verlässt.',
        safe: false
      };
    case 'unreachable':
      return {
        tone: 'fail',
        headline: 'Der Server antwortet nicht',
        detail:
          'Es wurde nichts übertragen und nichts verändert. Lade die Seite später noch einmal.',
        safe: false
      };
    case 'ended':
      return {
        tone: 'fail',
        headline: 'Der Server hat die Sitzung beendet',
        detail:
          'Was seit der letzten automatischen Sicherung geschrieben wurde, ist nicht gespeichert. ' +
          'Kopiere den Text und lade die Seite neu.',
        safe: false
      };
  }
}

/**
 * Whether keystrokes have somewhere to go.
 *
 * `offline` says yes deliberately — this is the whole reason the system uses a CRDT rather
 * than last-write-wins, and locking the editor there would discard work Yjs would otherwise
 * merge on reconnect. Every other non-live state is a dead end, and an editor that accepts
 * typing into a dead end is exactly the failure `collab.rs` refuses read-only sockets to
 * avoid.
 */
export function mayType(state: SessionState): boolean {
  return state === 'live' || state === 'offline';
}

/**
 * Whether there is work the server has not got.
 *
 * Drives the leave-the-page guard and the offer to copy the text out. `live` is false even
 * with local edits: the update went into the room as it was typed.
 */
export function hasUnsavedWork(state: SessionState, localEdits: boolean): boolean {
  if (!localEdits) return false;
  return state !== 'live';
}

/**
 * Whether reconnecting after this close code could ever succeed.
 *
 * y-websocket's own default treats only 4400-4499 as permanent, and `collab.rs` uses the
 * standard codes: `POLICY` (1008) for a revoked permission and for a rate-limit trip,
 * `UNSUPPORTED` (1003) for a frame it could not read, `SIZE` (1009) for one too large.
 * Retrying any of those reproduces the same close immediately and for ever — a reconnect
 * loop against the endpoint that just said no.
 *
 * The 4400-4499 range is kept as well, so that if the server ever adopts the convention
 * y-websocket documents, this predicate does not have to be the thing that remembers.
 */
export function isTerminalClose(code: number): boolean {
  return code === 1003 || code === 1008 || code === 1009 || (code >= 4400 && code < 4500);
}

/**
 * Why the very first connection attempt failed.
 *
 * A browser will not tell JavaScript the HTTP status of a refused upgrade — a 403 arrives
 * as close code 1006, byte for byte the same as the server being unplugged. So the question
 * is answered by asking a second one that *does* have a readable status: can this session
 * still read the page it was trying to edit? If yes, the API is up and answering as this
 * user, and the only thing that failed was the write decision.
 *
 * It can be wrong in one direction — a server that dies between the two requests reads as a
 * refusal — and that is the safe direction: it tells somebody to ask for permission rather
 * than telling somebody with no permission to keep reloading.
 */
export function diagnoseFirstFailure(pageStillReadable: boolean): SessionState {
  return pageStillReadable ? 'refused' : 'unreachable';
}

/** Where the collaboration socket for `path` lives. */
export interface CollabTarget {
  /** y-websocket appends `'/' + room` to this, with no encoding of its own. */
  serverUrl: string;
  room: string;
}

/**
 * The socket address for a document path.
 *
 * Two things this must not get wrong. The scheme has to follow the page's: a `wss:` socket
 * from an `http:` page fails to connect and a `ws:` socket from an `https:` page is blocked
 * as mixed content, both without an error the application can read. And the path has to be
 * escaped here, because y-websocket concatenates `serverUrl + '/' + roomname` verbatim — a
 * `?` in a slug would turn the rest of the path into a query string and open a session on a
 * different document. The separators are kept, or a nested page collapses into one segment
 * the router cannot match.
 */
export function collabTarget(origin: string, path: string): CollabTarget {
  const url = new URL(origin);
  const scheme = url.protocol === 'https:' ? 'wss:' : 'ws:';
  const room = path
    .replace(/^\/+/, '')
    .split('/')
    .map(encodeURIComponent)
    .join('/');
  return { serverUrl: `${scheme}//${url.host}/api/collab`, room };
}

/** The answer to a publish attempt. */
export interface PublishDescription {
  tone: 'ok' | 'fail';
  text: string;
}

/**
 * What `POST /api/collab/{path}` answered, in words.
 *
 * The four statuses it documents, each meaning something different about where the work is:
 * 200 it is in the history, 403 the permission went away between opening the editor and
 * pressing the button, 404 the page did too, 409 the room was evicted — the page is fine,
 * the session is not, and a reload will get a new one. Collapsing any pair of those would
 * send somebody looking in the wrong place.
 */
export function describePublish(status: number): PublishDescription {
  switch (status) {
    case 200:
      return { tone: 'ok', text: 'Veröffentlicht. Die Fassung steht in der Versionsgeschichte.' };
    case 403:
      return {
        tone: 'fail',
        text: 'Abgelehnt: du darfst diese Seite nicht schreiben. Es wurde nichts veröffentlicht.'
      };
    case 404:
      return { tone: 'fail', text: 'Diese Seite gibt es nicht mehr. Es wurde nichts veröffentlicht.' };
    case 409:
      return {
        tone: 'fail',
        text:
          'Die Sitzung ist auf dem Server nicht mehr offen. Lade die Seite neu — der zuletzt ' +
          'gesicherte Stand ist erhalten, spätere Änderungen sind es nicht.'
      };
    default:
      return {
        tone: 'fail',
        text: `Veröffentlichen fehlgeschlagen (HTTP ${status}). Es wurde nichts veröffentlicht.`
      };
  }
}
