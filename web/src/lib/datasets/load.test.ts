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

/**
 * The saved views (M8c C4). A FAKE THAT ANSWERS PER ENDPOINT WITH A FRESH RESPONSE — the
 * original `fakeFetch` hands one shared Response to every non-schema URL and its body can
 * only be read once, which the views read now competes with for the rows page.
 */
describe('loadDataset views', () => {
  const BOARD_VIEWS = {
    views: [{ id: 'b1', name: 'Status', kind: 'board', config: { group_by: 'status' }, position: 0 }]
  };
  const BOARD_FIELDS = {
    fields: [
      { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
      { key: 'status', label: 'Status', kind: 'select', config: { options: [{ id: 'offen', label: 'Offen' }] }, position: 1 }
    ]
  };

  function viewsFetch(over: { views?: Response; fields?: unknown; rows?: unknown } = {}) {
    const seen: string[] = [];
    const f = vi.fn(async (url: string | URL | Request) => {
      const u = String(url);
      seen.push(u);
      if (u.includes('/schema/')) return json(over.fields ?? BOARD_FIELDS);
      if (u.includes('/views/')) return over.views ?? json(BOARD_VIEWS);
      return json(over.rows ?? { rows: [], total: 0 });
    }) as unknown as typeof fetch;
    return { f, seen };
  }

  it('lists the saved views and picks the one the address names', async () => {
    const { f, seen } = viewsFetch();
    const out = await loadDataset(f, '/d', new URLSearchParams('view=b1'), null);
    expect(seen.some((u) => u.includes('/views/d'))).toBe(true);
    expect(out.views).toEqual(BOARD_VIEWS.views);
    expect(out.view?.id).toBe('b1');
    expect(out.error).toBeNull();
  });

  it('a views list that cannot be read leaves the page working with no views', async () => {
    const { f, seen } = viewsFetch({ views: json({ error: 'x' }, 404) });
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(seen.some((u) => u.includes('/views/d'))).toBe(true);
    expect(out.views).toEqual([]);
    expect(out.view).toBeNull();
    expect(out.error).toBeNull();
  });

  it('an unknown view id falls back to the table', async () => {
    const { f, seen } = viewsFetch();
    const out = await loadDataset(f, '/d', new URLSearchParams('view=nope&sort=name'), null);
    expect(out.view).toBeNull();
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('sort=name');
    expect(rowsUrl).not.toContain('limit=');
  });

  it('a board view asks all its cards at once and ignores the address sort and filter', async () => {
    const { f, seen } = viewsFetch();
    const out = await loadDataset(f, '/d', new URLSearchParams('view=b1&sort=name&desc=1&f=name:x'), null);
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('limit=500');
    expect(rowsUrl).not.toContain('sort=');
    expect(rowsUrl).not.toContain('filter=');
    expect(out.sort).toBeNull();
    expect(out.filter).toBeNull();
    expect(out.view?.kind).toBe('board');
  });

  it('the views list on a page without ?view comes from the same read that answered the rows', async () => {
    const { f, seen } = viewsFetch();
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(out.views).toEqual(BOARD_VIEWS.views);
    expect(out.error).toBeNull();
    expect(seen.filter((u) => u.includes('/views/'))).toHaveLength(1);
  });
});

/**
 * A picked CALENDAR view is read like a board (M8d D3b): a month with holes in it is no
 * calendar, so it wants every row, and the address's sort and filter are the table's.
 */
describe('loadDataset calendar view', () => {
  const CALENDAR_FIELDS = {
    fields: [
      { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
      { key: 'start', label: 'Start', kind: 'date', config: {}, position: 1 }
    ]
  };
  const calendarViews = (config: unknown) => ({
    views: [{ id: 'c1', name: 'Termine', kind: 'calendar', config, position: 0 }]
  });

  function calFetch(config: unknown, rows: unknown = { rows: [], total: 0 }) {
    const seen: string[] = [];
    const f = vi.fn(async (url: string | URL | Request) => {
      const u = String(url);
      seen.push(u);
      if (u.includes('/schema/')) return json(CALENDAR_FIELDS);
      if (u.includes('/views/')) return json(calendarViews(config));
      return json(rows);
    }) as unknown as typeof fetch;
    return { f, seen };
  }

  it('asks all its rows at once and ignores the address sort and filter', async () => {
    const { f, seen } = calFetch({ date: 'start', end: 'ende', title: 'name' });
    const out = await loadDataset(f, '/d', new URLSearchParams('view=c1&sort=name&desc=1&f=name:x'), null);
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('limit=500');
    expect(rowsUrl).not.toContain('sort=');
    expect(rowsUrl).not.toContain('filter=');
    expect(out.sort).toBeNull();
    expect(out.filter).toBeNull();
    expect(out.view?.kind).toBe('calendar');
    expect(out.error).toBeNull();
  });

  it('reads the view list BEFORE the rows, because the view decides the request', async () => {
    const { f, seen } = calFetch({ date: 'start' });
    await loadDataset(f, '/d', new URLSearchParams('view=c1'), null);
    expect(seen.findIndex((u) => u.includes('/views/'))).toBeLessThan(
      seen.findIndex((u) => u.includes('/rows/'))
    );
  });

  it('a calendar the dataset cannot place rows by is read as the table', async () => {
    const { f, seen } = calFetch({});
    const out = await loadDataset(f, '/d', new URLSearchParams('view=c1&sort=name'), null);
    expect(out.view?.id).toBe('c1');
    expect(out.sort).toEqual({ sort: 'name', desc: false });
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('sort=name');
    expect(rowsUrl).not.toContain('limit=');
  });

  it('a month outside the rows read is still the same one read', async () => {
    const { f, seen } = calFetch({ date: 'start' });
    const out = await loadDataset(f, '/d', new URLSearchParams('view=c1&month=2026-03'), null);
    expect(seen.filter((u) => u.includes('/views/'))).toHaveLength(1);
    expect(seen.filter((u) => u.includes('/rows/'))).toHaveLength(1);
    expect(out.error).toBeNull();
  });
});

/**
 * A picked GALLERY view is read like a board (M8d D4b) — every row at once, the address's sort
 * and filter are the table's — AND the page's attachment listing is read beside it, because a
 * cover is only as good as the listing that says what its file is. A listing that cannot be
 * read is EMPTY, not an error: every card then states its placeholder rather than the page
 * failing.
 */
describe('loadDataset gallery view', () => {
  const GALLERY_FIELDS = {
    fields: [
      { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
      { key: 'bild', label: 'Bild', kind: 'file', config: {}, position: 1 }
    ]
  };
  const galleryViews = (config: unknown) => ({
    views: [{ id: 'g1', name: 'Galerie', kind: 'gallery', config, position: 0 }]
  });
  const LISTING = {
    attachments: [
      {
        filename: 'plan.png',
        media_type: 'image/png',
        byte_size: 10,
        uploaded_at: '',
        uploaded_by_name: '',
        href: '/api/attachment/plan.png/d'
      }
    ],
    may_write: false
  };

  function galFetch(
    config: unknown,
    over: { attachments?: Response; rows?: unknown } = {}
  ) {
    const seen: string[] = [];
    const f = vi.fn(async (url: string | URL | Request) => {
      const u = String(url);
      seen.push(u);
      if (u.includes('/schema/')) return json(GALLERY_FIELDS);
      if (u.includes('/views/')) return json(galleryViews(config));
      if (u.includes('/api/attachments/')) return over.attachments ?? json(LISTING);
      return json(over.rows ?? { rows: [], total: 0 });
    }) as unknown as typeof fetch;
    return { f, seen };
  }

  it('asks all its rows at once and ignores the address sort and filter', async () => {
    const { f, seen } = galFetch({ cover: 'bild', title: 'name' });
    const out = await loadDataset(f, '/d', new URLSearchParams('view=g1&sort=name&desc=1&f=name:x'), null);
    const rowsUrl = seen.find((u) => u.includes('/rows/'))!;
    expect(rowsUrl).toContain('limit=500');
    expect(rowsUrl).not.toContain('sort=');
    expect(rowsUrl).not.toContain('filter=');
    expect(out.sort).toBeNull();
    expect(out.filter).toBeNull();
    expect(out.view?.kind).toBe('gallery');
    expect(out.error).toBeNull();
  });

  it('reads the page\'s attachment listing for a picked gallery and exposes it', async () => {
    const { f, seen } = galFetch({ cover: 'bild' });
    const out = await loadDataset(f, '/d', new URLSearchParams('view=g1'), null);
    expect(seen.some((u) => u.includes('/api/attachments/d'))).toBe(true);
    expect(out.attachments).toEqual(LISTING.attachments);
  });

  it('a listing that cannot be read is no list: the page loads and every cover will state its placeholder', async () => {
    const { f } = galFetch({ cover: 'bild' }, { attachments: json({ error: 'x' }, 500) });
    const out = await loadDataset(f, '/d', new URLSearchParams('view=g1'), null);
    expect(out.error).toBeNull();
    expect(out.page).not.toBeNull();
    expect(out.attachments).toEqual([]);
  });

  it('no listing is read when no gallery is picked', async () => {
    const { f, seen } = galFetch({ cover: 'bild' });
    const out = await loadDataset(f, '/d', new URLSearchParams(), null);
    expect(seen.some((u) => u.includes('/api/attachments/'))).toBe(false);
    expect(out.attachments).toEqual([]);
  });
});
