import { describe, expect, it, vi } from 'vitest';
import { isRedirect } from '@sveltejs/kit';
import { load } from './+page.server';

/**
 * Who reaches the console at all.
 *
 * `/admin` used to render its whole frame for anybody — anonymous visitors included — and
 * let every panel fill itself with a German "could not be loaded". Nothing leaked, because
 * each endpoint refused, but the page never decided whether the caller belonged there.
 *
 * It decides now, first: `/api/me` is asked before anything else, and anybody for whom it
 * does not say `administers: true` is sent to the sign-in page with a 303. The sign-in PAGE,
 * never `/auth/oidc`: a signed-in reader bounced through Authelia would come back as the same
 * reader and be bounced again, for ever. The page tells them who they are signed in as.
 *
 * Fail closed. An `/api/me` that errors, does not answer or answers without the flag is a
 * redirect, not a console.
 */

interface Call {
  url: string;
}

function spyFetch(byPath: Record<string, { status: number; body?: unknown } | 'throw'>) {
  const calls: Call[] = [];
  const fetchFn = vi.fn(async (url: string | URL | Request) => {
    const text = String(url);
    calls.push({ url: text });
    const key = Object.keys(byPath)
      .sort((a, b) => b.length - a.length)
      .find((path) => new URL(text, 'http://api.test').pathname === path);
    const answer = key ? byPath[key] : { status: 200, body: [] };
    if (answer === 'throw') throw new TypeError('fetch failed');
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function event(fetchFn: typeof fetch): any {
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/admin', { headers: { cookie: 'gw_session=abc' } }),
    url: new URL('http://wiki.test/admin?pfad=/raum')
  };
}

const ANONYMOUS_ME = {
  authenticated: false,
  username: null,
  display_name: null,
  email: null,
  groups: [],
  teams: [],
  baseline: 'public',
  login_available: true,
  source: 'anonymous',
  view_as: null,
  administers: false
};

const READER_ME = {
  ...ANONYMOUS_ME,
  authenticated: true,
  username: 'leser',
  display_name: 'Leser',
  source: 'session'
};

const ADMIN_ME = { ...READER_ME, username: 'chef', display_name: 'Chef', administers: true };

async function thrownBy(fetchFn: typeof fetch): Promise<unknown> {
  try {
    await load(event(fetchFn));
  } catch (thrown) {
    return thrown;
  }
  return null;
}

function expectSignInRedirect(thrown: unknown) {
  expect(isRedirect(thrown), `expected a redirect, got ${JSON.stringify(thrown)}`).toBe(true);
  const redirect = thrown as { status: number; location: string };
  expect(redirect.status).toBe(303);
  // Exactly the page, with nothing appended: no return-to parameter, which would be an open
  // redirect to defend, and never `/auth/oidc`, which would loop a signed-in reader.
  expect(redirect.location).toBe('/auth/login');
}

function adminCalls(calls: Call[]) {
  return calls.filter((call) => call.url.includes('/api/admin'));
}

describe('somebody who administers nothing', () => {
  it('is sent to the sign-in page when anonymous, before any admin endpoint is asked', async () => {
    const { calls, fetchFn } = spyFetch({ '/api/me': { status: 200, body: ANONYMOUS_ME } });
    expectSignInRedirect(await thrownBy(fetchFn));
    expect(adminCalls(calls)).toEqual([]);
    // And nothing else either: the tree is not the console's to hand out to anybody.
    expect(calls.map((call) => new URL(call.url, 'http://api.test').pathname)).toEqual([
      '/api/me'
    ]);
  });

  it('is sent to the sign-in page when signed in without administering anything', async () => {
    const { calls, fetchFn } = spyFetch({ '/api/me': { status: 200, body: READER_ME } });
    expectSignInRedirect(await thrownBy(fetchFn));
    expect(adminCalls(calls)).toEqual([]);
  });

  it('forwards the caller‘s cookie, so /api/me answers about THEM', async () => {
    const { fetchFn } = spyFetch({ '/api/me': { status: 200, body: READER_ME } });
    await thrownBy(fetchFn);
    const call = (fetchFn as unknown as ReturnType<typeof vi.fn>).mock.calls[0];
    expect((call?.[1] as RequestInit).headers).toMatchObject({ cookie: 'gw_session=abc' });
  });
});

describe('when /api/me cannot say', () => {
  for (const [label, answer] of [
    ['answers 500', { status: 500 }],
    ['answers 404', { status: 404 }],
    ['does not answer at all', 'throw'],
    ['answers without the flag', { status: 200, body: { ...ADMIN_ME, administers: undefined } }],
    ['answers the flag as a string', { status: 200, body: { ...ADMIN_ME, administers: 'true' } }],
    ['answers null', { status: 200, body: null }]
  ] as const) {
    it(`redirects when it ${label}`, async () => {
      const { calls, fetchFn } = spyFetch({ '/api/me': answer });
      expectSignInRedirect(await thrownBy(fetchFn));
      expect(adminCalls(calls)).toEqual([]);
    });
  }
});

describe('somebody who administers something', () => {
  it('gets the console, with every panel asked for', async () => {
    const { calls, fetchFn } = spyFetch({
      '/api/me': { status: 200, body: ADMIN_ME },
      '/api/admin/audit': { status: 200, body: { entries: [], truncated: false } },
      '/api/admin/acl': { status: 200, body: { path: '/raum' } }
    });
    const data = (await load(event(fetchFn))) as unknown as Record<string, unknown>;

    const asked = calls.map((call) => new URL(call.url, 'http://api.test').pathname);
    expect(asked[0]).toBe('/api/me');
    for (const path of [
      '/api/tree',
      '/api/admin/principals',
      '/api/admin/teams',
      '/api/admin/roles',
      '/api/admin/audit',
      '/api/admin/invites',
      '/api/admin/acl'
    ]) {
      expect(asked).toContain(path);
    }
    expect(data.selectedPath).toBe('/raum');
    expect(data.people).toEqual({ data: [], error: null });
  });

  it('still shows a panel that fails as a sentence, not as a redirect', async () => {
    // A space admin is refused the instance-wide panels. That is a sentence in the panel —
    // they belong in the console, and the redirect is only for those who do not.
    const { fetchFn } = spyFetch({
      '/api/me': { status: 200, body: ADMIN_ME },
      '/api/admin/principals': { status: 403 }
    });
    const data = (await load(event(fetchFn))) as unknown as Record<
      string,
      { data: unknown; error: string | null }
    >;
    expect(data.people.data).toBeNull();
    expect(data.people.error).toContain('Personenliste');
  });
});
