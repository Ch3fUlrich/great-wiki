import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type TreeNode } from '$lib/api';
import { TAB_PARAM } from '$lib/tabs';

/**
 * The start page — which is now also the NEW TAB page.
 *
 * That second job is what everything below is about. »Neuer Reiter« in the strip opens
 * this address, exactly as a browser's own new tab opens a start page, and you then
 * navigate from here to whatever the tab is for. So a link on this page has to carry the
 * workspace: if it did not, opening a second tab and then using it would close the first
 * one, silently, on the very first click.
 */
const tree: TreeNode[] = [
  {
    path: '/rundgang',
    slug: 'rundgang',
    title: 'Rundgang',
    doc_type: 'page',
    visibility: 'restricted',
    children: [
      {
        path: '/rundgang/tabellen',
        slug: 'tabellen',
        title: 'Tabellen',
        doc_type: 'page',
        visibility: 'restricted',
        children: []
      }
    ]
  }
];

function html(
  { nodes = tree, tabHrefs = [] as string[], hier = '/' } = {}
): string {
  return render(Page, {
    props: {
      // `themen` and the sidebar's own choice come from the root layout and are part of
      // `PageData`; this view reads neither.
      data: {
        me: ANONYMOUS,
        tree: nodes,
        tabHrefs,
        hier,
        themen: [],
        themenFehler: null,
        seitenleiste: 'seiten' as const
      }
    }
  }).body.replace(
    /<!--.*?-->/g,
    ''
  );
}

describe('the start page', () => {
  it('lists every page the reader may see, as links', () => {
    const out = html();
    expect(out).toContain('href="/rundgang"');
    expect(out).toContain('Rundgang');
    expect(out).toContain('href="/rundgang/tabellen"');
  });

  it('does not call its list the Seitenbaum: the shell already has one of those', () => {
    // Two landmarks with one name is one landmark too many, and the shell's is the one
    // that is on every view. This is a directory of the same tree, not a second copy of
    // the navigation.
    expect(html()).not.toContain('Seitenbaum');
    expect(html()).toMatch(/aria-label="Alle Seiten"/);
  });

  it('offers the whole-wiki views, so a new tab can be one of them in one click', () => {
    const out = html();
    for (const href of ['/aufgaben', '/projekte', '/graph']) {
      expect(out).toContain(href);
    }
  });

  it('says a wiki with no pages has none, rather than rendering an empty frame', () => {
    expect(html({ nodes: [] })).toContain('Noch keine Seiten');
  });

  it('carries the workspace on every link it offers, so a new tab survives its first click', () => {
    const out = html({ tabHrefs: ['/graph', '/'], hier: '/' });
    for (const pattern of [/href="\/rundgang\?([^"]*)"/, /href="\/aufgaben\?([^"]*)"/]) {
      const query = out.match(pattern)?.[1] ?? '';
      const set = new URLSearchParams(query.replace(/&amp;/g, '&')).getAll(TAB_PARAM);
      expect(set).toHaveLength(2);
      expect(set[0]).toBe('/graph');
    }
  });

  it('carries nothing while a single tab is open', () => {
    const out = html({ tabHrefs: [], hier: '/' });
    expect(out).toContain('href="/rundgang"');
    expect(out).not.toContain(`${TAB_PARAM}=`);
  });
});
