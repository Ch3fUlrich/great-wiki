import { describe as suite, expect, it, vi } from 'vitest';
import {
  describe,
  describeFailure,
  markAllRead,
  markRead,
  type Notification
} from './notifications';

function n(over: Partial<Notification> = {}): Notification {
  return {
    id: 'e1',
    kind: 'mention',
    created_at: '2026-10-01 10:00:00',
    read: false,
    actor_name: 'Ada',
    page: { path: '/a/b', title: 'Titel' },
    ...over
  };
}

suite('describe', () => {
  it('names actor, action and page title', () => {
    expect(describe(n())).toEqual({
      actor: 'Ada',
      text: 'hat Sie erwähnt auf',
      page: 'Titel'
    });
  });
  it('falls back to the path without a title', () => {
    expect(describe(n({ page: { path: '/a/b', title: null } })).page).toBe('/a/b');
  });
  it('says "Jemand" when no actor is named', () => {
    expect(describe(n({ actor_name: null })).text).toBe('Jemand hat Sie erwähnt auf');
  });
  it('covers every kind with a sentence', () => {
    for (const kind of [
      'comment_reply',
      'mention',
      'page_edited',
      'task_assigned',
      'task_due',
      'invite_accepted',
      'grant_changed'
    ] as const) {
      expect(describe(n({ kind })).text.length).toBeGreaterThan(3);
    }
    expect(describe(n({ kind: 'task_due' })).actor).toBeNull();
  });
  it('words failures', () => {
    expect(describeFailure(401)).toContain('melden Sie sich an');
    expect(describeFailure(500)).toContain('500');
  });
});

suite('client', () => {
  it('posts to the read endpoints', async () => {
    const f = vi.fn(async (..._args: unknown[]) => new Response('', { status: 204 }));
    await markRead(f as unknown as typeof fetch, null, 'a b');
    await markAllRead(f as unknown as typeof fetch, null);
    const urls = f.mock.calls.map((c) => String(c[0]));
    expect(urls[0]).toMatch(/\/api\/notifications\/a%20b\/read$/);
    expect(urls[1]).toMatch(/\/api\/notifications\/read-all$/);
  });
});
