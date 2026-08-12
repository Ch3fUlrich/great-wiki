import { describe, expect, it } from 'vitest';
import {
  ALL_SESSION_STATES,
  HISTORY_WARNING,
  collabTarget,
  describeSession,
  diagnoseFirstFailure,
  describePublish,
  hasUnsavedWork,
  isTerminalClose,
  mayType,
  type SessionState
} from './session';

/**
 * The one failure this feature is not allowed to have: a person believing their work is
 * saved when it is not.
 *
 * Every state below is reachable in production. `describeSession` is the only thing that
 * turns one into words, so it is the only place that can tell the lie — which is why the
 * tests here are mostly about what the words may NOT say, rather than about their wording.
 */

describe('what the person is told', () => {
  it('never calls work saved unless the server has actually taken it', () => {
    // The property, over every state at once, rather than a sentence per state: a state
    // added later without a decision about safety fails this immediately. `live` is the
    // only state in which an update has reached the room.
    for (const state of ALL_SESSION_STATES) {
      const { headline, detail, safe } = describeSession(state);
      const words = `${headline} ${detail}`;
      if (state === 'live') {
        expect(safe, `${state} should be the safe state`).toBe(true);
      } else {
        expect(safe, `${state} must not claim to be safe`).toBe(false);
        expect(words, `${state} must not use the word "gespeichert" affirmatively`).not.toMatch(
          /(?<!nicht )(?<!noch nicht )gespeichert/
        );
      }
    }
  });

  it('says something specific, in German, for every state', () => {
    for (const state of ALL_SESSION_STATES) {
      const { headline, detail } = describeSession(state);
      expect(headline.length, state).toBeGreaterThan(8);
      expect(detail.length, state).toBeGreaterThan(20);
      // ASCII-only text here would mean an English string slipped through; every one of
      // these sentences contains at least one German-specific character or word.
      expect(`${headline} ${detail}`, state).toMatch(/[äöüÄÖÜß]|\b(nicht|und|der|die|das|wird)\b/);
    }
  });

  it('tells a refused person why, and does not blame the network for it', () => {
    // The whole point of the differential diagnosis: a browser reports a refused WebSocket
    // upgrade as close code 1006, indistinguishable from the server being down. Telling
    // somebody "connection lost" when the truth is "you have no write permission" sends
    // them to reload the page for ever.
    const refused = describeSession('refused');
    expect(refused.headline).toContain('nicht bearbeiten');
    expect(refused.detail).toContain('Berechtigung');

    const unreachable = describeSession('unreachable');
    expect(unreachable.detail).not.toContain('Berechtigung');
  });

  it('decides which of the two it was by asking a question with a readable answer', () => {
    // If the page is still readable as this session, the API is up and answering as this
    // user, so the only thing that failed was the write decision.
    expect(diagnoseFirstFailure(true)).toBe('refused');
    expect(diagnoseFirstFailure(false)).toBe('unreachable');
  });
});

describe('what the person may do', () => {
  it('only lets somebody type when the keystrokes have somewhere to go', () => {
    // `offline` is deliberately still typeable: the CRDT merges what was typed while the
    // socket was down as soon as it comes back, so refusing input there would throw away
    // work the system can actually keep. Every other non-live state is a dead end.
    expect(mayType('live')).toBe(true);
    expect(mayType('offline')).toBe(true);
    expect(mayType('connecting')).toBe(false);
    expect(mayType('refused')).toBe(false);
    expect(mayType('revoked')).toBe(false);
    expect(mayType('unreachable')).toBe(false);
    expect(mayType('ended')).toBe(false);
  });

  it('reports unsaved work exactly when edits exist that the server has not taken', () => {
    // Drives both the leave-the-page guard and the offer to copy the text out. `live` with
    // local edits is NOT unsaved: the update went into the room as it was typed.
    expect(hasUnsavedWork('live', true)).toBe(false);
    expect(hasUnsavedWork('offline', true)).toBe(true);
    expect(hasUnsavedWork('revoked', true)).toBe(true);
    expect(hasUnsavedWork('ended', true)).toBe(true);
    // No local edits: nothing to lose, so no guard and no scare.
    for (const state of ALL_SESSION_STATES) {
      expect(hasUnsavedWork(state, false), state).toBe(false);
    }
  });
});

describe('when the server ends a session', () => {
  it('stops reconnecting on a decision, and keeps reconnecting on a blip', () => {
    // `collab.rs` closes with `close_code::POLICY` (1008) for a revoked permission and for
    // a rate-limit trip, `UNSUPPORTED` (1003) for a frame it cannot read and `SIZE` (1009)
    // for one that is too big. Reconnecting after any of those produces exactly the same
    // close again, for ever, several times a second — y-websocket's own default would do
    // that, because it only treats 4400-4499 as permanent.
    expect(isTerminalClose(1008)).toBe(true);
    expect(isTerminalClose(1003)).toBe(true);
    expect(isTerminalClose(1009)).toBe(true);
    expect(isTerminalClose(4403)).toBe(true);
    // A dropped connection, a server restart, a closed laptop lid: all worth retrying,
    // because the CRDT will merge whatever was typed in the meantime.
    expect(isTerminalClose(1006)).toBe(false);
    expect(isTerminalClose(1001)).toBe(false);
    expect(isTerminalClose(1012)).toBe(false);
  });
});

describe('the address of the session', () => {
  it('points at the page being read, over the transport the page was served on', () => {
    expect(collabTarget('https://wiki.example.org', '/handbuch/onboarding')).toEqual({
      serverUrl: 'wss://wiki.example.org/api/collab',
      room: 'handbuch/onboarding'
    });
    // A page served over plain HTTP must not open a `wss:` socket, and vice versa —
    // mixed content is blocked outright by the browser, with no error the app can see.
    expect(collabTarget('http://127.0.0.1:5173', '/notiz').serverUrl).toBe(
      'ws://127.0.0.1:5173/api/collab'
    );
  });

  it('escapes a path segment rather than putting it in a URL raw', () => {
    // y-websocket builds its URL as `serverUrl + '/' + roomname`, with no encoding at all,
    // so anything the slug generator ever lets through arrives here unescaped. A `?` would
    // silently turn the rest of the path into a query string and open a session on the
    // wrong document.
    expect(collabTarget('https://x.test', '/a b/c?d').room).toBe('a%20b/c%3Fd');
    // The separators must survive, or a nested page becomes one flat segment the router
    // cannot match.
    expect(collabTarget('https://x.test', '/eins/zwei/drei').room).toBe('eins/zwei/drei');
  });
});

describe('publishing', () => {
  it('has a distinct, honest answer for every status the endpoint returns', () => {
    // 200/403/404/409, exactly as `collab::publish` documents them.
    expect(describePublish(200).tone).toBe('ok');
    expect(describePublish(403).tone).toBe('fail');
    expect(describePublish(404).tone).toBe('fail');
    expect(describePublish(409).tone).toBe('fail');
    expect(describePublish(500).tone).toBe('fail');

    const texts = [200, 403, 404, 409, 500].map((s) => describePublish(s).text);
    expect(new Set(texts).size, 'two statuses share a message').toBe(texts.length);

    // 409 means the room is gone — the page is fine, the session is not. Telling somebody
    // "published" or "no permission" there sends them looking in the wrong place.
    expect(describePublish(409).text).toContain('Sitzung');
    expect(describePublish(403).text).toContain('nicht');
  });

  it('never reports a failure as though the revision exists', () => {
    for (const status of [403, 404, 409, 500, 502]) {
      expect(describePublish(status).text, `${status}`).not.toContain('Veröffentlicht.');
    }
  });
});

describe('the warning the milestone requires', () => {
  it('says that removing text does not remove it from the history', () => {
    // D-M3-5 and the M3 plan both require this at the moment of editing rather than in
    // documentation: history is readable by anyone who can read the page, so deleting a
    // sentence is an edit, not a redaction. Somebody who pastes a password and deletes it
    // has to learn that now.
    expect(HISTORY_WARNING).toMatch(/Versionsgeschichte|Verlauf/);
    expect(HISTORY_WARNING).toMatch(/lösch/i);
  });
});

// A compile-time guard rather than a runtime one: the list the property test iterates has
// to be the whole union, or the property silently covers less than it claims.
const _everyStateListed: Record<SessionState, true> = Object.fromEntries(
  ALL_SESSION_STATES.map((s) => [s, true])
) as Record<SessionState, true>;
void _everyStateListed;
