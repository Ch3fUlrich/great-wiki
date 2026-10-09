import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Comments from './Comments.svelte';
import type { Comment, Thread } from '$lib/comments';

const c = (id: string, over: Partial<Thread> = {}): Thread => ({
  id,
  parent_id: null,
  author_name: 'Sergej',
  body: `Text ${id}`,
  anchor: null,
  orphaned: false,
  resolved: false,
  resolved_at: null,
  created_at: '2026-10-08 09:00',
  replies: [],
  ...over
});

const html = (props: { threads: Thread[]; angemeldet?: boolean; fehler?: string | null }) =>
  render(Comments, { props: { path: '/p', angemeldet: false, ...props } }).body;

describe('Comments', () => {
  it('lists open threads and collapses resolved ones with their count', () => {
    const reply: Comment = {
      ...c('r'),
      replies: undefined
    } as unknown as Comment;
    const out = html({
      threads: [
        c('1', { replies: [reply] }),
        c('2', { resolved: true }),
        c('3', { resolved: true })
      ]
    });
    expect(out).toContain('Text 1');
    expect(out).toContain('Text r');
    expect(out).toContain('Erledigt (2)');
    expect(out).toMatch(/<details(?![^>]*\bopen\b)/);
  });

  it('shows the quote and the orphan note for a lost passage', () => {
    const out = html({
      threads: [
        c('1', {
          orphaned: true,
          anchor: { start: 'a', end: 'b', quote: 'der Satz' }
        })
      ]
    });
    expect(out).toContain('der Satz');
    expect(out).toContain('Textstelle nicht mehr vorhanden');
  });

  it('offers the forms and buttons to signed-in users only, and never a delete', () => {
    const threads = [c('1'), c('2', { resolved: true })];
    const anon = html({ threads });
    expect(anon).not.toContain('<form');
    expect(anon).not.toContain('<textarea');
    const user = html({ threads, angemeldet: true });
    expect(user).toContain('Neuer Kommentar');
    expect(user).toContain('Antworten');
    expect(user).toContain('Erledigt</button>');
    expect(user).toContain('Wieder öffnen');
    expect(user.toLowerCase()).not.toMatch(/löschen|delete/);
  });

  it('says so when the list failed, and the form stays available', () => {
    const out = html({
      threads: [],
      fehler: 'Die Kommentare konnten nicht geladen werden (Fehler 500).',
      angemeldet: true
    });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Noch keine Kommentare');
    expect(out).toContain('Neuer Kommentar');
  });

  it('says there are no comments when there are none', () => {
    expect(html({ threads: [] })).toContain('Noch keine Kommentare');
  });
});
