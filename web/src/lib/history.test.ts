import { describe, expect, it } from 'vitest';
import {
  attributeLabel,
  blockLabel,
  CHANGE_LABEL,
  CHANGE_MARK,
  formatDelta,
  isView,
  parseInstant,
  relativeTime,
  selectPair,
  sizeDelta,
  VIEWS,
  type RevisionSummary
} from './history';

/**
 * The vocabulary and the arithmetic of the history page.
 *
 * Everything here is a pure function on purpose: there is no DOM environment in this
 * project, so anything that has to be *asserted* rather than looked at has to be liftable
 * out of the component. What is left in `+page.svelte` is markup.
 */
function revision(id: string, size: number, parent: string | null = null): RevisionSummary {
  return {
    id,
    parent_id: parent,
    summary: null,
    author_name: 'chef',
    author_is_account: true,
    byte_size: size,
    created_at: '2026-08-20 09:00:00'
  };
}

describe('reading a timestamp', () => {
  it('treats a bare SQLite timestamp as UTC, which is what the database writes', () => {
    expect(parseInstant('2026-08-20 09:00:00')).toBe(Date.UTC(2026, 7, 20, 9, 0, 0));
  });

  it('honours an explicit zone rather than re-reading it as UTC', () => {
    expect(parseInstant('2026-08-20T11:00:00+02:00')).toBe(Date.UTC(2026, 7, 20, 9, 0, 0));
  });

  it('returns null for something that is not a timestamp, rather than NaN', () => {
    expect(parseInstant('irgendwann')).toBeNull();
  });
});

describe('relative time', () => {
  const now = Date.UTC(2026, 7, 20, 12, 0, 0);
  const ago = (ms: number) => new Date(now - ms).toISOString();

  it('says gerade eben for anything under a minute', () => {
    expect(relativeTime(ago(30_000), now)).toBe('gerade eben');
  });

  it('counts in the largest unit that fits, singular and plural', () => {
    expect(relativeTime(ago(60_000), now)).toBe('vor 1 Minute');
    expect(relativeTime(ago(5 * 60_000), now)).toBe('vor 5 Minuten');
    expect(relativeTime(ago(3 * 3_600_000), now)).toBe('vor 3 Stunden');
    expect(relativeTime(ago(24 * 3_600_000), now)).toBe('vor 1 Tag');
    expect(relativeTime(ago(3 * 24 * 3_600_000), now)).toBe('vor 3 Tagen');
    expect(relativeTime(ago(14 * 24 * 3_600_000), now)).toBe('vor 2 Wochen');
    expect(relativeTime(ago(70 * 24 * 3_600_000), now)).toBe('vor 2 Monaten');
    expect(relativeTime(ago(800 * 24 * 3_600_000), now)).toBe('vor 2 Jahren');
  });

  it('does not say "in der Zukunft" when a clock is a few seconds ahead', () => {
    // Server and browser clocks differ, and a revision published one second from now must
    // not render as a negative age.
    expect(relativeTime(new Date(now + 5_000).toISOString(), now)).toBe('gerade eben');
  });

  it('hands back an unreadable timestamp unchanged rather than inventing an age', () => {
    expect(relativeTime('irgendwann', now)).toBe('irgendwann');
  });
});

describe('the size delta', () => {
  const revisions = [revision('c', 1200, 'b'), revision('b', 1000, 'a'), revision('a', 900)];

  it('is measured against the revision this one was published on top of', () => {
    expect(sizeDelta(revisions[0], revisions)).toBe(200);
    expect(sizeDelta(revisions[1], revisions)).toBe(100);
  });

  it('is null for the first revision, which grew from nothing rather than from a version', () => {
    expect(sizeDelta(revisions[2], revisions)).toBeNull();
  });

  it('is null when the parent is not in the list, rather than a delta against zero', () => {
    expect(sizeDelta(revision('x', 500, 'nicht-da'), revisions)).toBeNull();
  });

  it('renders with a sign, a unit and a word for the version that started the page', () => {
    expect(formatDelta(200)).toBe('+200 B');
    expect(formatDelta(-2048)).toBe('−2,0 kB');
    expect(formatDelta(0)).toBe('±0');
    expect(formatDelta(null)).toBe('neu');
  });
});

describe('the vocabulary', () => {
  it('names every kind of change in German and marks it in text as well as colour', () => {
    // The marker is the accessibility requirement: a diff that distinguishes an addition
    // from a removal by colour alone is unreadable to a reader who cannot see the colour.
    for (const kind of ['added', 'removed', 'moved', 'changed'] as const) {
      expect(CHANGE_LABEL[kind]).toMatch(/\p{L}/u);
      expect(CHANGE_MARK[kind].length).toBeGreaterThan(0);
    }
    expect(CHANGE_MARK.added).not.toBe(CHANGE_MARK.removed);
  });

  it('translates the block kinds the document model actually uses', () => {
    expect(blockLabel('paragraph')).toBe('Absatz');
    expect(blockLabel('heading')).toBe('Überschrift');
    expect(blockLabel('bulletList')).toBe('Aufzählung');
    expect(blockLabel('tableCell')).toBe('Tabellenzelle');
  });

  it('falls back to the raw name rather than to an empty cell', () => {
    // An unknown kind is information — "this came from a newer content model" — and
    // blanking it would hide exactly the row somebody is looking at.
    expect(blockLabel('kanban')).toBe('kanban');
    expect(attributeLabel('colspan')).toBe('colspan');
  });

  it('translates the attributes a design diff reports most', () => {
    expect(attributeLabel('level')).toBe('Ebene');
    expect(attributeLabel('alignment')).toBe('Ausrichtung');
    expect(attributeLabel('marks')).toBe('Formatierung');
  });
});

describe('the four views', () => {
  it('accepts exactly the four the page can render', () => {
    for (const view of VIEWS) expect(isView(view)).toBe(true);
    expect(isView('prose')).toBe(false);
    expect(isView(null)).toBe(false);
  });
});

describe('choosing what to compare', () => {
  const list = [revision('c', 1200, 'b'), revision('b', 1000, 'a'), revision('a', 900)];

  it('defaults to the newest change: the newest revision against its parent', () => {
    expect(selectPair(list, null, null)).toEqual({ from: list[1], to: list[0] });
  });

  it('always reads old to new, whichever way the two boxes were ticked', () => {
    expect(selectPair(list, 'c', 'a')).toEqual({ from: list[2], to: list[0] });
    expect(selectPair(list, 'a', 'c')).toEqual({ from: list[2], to: list[0] });
  });

  it('ignores an id that is not in this page history rather than forwarding it', () => {
    // The API refuses another page's revision, and this page must not be the thing that
    // asks. A stray id falls back to the default selection.
    expect(selectPair(list, 'fremd', null)).toEqual({ from: list[1], to: list[0] });
  });

  it('compares the first revision against nothing at all', () => {
    expect(selectPair([list[2]], null, null)).toEqual({ from: null, to: list[2] });
  });

  it('has nothing to compare when there is no history', () => {
    expect(selectPair([], null, null)).toEqual({ from: null, to: null });
  });
});
