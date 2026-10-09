import { describe, expect, it, vi } from 'vitest';
import { isRedirect } from '@sveltejs/kit';
import { actions, load } from './+page.server';

const ev = (fetchFn: unknown) =>
  ({
    fetch: fetchFn,
    request: new Request('http://x/', { headers: { cookie: 'a=b' } })
  }) as never;

type Call = (...args: unknown[]) => Promise<Response>;

describe('/benachrichtigungen loader', () => {
  it('returns the list', async () => {
    const f = vi.fn<Call>(
      async () => new Response(JSON.stringify({ notifications: [{ id: 'x' }] }))
    );
    const r = (await load(ev(f))) as { notifications: unknown[] };
    expect(r.notifications).toHaveLength(1);
    expect(String(f.mock.calls[0][0])).toContain('/api/notifications?limit=');
  });
  it('words a signed-out 401', async () => {
    const f = vi.fn<Call>(async () => new Response('', { status: 401 }));
    const r = (await load(ev(f))) as { fehler: string };
    expect(r.fehler).toContain('melden Sie sich an');
  });
  it('words an unreachable API', async () => {
    const f = vi.fn<Call>(async () => {
      throw new Error('down');
    });
    const r = (await load(ev(f))) as { fehler: string };
    expect(r.fehler).toContain('keine Antwort');
  });
  it('marks all read and redirects back', async () => {
    const f = vi.fn<Call>(async () => new Response('', { status: 204 }));
    let thrown: unknown;
    try {
      await (actions.alleGelesen as (e: never) => Promise<unknown>)(ev(f));
    } catch (e) {
      thrown = e;
    }
    expect(isRedirect(thrown)).toBe(true);
    expect(String(f.mock.calls[0][0])).toContain('/api/notifications/read-all');
  });
});
