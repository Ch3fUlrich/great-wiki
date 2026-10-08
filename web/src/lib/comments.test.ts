import { describe, expect, it, vi } from 'vitest';
import * as Y from 'yjs';
import {
  cleanBody,
  commentsApiPath,
  encodeAnchor,
  fromBase64,
  groupThreads,
  passageOf,
  postComment,
  quoteOf,
  setResolved,
  type Thread
} from './comments';

const t = (id: string, over: Partial<Thread> = {}): Thread => ({
  id,
  parent_id: null,
  author_name: 'A',
  body: 'x',
  anchor: null,
  orphaned: false,
  resolved: false,
  resolved_at: null,
  created_at: '2026-10-08',
  replies: [],
  ...over
});

describe('grouping', () => {
  it('splits open from resolved, keeping order; orphaned stays open', () => {
    const g = groupThreads([
      t('1'),
      t('2', { resolved: true }),
      t('3', { orphaned: true, anchor: { start: '', end: '', quote: 'q' } })
    ]);
    expect(g.open.map((x) => x.id)).toEqual(['1', '3']);
    expect(g.resolved.map((x) => x.id)).toEqual(['2']);
  });
  it('passageOf reports the quote and the orphan flag', () => {
    expect(passageOf(t('1'))).toBeNull();
    expect(
      passageOf(t('1', { orphaned: true, anchor: { start: '', end: '', quote: 'q' } }))
    ).toEqual({ quote: 'q', orphaned: true });
  });
});

describe('helpers', () => {
  it('encodes the path per segment without a doubled slash', () => {
    expect(commentsApiPath('/a b/c')).toBe('/api/comments/document/a%20b/c');
  });
  it('cleanBody trims and refuses empty', () => {
    expect(cleanBody('  hi ')).toBe('hi');
    expect(cleanBody('   ')).toBeNull();
  });
  it('quoteOf collapses whitespace and cuts at 200 characters', () => {
    expect(quoteOf(' a \n b ')).toBe('a b');
    expect(Array.from(quoteOf('ä'.repeat(300)))).toHaveLength(200);
  });
});

describe('encodeAnchor', () => {
  it('round-trips through Y.decodeRelativePosition with start assoc >= 0 and end assoc < 0', () => {
    const doc = new Y.Doc();
    const frag = doc.getXmlFragment('content');
    const p = new Y.XmlElement('paragraph');
    const text = new Y.XmlText();
    frag.insert(0, [p]);
    p.insert(0, [text]);
    text.insert(0, 'Hallo schöne Welt');
    const a = encodeAnchor(
      doc,
      Y.createRelativePositionFromTypeIndex(text, 6, -1),
      Y.createRelativePositionFromTypeIndex(text, 12, 0),
      'schöne'
    );
    if (!a) throw new Error('no anchor');
    const s = Y.decodeRelativePosition(fromBase64(a.start));
    const e = Y.decodeRelativePosition(fromBase64(a.end));
    expect(s.assoc).toBeGreaterThanOrEqual(0);
    expect(e.assoc).toBeLessThan(0);
    const sa = Y.createAbsolutePositionFromRelativePosition(s, doc);
    const ea = Y.createAbsolutePositionFromRelativePosition(e, doc);
    expect(sa?.index).toBe(6);
    expect(ea?.index).toBe(12);
    expect(sa?.type).toBe(text);
    expect(a.quote).toBe('schöne');
  });
});

describe('writes', () => {
  it('posts a comment with snake_case fields', async () => {
    const f = vi.fn().mockResolvedValue(new Response('{}', { status: 201 }));
    const r = await postComment(f, '/p', { body: 'x', parentId: 'u' });
    expect(r.status).toBe(201);
    expect(JSON.parse(f.mock.calls[0][1].body)).toEqual({
      body: 'x',
      parent_id: 'u',
      anchor: null
    });
  });
  it('resolve and reopen hit their routes; a dead network is status 0', async () => {
    const f = vi.fn().mockResolvedValue(new Response('{}'));
    await setResolved(f, 'i', true);
    await setResolved(f, 'i', false);
    expect(f.mock.calls.map((c) => c[0])).toEqual([
      '/api/comments/i/resolve',
      '/api/comments/i/reopen'
    ]);
    expect((await setResolved(vi.fn().mockRejectedValue(new Error()), 'i', true)).status).toBe(0);
  });
});
