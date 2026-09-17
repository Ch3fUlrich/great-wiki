import { describe, expect, it } from 'vitest';
import { flattenPages, matchingPages, PICKER_LIMIT } from './pagePicker';
import type { TreeNode } from '$lib/api';

function node(id: string, path: string, title: string, children: TreeNode[] = []): TreeNode {
  return {
    id,
    path,
    slug: path.slice(path.lastIndexOf('/') + 1),
    title,
    doc_type: 'page',
    visibility: 'restricted',
    children
  };
}

const tree: TreeNode[] = [
  node('id-rundgang', '/rundgang', 'Rundgang', [
    node('id-tabellen', '/rundgang/tabellen', 'Tabellen'),
    node('id-roentgen', '/rundgang/roentgen', 'Röntgen und Größe')
  ]),
  node('id-darm', '/darm', 'Darm')
];

describe('the pages the link dialog offers', () => {
  it('flattens the tree it was handed and never looks anywhere else', () => {
    // The tree IS the permission-filtered answer to "which pages are there"
    // (`Store::tree_for`), so a picker built on it inherits that filtering and cannot have a
    // second, weaker one. A page the caller may not read is not in this array because it was
    // never in the tree — and there is nothing here that counts what is missing, because a
    // count of what was hidden is the same disclosure with the name filed off.
    expect(flattenPages(tree).map((p) => p.path)).toEqual([
      '/rundgang',
      '/rundgang/tabellen',
      '/rundgang/roentgen',
      '/darm'
    ]);
  });

  it('carries each page identity, which is what a link stores', () => {
    // D-5. A picker that offered only paths would be a picker that stores an address, and an
    // address breaks the day the page moves — which is the whole thing this feature exists
    // to stop.
    expect(flattenPages(tree)[0].id).toBe('id-rundgang');
  });

  it('matches a title without caring about case or umlauts', () => {
    // German wiki. Nobody types `Röntgen` into a search field with the umlaut in mind, and a
    // picker that only matched the exact spelling would read as "that page does not exist".
    expect(matchingPages(tree, 'rontgen').map((p) => p.id)).toEqual(['id-roentgen']);
    expect(matchingPages(tree, 'RÖNTGEN').map((p) => p.id)).toEqual(['id-roentgen']);
    expect(matchingPages(tree, 'größe').map((p) => p.id)).toEqual(['id-roentgen']);
  });

  it('matches the address too, because two pages can share a title', () => {
    expect(matchingPages(tree, '/darm').map((p) => p.id)).toEqual(['id-darm']);
  });

  it('offers something before anything is typed', () => {
    // A picker that shows nothing until a query is typed is a search box, and a search box
    // has to be guessed at. Tree order, so what it offers is the shape of the wiki.
    expect(matchingPages(tree, '').map((p) => p.path)[0]).toBe('/rundgang');
  });

  it('never offers more than it can show', () => {
    const many = Array.from({ length: PICKER_LIMIT + 5 }, (_, i) =>
      node(`id-${i}`, `/seite-${i}`, `Seite ${i}`)
    );
    expect(matchingPages(many, 'Seite')).toHaveLength(PICKER_LIMIT);
  });

  it('answers nothing for a query nothing matches, rather than everything', () => {
    expect(matchingPages(tree, 'gibt-es-nicht')).toEqual([]);
  });
});
