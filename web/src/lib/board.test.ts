import { describe, expect, it } from 'vitest';
import {
  BOARD_NOTICE_ID,
  boardPath,
  columnsOf,
  describeBoard,
  describeDue,
  describeEmbeddedBoard,
  describeMove,
  detachedText,
  dueState,
  formatDue,
  isStatus,
  noticeFor,
  readOnly,
  readOnlyText,
  returnTo,
  STATUSES,
  type BoardResponse,
  type BoardTask
} from './board';
import { ANONYMOUS, type Me } from '$lib/api';

/**
 * The pure half of the board: the three columns, the wording of a due date, the wording of
 * a refusal, and the two guards that keep a query string from deciding something it may not.
 *
 * Everything here is a pure function on purpose. The board is rendered in two places (D-12)
 * and both of them are the SAME component fed by the SAME endpoint — so any rule that could
 * differ between the two is a rule that has to live in one tested function rather than in
 * two pieces of markup. "Is this overdue", "may this card be moved" and "where does a move
 * come back to" are exactly those rules.
 */
const NOW = Date.UTC(2026, 7, 24, 12, 0, 0); // 2026-08-24 12:00 UTC

function task(over: Partial<BoardTask> = {}): BoardTask {
  return {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
    assignee_name: null,
    due_at: null,
    position: 0,
    anchored: true,
    page: { path: '/rundgang/tabellen', title: 'Tabellen' },
    detached: false,
    created_at: '2026-08-20 09:00:00',
    updated_at: '2026-08-20 09:00:00',
    ...over
  };
}

const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  source: 'session'
};

describe('the three columns', () => {
  it('are D-9 s three, in board order, spelled the way the API spells them', () => {
    // Composed ä (U+00E4), not `a` + U+0308. The store's CHECK constraint compares bytes,
    // and gw-store pins the same byte sequence for the same reason.
    expect(STATUSES).toEqual(['Offen', 'Läuft', 'Fertig']);
    expect(STATUSES[1].normalize('NFC')).toBe(STATUSES[1]);
  });

  it('are all three even when the answer carries fewer', () => {
    // D-9 fixed the columns, so a board is three columns whatever the endpoint sent. An
    // empty column is a fact about the board; a MISSING column is a hole in the interface.
    const board: BoardResponse = {
      project: null,
      columns: [{ status: 'Fertig', tasks: [task({ id: 't9', status: 'Fertig' })] }]
    };
    const columns = columnsOf(board);
    expect(columns.map((column) => column.status)).toEqual(['Offen', 'Läuft', 'Fertig']);
    expect(columns[0].tasks).toEqual([]);
    expect(columns[2].tasks).toHaveLength(1);
  });

  it('keeps the order the endpoint answered with inside a column', () => {
    // `board_for` already answers in board order — column, then position, then id — so the
    // only correct thing to do with a column's list is leave it alone.
    const board: BoardResponse = {
      project: null,
      columns: [
        {
          status: 'Offen',
          tasks: [task({ id: 'b', position: 0 }), task({ id: 'a', position: 1 })]
        }
      ]
    };
    expect(columnsOf(board)[0].tasks.map((entry) => entry.id)).toEqual(['b', 'a']);
  });

  it('invents no card: every card in comes out, and nothing else does', () => {
    const board: BoardResponse = {
      project: null,
      columns: [
        { status: 'Offen', tasks: [task({ id: 'a' })] },
        { status: 'Läuft', tasks: [] },
        { status: 'Fertig', tasks: [task({ id: 'b', status: 'Fertig' })] }
      ]
    };
    const ids = columnsOf(board).flatMap((column) => column.tasks.map((entry) => entry.id));
    expect(ids).toEqual(['a', 'b']);
  });

  it('recognises exactly the three statuses and refuses anything else', () => {
    expect(isStatus('Läuft')).toBe(true);
    expect(isStatus('erledigt')).toBe(false);
    expect(isStatus(null)).toBe(false);
    // Decomposed: a Mac's own spelling of the same word. The API normalises it; this guard
    // does not have to accept it, but it must not accept it as a DIFFERENT fourth status.
    expect(isStatus('Läuft')).toBe(false);
  });
});

describe('which board is asked for', () => {
  it('asks the one endpoint, unfiltered, for the global board', () => {
    expect(boardPath({ kind: 'alle' })).toBe('/api/board');
  });

  it('binds it to a project, and to a page, with the same endpoint', () => {
    // D-12: the embedded board IS the global board with the filter bound. Two endpoints
    // would be two answers, and every card is a disclosure surface.
    expect(boardPath({ kind: 'projekt', id: 'p1' })).toBe('/api/board?projekt=p1');
    expect(boardPath({ kind: 'seite', path: '/rundgang/tabellen' })).toBe(
      '/api/board?seite=%2Frundgang%2Ftabellen'
    );
  });

  it('escapes what it is given rather than pasting it in', () => {
    expect(boardPath({ kind: 'projekt', id: 'p 1&x=2' })).toBe('/api/board?projekt=p+1%26x%3D2');
  });
});

describe('a due date', () => {
  it('is nothing at all when there is none', () => {
    expect(dueState(null, NOW)).toBeNull();
    expect(describeDue(null, NOW)).toBe('');
  });

  it('is overdue the day after a date-only deadline, not during it', () => {
    // A bare `2026-08-24` is a whole day, not midnight. Treating it as an instant would
    // mark everything due today as overdue from one second past midnight.
    expect(dueState('2026-08-24', NOW)).toBe('heute');
    expect(dueState('2026-08-23', NOW)).toBe('überfällig');
    expect(dueState('2026-08-25', NOW)).toBe('offen');
  });

  it('takes a time as the deadline when one was given', () => {
    expect(dueState('2026-08-24 09:00:00', NOW)).toBe('überfällig');
    expect(dueState('2026-08-24 18:00:00', NOW)).toBe('heute');
  });

  it('says overdue in WORDS, not only in colour', () => {
    // The line this codebase holds everywhere — the diff views say so about their own
    // colours. A card told apart from its neighbours by a red border alone is a card that
    // is not told apart at all in a black-and-white print, at low vision, or read aloud.
    expect(describeDue('2026-08-23', NOW)).toContain('Überfällig');
    expect(describeDue('2026-08-23', NOW)).toContain('23.08.2026');
    expect(describeDue('2026-08-24', NOW)).toContain('Fällig heute');
    expect(describeDue('2026-08-25', NOW)).toBe('Fällig 25.08.2026');
  });

  it('formats by hand rather than through Intl, day first', () => {
    // Same reasoning as `formatInstant` in `$lib/adminApi`: this string is rendered on the
    // server and then again in the browser, and two ICU builds would disagree about it.
    expect(formatDue('2026-09-01')).toBe('01.09.2026');
    expect(formatDue('2026-09-01 14:30:00')).toBe('01.09.2026, 14:30');
  });

  it('hands back nonsense unchanged rather than rendering NaN', () => {
    expect(formatDue('irgendwann')).toBe('irgendwann');
    expect(dueState('irgendwann', NOW)).toBeNull();
    // The date is kept rather than dropped: that there IS one is a fact, and losing it
    // would be worse than showing a value nobody can parse.
    expect(describeDue('irgendwann', NOW)).toBe('F\u00e4llig irgendwann');
  });
});

describe('a card you may see but not move', () => {
  it('is read-only for a reader who is not signed in, and says which reason', () => {
    // Nothing in this deployment grants write to `anyone`, so this one is knowable from
    // here. It is the same crude answer `[...path]/+page.svelte` gives about its edit link.
    expect(readOnly(task(), ANONYMOUS)).toBe('anmelden');
    expect(readOnly(task(), null)).toBe('anmelden');
  });

  it('is offered to somebody signed in, because nothing on the wire says otherwise', () => {
    expect(readOnly(task(), signedIn)).toBeNull();
  });

  it('honours an explicit refusal from the API over the offer', () => {
    // `movable: false` is not on the committed wire. It is honoured if it ever arrives, so
    // that the per-card read-only marking has one place to become true rather than a new
    // one; until then the offer can be false and the move itself cannot.
    expect(readOnly(task({ movable: false }), signedIn)).toBe('schreibrecht');
    expect(readOnly(task({ movable: true }), signedIn)).toBeNull();
  });

  it('says why in words, both ways', () => {
    expect(readOnlyText('anmelden')).toContain('Nur lesbar');
    expect(readOnlyText('anmelden')).toContain('anmelden');
    expect(readOnlyText('schreibrecht')).toContain('Nur lesbar');
    expect(readOnlyText('schreibrecht')).toContain('Schreibrecht');
  });
});

describe('a detached card', () => {
  it('says the page no longer holds the line, and that the card still stands', () => {
    // D-8. The whole point of keeping it is the due date and the assignee somebody set, so
    // the sentence has to say that rather than merely flagging the card as stale.
    const text = detachedText(task({ detached: true }));
    expect(text).toContain('Abgelöst');
    expect(text).toContain('Tabellen');
    expect(text).toMatch(/Fälligkeit|Zuständigkeit/);
  });

  it('names no page for a card that was never written in one', () => {
    const text = detachedText(task({ detached: true, anchored: false, page: null }));
    expect(text).toContain('Abgelöst');
    expect(text).not.toContain('undefined');
    expect(text).not.toContain('null');
  });
});

describe('where a move comes back to', () => {
  it('returns to the page the card was moved on, carrying the announcement', () => {
    expect(returnTo('/rundgang/tabellen', { verschoben: 't1' })).toBe(
      `/rundgang/tabellen?verschoben=t1#${BOARD_NOTICE_ID}`
    );
  });

  it('keeps a filter that was already in the address', () => {
    expect(returnTo('/aufgaben?projekt=p1', { verschoben: 't1' })).toBe(
      `/aufgaben?projekt=p1&verschoben=t1#${BOARD_NOTICE_ID}`
    );
  });

  it('refuses to leave this site', () => {
    // The value comes off a form field, so it is whatever anybody put there. A redirect
    // that follows it anywhere is an open redirect, and a board is exactly the kind of page
    // somebody gets sent a link to.
    for (const hostile of [
      'https://example.invalid/',
      '//example.invalid/',
      '/\\example.invalid',
      'javascript:alert(1)',
      '',
      null
    ]) {
      expect(returnTo(hostile, { verschoben: 't1' })).toBe(
        `/aufgaben?verschoben=t1#${BOARD_NOTICE_ID}`
      );
    }
  });

  it('drops a stale answer that was already in the address', () => {
    // Otherwise a successful move lands on a page still carrying the refusal from the
    // attempt before it, saying two contradictory things about one card.
    expect(returnTo('/aufgaben?projekt=p1&fehler=403', { verschoben: 't1' })).toBe(
      `/aufgaben?projekt=p1&verschoben=t1#${BOARD_NOTICE_ID}`
    );
    expect(returnTo('/aufgaben?verschoben=t9', { fehler: '403' })).toBe(
      `/aufgaben?fehler=403#${BOARD_NOTICE_ID}`
    );
  });

  it('drops a fragment somebody else put on it', () => {
    expect(returnTo('/aufgaben#woanders', { verschoben: 't1' })).toBe(
      `/aufgaben?verschoben=t1#${BOARD_NOTICE_ID}`
    );
  });
});

describe('what a board says about what just happened on it', () => {
  const moved: BoardResponse = {
    project: null,
    columns: [{ status: 'Läuft', tasks: [task({ id: 't2', title: 'Regal', status: 'Läuft' })] }]
  };

  it('confirms a move against the board that was read, not against the address', () => {
    // Both placements call this, which is what stops them from saying different things
    // about the same event — and a hand-typed id names a card only if it is really there.
    const ok = noticeFor(new URLSearchParams('verschoben=t2'), moved);
    expect(ok?.art).toBe('ok');
    expect(ok?.text).toContain('Regal');
    expect(ok?.text).toContain('Läuft');
    expect(noticeFor(new URLSearchParams('verschoben=t-fremd'), moved)).toBeNull();
  });

  it('turns a refusal into a sentence, and nonsense into nothing at all', () => {
    const bad = noticeFor(new URLSearchParams('fehler=403'), moved);
    expect(bad?.art).toBe('fehler');
    expect(bad?.text).toContain('Schreibrecht');
    expect(noticeFor(new URLSearchParams('fehler=nein'), moved)).toBeNull();
    expect(noticeFor(new URLSearchParams(''), moved)).toBeNull();
  });
});

describe('what a refusal says', () => {
  it('never reports a failed board as an empty one', () => {
    expect(describeBoard(500)).toContain('500');
    expect(describeBoard(0)).toContain('antwortet nicht');
  });

  it('does not announce a board on a page that may well have none', () => {
    // Nearly every page in the wiki is nobody's project home, and a failed request cannot
    // say which kind this one is. "Die Aufgaben konnten nicht geladen werden" on such a page
    // would claim a board the request never established.
    const said = describeEmbeddedBoard(500);
    expect(said).toContain('Falls');
    expect(said).toContain('500');
    expect(describeEmbeddedBoard(0)).toContain('antwortet nicht');
  });

  it('promises, in every branch, that the card did not move', () => {
    for (const status of [0, 400, 401, 403, 404, 409, 500]) {
      expect(describeMove(status)).toMatch(/nicht verschoben|nichts verschoben/);
    }
    expect(describeMove(403)).toContain('Schreibrecht');
    expect(describeMove(404)).toMatch(/gibt es nicht/);
    expect(describeMove(500)).toContain('500');
  });
});
