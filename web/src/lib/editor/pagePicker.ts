import type { TreeNode } from '$lib/api';

/**
 * One page the link dialog can offer, flattened out of the navigation tree.
 *
 * `id` is what a link actually stores (D-5): the page's identity, so that renaming or moving
 * it afterwards cannot break the link. `path` and `title` are what a person picks by.
 */
export interface PageChoice {
  id: string;
  path: string;
  title: string;
}

/** How many matches the dialog offers at once. */
export const PICKER_LIMIT = 12;

/**
 * Every page in `tree`, depth first, as flat choices.
 *
 * **The tree is already the answer to "which pages are there".** `GET /api/tree` is
 * `Store::tree_for`, which filters per document through the same `can()` a page read goes
 * through and skips a whole branch whose root is refused — so a page this caller may not read
 * is not in the tree, is not offered here, and is not counted anywhere. That matters more
 * than it looks: an unfiltered page picker is a whole-corpus existence-and-title oracle for
 * exactly the person in the threat model, somebody with write on one page who wants to know
 * whether `/darm/befund-mueller` exists.
 *
 * **And nothing here says how many were hidden.** A count of what was filtered out is the
 * same disclosure with the name filed off, which is the rule `backlinks_for` and `graph_for`
 * already follow and the Papierkorb's own listing states.
 */
export function flattenPages(tree: TreeNode[]): PageChoice[] {
  const out: PageChoice[] = [];
  const walk = (nodes: TreeNode[]) => {
    for (const node of nodes) {
      out.push({ id: node.id, path: node.path, title: node.title });
      walk(node.children ?? []);
    }
  };
  walk(tree);
  return out;
}

/**
 * The pages whose title or path contains `query`, at most [`PICKER_LIMIT`] of them.
 *
 * Case- and diacritic-insensitive, because this wiki is German and nobody types `Röntgen`
 * into a search field with the umlaut in mind. `localeCompare` is not enough — it orders,
 * it does not fold — so the fold is `normalize('NFD')` with the combining marks stripped,
 * which turns `ö` into `o` and leaves `ß` alone (it has no decomposition; typing `ss` for it
 * is a separate question this does not pretend to answer).
 *
 * An empty query offers the first pages in tree order rather than nothing: the dialog opens
 * showing something to pick, which is the difference between a picker and a search box that
 * has to be guessed at.
 *
 * The path is matched as well as the title because two pages in this corpus really are
 * called "Übersicht", and their addresses are what tells them apart — which is also why the
 * dialog shows the path beside every title.
 */
export function matchingPages(tree: TreeNode[], query: string): PageChoice[] {
  const needle = fold(query);
  const all = flattenPages(tree);
  const hits = needle === '' ? all : all.filter((p) => fold(p.title + ' ' + p.path).includes(needle));
  return hits.slice(0, PICKER_LIMIT);
}

function fold(s: string): string {
  return s
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .trim();
}
