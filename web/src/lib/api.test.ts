import { afterEach, describe, expect, it, vi } from 'vitest';
import { env } from '$env/dynamic/private';
import { apiGet } from './api';

/**
 * The server-side call to the API, and the header that makes it acceptable.
 *
 * This file exists because of a defect that no test could see and no log would have
 * reported. `proxy_guard` refuses any request arriving without `x-gw-proxy` once the API
 * is bound to anything but loopback — which is every deployment — and `apiGet` sent only
 * the cookie. Every server-rendered page would have had its `/api/me` refused with 403;
 * `+layout.server.ts` deliberately turns a failed `/api/me` into `ANONYMOUS`, so the wiki
 * would have rendered the public view to signed-in people, silently and permanently.
 *
 * It survived because nothing had ever been deployed. The tests all ran against a
 * loopback API that demands no attestation, which is exactly the configuration that
 * cannot notice.
 */

/** A `fetch` that records what it was called with and answers 200 with `{}`. */
function spyFetch() {
  const calls: { url: string; headers: Record<string, string> }[] = [];
  const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({
      url: String(url),
      headers: (init?.headers ?? {}) as Record<string, string>
    });
    return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

const ORIGINAL = { ...env };

afterEach(() => {
  for (const key of Object.keys(env)) delete env[key];
  Object.assign(env, ORIGINAL);
});

describe('apiGet', () => {
  it('attests itself when a proxy secret is configured', async () => {
    env.GW_PROXY_SECRET = 'ein-geheimnis-aus-der-umgebung';
    const { calls, fetchFn } = spyFetch();

    await apiGet(fetchFn, '/api/me', null);

    // The VALUE, not merely the presence of a header. A test that asserted the key alone
    // would pass against an empty string, which the API refuses exactly as it refuses a
    // missing header — so it would prove nothing about the thing that broke.
    expect(calls[0].headers['X-GW-Proxy']).toBe('ein-geheimnis-aus-der-umgebung');
  });

  it('sends no attestation at all when none is configured', async () => {
    delete env.GW_PROXY_SECRET;
    const { calls, fetchFn } = spyFetch();

    await apiGet(fetchFn, '/api/me', null);

    // Development binds the API to loopback, where it demands nothing. An empty header
    // would be *presented* and would be compared, so absent and empty are not the same
    // thing to write — absent is the honest one.
    expect(calls[0].headers).not.toHaveProperty('X-GW-Proxy');
  });

  it('still forwards the caller cookie, alongside the attestation', async () => {
    env.GW_PROXY_SECRET = 'geheim';
    const { calls, fetchFn } = spyFetch();

    await apiGet(fetchFn, '/api/me', 'gw_session=abc');

    expect(calls[0].headers.cookie).toBe('gw_session=abc');
    expect(calls[0].headers['X-GW-Proxy']).toBe('geheim');
  });

  it('reads the API base from the environment, so the container can point it at the service', async () => {
    env.GW_API = 'http://gw-api:8092';
    const { calls, fetchFn } = spyFetch();

    await apiGet(fetchFn, '/api/me', null);

    expect(calls[0].url).toBe('http://gw-api:8092/api/me');
  });

  it('falls back to loopback when no base is configured', async () => {
    delete env.GW_API;
    const { calls, fetchFn } = spyFetch();

    await apiGet(fetchFn, '/api/me', null);

    expect(calls[0].url).toBe('http://127.0.0.1:8092/api/me');
  });
});
