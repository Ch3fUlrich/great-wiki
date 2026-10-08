import { describe, expect, it } from 'vitest';
import type { TreeNode } from '$lib/api';
import {
  describeMove,
  destinations,
  dropHref,
  MOVE_PARAM,
  MOVE_REGION_ID,
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

describe('a page dropped onto another in the sidebar', () => {
  const page = node('/a/p', 'Seite P');

  it('opens the same dialog, prefilled, on the dragged page — never a move by itself', () => {
    const href = dropHref(page, node('/b', 'B'));
    expect(href).not.toBeNull();
    const [path, rest] = (href as string).split('?');
    expect(path).toBe('/a/p');
    const [query, fragment] = rest.split('#');
    expect(fragment).toBe(MOVE_REGION_ID);
    const fields = readFields(new URLSearchParams(query));
    // Fields present means "measure it" for the loader: the access change is on screen
    // before there is a button, and the button is the dialog's own.
    expect(fields).toEqual({ ziel: '/b', titel: 'Seite P', adresse: 'p' });
    expect(new URLSearchParams(query).get(MOVE_PARAM)).toBe('1');
  });

  it('is a drop onto the top of the tree when no page is the target', () => {
    expect(readFields(new URLSearchParams((dropHref(page, null) as string).split('?')[1].split('#')[0]))?.ziel).toBe('');
  });

  it('refuses the drops that would be a lie: onto itself, its own subtree, or where it already is', () => {
    expect(dropHref(page, node('/a/p', 'P'))).toBeNull();
    expect(dropHref(page, node('/a/p/kind', 'Kind'))).toBeNull();
    expect(dropHref(page, node('/a', 'A'))).toBeNull();
    expect(dropHref(node('/top', 'Top'), null)).toBeNull();
    // A sibling whose name merely starts the same is a real target.
    expect(dropHref(page, node('/a/pe', 'Pe'))).not.toBeNull();
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
