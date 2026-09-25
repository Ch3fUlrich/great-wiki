import { describe, expect, it } from 'vitest';
import type { TreeNode } from '$lib/api';
import {
  describeMove,
  destinations,
  forwardApiPath,
  moveApiPath,
  moveHref,
  movePreviewApiPath,
  parentOf,
  readFields,
  readerText
} from './moves';

function node(path: string, title: string, children: TreeNode[] = []): TreeNode {
  return {
    id: path,
    path,
    slug: path.split('/').pop() ?? '',
    title,
    doc_type: 'page',
    visibility: 'public',
    children
  };
}

const tree = [
  node('/a', 'A', [node('/a/p', 'P', [node('/a/p/q', 'Q')]), node('/a/pe', 'Pe')]),
  node('/b', 'B')
];

describe('where a move is asked', () => {
  it('names the page in the address, segment by segment', () => {
    expect(moveApiPath('/a/über uns')).toBe('/api/move/a/%C3%BCber%20uns');
    expect(forwardApiPath('/alt')).toBe('/api/forwards/alt');
  });

  it('asks the preview with the same fields the move sends', () => {
    const url = movePreviewApiPath('/a/p', { ziel: '/b', titel: 'P & Q', adresse: 'p' });
    expect(url).toBe('/api/move/a/p?parent=%2Fb&title=P+%26+Q&slug=p');
  });

  it('opens the dialog with a link whose fragment focuses it', () => {
    expect(moveHref('/a/p')).toBe('/a/p?verschieben=1#gw-verschieben');
  });

  it('tells an opened dialog from a submitted one by whether a title came with it', () => {
    expect(readFields(new URLSearchParams('verschieben=1'))).toBeNull();
    expect(readFields(new URLSearchParams('verschieben=1&titel=%20Neu%20&ziel=&adresse=neu'))).toEqual({
      ziel: '',
      titel: 'Neu',
      adresse: 'neu'
    });
  });
});

describe('the destination picker', () => {
  it('offers the tree it was given, in order, without the page and what is under it', () => {
    expect(destinations(tree, '/a/p').map((d) => [d.path, d.depth])).toEqual([
      ['/a', 0],
      ['/a/pe', 1],
      ['/b', 0]
    ]);
  });

  it('does not mistake a sibling whose name starts the same for a subpage', () => {
    expect(destinations(tree, '/a/p').some((d) => d.path === '/a/pe')).toBe(true);
  });

  it('knows where a page sits now', () => {
    expect(parentOf('/a/p')).toBe('/a');
    expect(parentOf('/a')).toBe('');
  });
});

describe('the words', () => {
  it('names everybody without an account as such', () => {
    expect(readerText({ anonymous: true, name: 'Anonymous', username: null, pages: 2 }, 2)).toBe(
      'Alle ohne Anmeldung — alle Seiten'
    );
  });

  it('says on how many of the moved pages access changes', () => {
    expect(readerText({ anonymous: false, name: 'Anna', username: 'anna', pages: 1 }, 3)).toBe(
      'Anna (anna) — 1 von 3 Seiten'
    );
    expect(readerText({ anonymous: false, name: 'anna', username: 'anna', pages: 1 }, 1)).toBe('anna');
  });

  it('carries the API s own reason inside a 409 and always says nothing moved', () => {
    const text = describeMove(409, 'there is already a page at /b/p');
    expect(text).toContain('/b/p');
    expect(text).toContain('Es wurde nichts verschoben.');
    expect(describeMove(0)).toContain('Es wurde nichts verschoben.');
  });
});
