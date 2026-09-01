import { afterEach, describe, expect, it, vi } from 'vitest';
import { env } from '$env/dynamic/private';
import { apiGet, apiUpload } from './api';

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

/**
 * The upload: the one call in this interface whose body is bytes rather than JSON.
 *
 * `POST /api/attachment/{filename}/{page}` takes the file and nothing else — no multipart, no
 * envelope, no declared type. `gw_api::routes::attachments` says why in its own words: a
 * `Content-Type` in the request is a type the *uploader* chose, and the only way to be sure it
 * is never echoed back is for there to be nowhere to put it. The name travels in the address.
 *
 * So this function exists rather than a `body:` on `apiSend`, which would have JSON-encoded
 * the file. It shares `base()` and `proxySecret()` with the other two deliberately: a second
 * spelling of the attestation is precisely the defect the rest of this file was written about.
 */
describe('apiUpload', () => {
  /** A `fetch` that records the request it was given and answers 201 with a view. */
  function spyUpload(status = 201, body: unknown = { filename: 'befund.pdf' }) {
    const calls: { url: string; method: string; headers: Record<string, string>; body: unknown }[] =
      [];
    const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
      calls.push({
        url: String(url),
        method: init?.method ?? 'GET',
        headers: (init?.headers ?? {}) as Record<string, string>,
        body: init?.body
      });
      return new Response(JSON.stringify(body), {
        status,
        headers: { 'content-type': 'application/json' }
      });
    });
    return { calls, fetchFn: fetchFn as unknown as typeof fetch };
  }

  const datei = () => new Blob([new Uint8Array([0x25, 0x50, 0x44, 0x46])]);

  it('posts the bytes themselves, with no envelope around them', async () => {
    const { calls, fetchFn } = spyUpload();
    const file = datei();

    await apiUpload(fetchFn, '/api/attachment/befund.pdf/rundgang', null, file);

    expect(calls[0].method).toBe('POST');
    expect(calls[0].url).toBe('http://127.0.0.1:8092/api/attachment/befund.pdf/rundgang');
    // The Blob itself, not a JSON string of it and not a FormData wrapper: the API reads the
    // request body as the file.
    expect(calls[0].body).toBe(file);
  });

  it('declares nothing about what the bytes are', async () => {
    // The type is sniffed from the file on the server. Sending the browser-declared one would
    // put a type the uploader chose into the request, which is the thing the API has
    // deliberately left nowhere to store.
    const { calls, fetchFn } = spyUpload();
    await apiUpload(fetchFn, '/api/attachment/befund.pdf/rundgang', null, datei());
    expect(calls[0].headers['content-type']).toBe('application/octet-stream');
  });

  it('carries the caller cookie and the attestation, exactly as the other two calls do', async () => {
    env.GW_PROXY_SECRET = 'geheim';
    const { calls, fetchFn } = spyUpload();

    await apiUpload(fetchFn, '/api/attachment/befund.pdf/rundgang', 'gw_session=abc', datei());

    expect(calls[0].headers.cookie).toBe('gw_session=abc');
    expect(calls[0].headers['X-GW-Proxy']).toBe('geheim');
  });

  it('gives back what a refusal said, so a 415 is not rendered as a bare status', async () => {
    const { fetchFn } = spyUpload(415, { error: 'this wiki stores images, PDFs' });
    const { status, failure } = await apiUpload(
      fetchFn,
      '/api/attachment/x.txt/rundgang',
      null,
      datei()
    );
    expect(status).toBe(415);
    expect(failure?.message).toBe('this wiki stores images, PDFs');
  });

  it('reports "no answer at all" as status 0, apart from any 5xx', async () => {
    // Offline, DNS, a dead proxy. "Not reachable" and "answered with 500" send somebody to
    // different places — the same split `apiSend` and `$lib/adminApi` make.
    const fetchFn = vi.fn(async () => {
      throw new TypeError('fetch failed');
    }) as unknown as typeof fetch;
    const { status, failure } = await apiUpload(fetchFn, '/api/attachment/x/y', null, datei());
    expect(status).toBe(0);
    expect(failure?.status).toBe(0);
  });
});
