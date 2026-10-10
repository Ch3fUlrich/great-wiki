import { describe, expect, it, vi } from 'vitest';
import { loadDataset } from './load';

vi.mock('$env/dynamic/private', () => ({ env: { GW_API: 'http://api' } }));

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

const FIELDS = { fields: [{ key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 }] };

function fakeFetch(rows: Response) {
  const seen: string[] = [];
  const f = vi.fn(async (url: string | URL | Request) => {
    const u = String(url);
    seen.push(u);
    return u.includes('/schema/') ? json(FIELDS) : rows;
  }) as unknown as typeof fetch;
  return { f, seen };
}

describe('loadDataset', () => {
  it('reads schema and the first page, passing sort and filter from the address', async () => {
    const { f, seen } = fakeFetch(json({ rows: [], total: 0 }));
    const out = await loadDataset(f, '/d', new URLSearchParams('sort=name&desc=1&fk=name&fv=ad'), null);
    expect(out.error).toBeNull();
    expect(out.sort).toEqual({ sort: 'name', desc: true });
    expect(out.filter).toEqual({ key: 'name', value: 'ad' });
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('sort=name');
    expect(rowsUrl).toContain('desc=true');
    expect(rowsUrl).toContain('filter=');
  });

  it('ignores a sort key the dataset does not have', async () => {
    const { f, seen } = fakeFetch(json({ rows: [], total: 0 }));
    const out = await loadDataset(f, '/d', new URLSearchParams('sort=nope'), null);
    expect(out.sort).toBeNull();
    expect(seen.find((u) => u.includes('/rows/'))).not.toContain('sort=');
  });

  it('states a failed row read instead of pretending the table is empty', async () => {
    const { f } = fakeFetch(json({ error: 'x' }, 500));
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(out.page).toBeNull();
    expect(out.error).toContain('500');
  });
});
