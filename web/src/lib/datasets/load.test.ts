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

  const REL_FIELDS = {
    fields: [
      { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
      { key: 'wer', label: 'Wer', kind: 'relation', config: { multiple: false, target_path: '/d2' }, position: 1 },
      { key: 'wo', label: 'Wo', kind: 'relation', config: { multiple: true, target_path: '/d2' }, position: 2 }
    ]
  };
  const targetRow = { id: 'a', values: { name: 'Ada' }, version: 1, created_at: '', updated_at: '' };

  function relFetch(target: Response) {
    const seen: string[] = [];
    const f = vi.fn(async (url: string | URL | Request) => {
      const u = String(url);
      seen.push(u);
      if (u.includes('/schema/')) return json(REL_FIELDS);
      if (u.includes('/rows/d2')) return target;
      return json({ rows: [{ id: '1', values: { wer: 'a' }, version: 1, created_at: '', updated_at: '' }], total: 1 });
    }) as unknown as typeof fetch;
    return { f, seen };
  }

  it('fetches each relation column\'s target page once and maps its row labels', async () => {
    const { f, seen } = relFetch(json({ rows: [targetRow], total: 1 }));
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(seen.filter((u) => u.includes('/rows/d2'))).toHaveLength(1);
    expect(out.labels).toEqual({ wer: { a: 'Ada' }, wo: { a: 'Ada' } });
  });

  it('survives a failing target: the page loads, the labels are simply absent', async () => {
    const { f } = relFetch(json({ error: 'x' }, 500));
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(out.error).toBeNull();
    expect(out.labels).toEqual({});
  });

  it('asks no target when the dataset has no relation column', async () => {
    const { f, seen } = fakeFetch(json({ rows: [], total: 0 }));
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(seen.filter((u) => u.includes('/rows/'))).toHaveLength(1);
    expect(out.labels).toEqual({});
  });
});
