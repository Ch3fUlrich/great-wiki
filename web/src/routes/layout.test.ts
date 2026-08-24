import { describe, expect, it } from 'vitest';
import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import Layout from './+layout.svelte';
import { ANONYMOUS, type Me, type TreeNode } from '$lib/api';
import { TAB_PARAM } from '$lib/tabs';

/**
 * The application shell: the header, the page tree, the tab strip and the panel the routed
 * view renders into.
 *
 * Two requirements have no other home, and both are about the FIRST RESPONSE.
 *
 * **A page nothing links to is a page nobody finds.** `/projekte` records that about
 * itself, and it is sharper for `/aufgaben`: D-12 also put a board on every project's home
 * page, so it would be easy to think the global one is a convenience — but a task
 * belonging to no project has no home page to appear on, and the global board is the only
 * place it exists at all. A to-do that exists in exactly one place nobody navigates to is
 * exactly how a to-do goes missing, which is the failure D-6 exists to prevent.
 *
 * **The workspace is server-rendered.** The tab set is a query parameter, so which tabs
 * are open is decided here, in the response, and not by a script that may never arrive.
 * Everything below is rendered by `svelte/server` — there is no DOM in this project — so a
 * control that only works after hydration cannot pass any of it.
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
  {
    me = ANONYMOUS,
    tabHrefs = [],
    hier = '/rundgang',
    nodes = tree
  }: { me?: Me; tabHrefs?: string[]; hier?: string; nodes?: TreeNode[] } = {},
  inhalt = '<p>Inhalt</p>'
): string {
  return render(Layout, {
    props: {
      data: { me, tree: nodes, tabHrefs, hier },
      children: createRawSnippet(() => ({ render: () => inhalt }))
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the header navigation', () => {
  it('names all three whole-wiki views, and links each of them', () => {
    const out = html();
    for (const [label, href] of [
      ['Aufgaben', '/aufgaben'],
      ['Projekte', '/projekte'],
      ['Graph', '/graph']
    ]) {
      expect(out).toContain(`href="${href}"`);
      expect(out).toContain(label);
    }
  });

  it('is a named landmark, so it can be reached without reading down the page', () => {
    expect(html()).toMatch(/<nav[^>]*aria-label="Hauptbereiche"/);
  });

  it('offers them to a reader who is not signed in as well', () => {
    // The board filters itself; being anonymous is a reason to be shown less ON it, never a
    // reason to be unable to find it.
    expect(html()).toContain('href="/aufgaben"');
  });
});

describe('what the shell must not lose', () => {
  it('puts the skip link first, ahead of the tree and the tab strip', () => {
    const out = html();
    expect(out.indexOf('href="#content"')).toBeGreaterThanOrEqual(0);
    expect(out.indexOf('href="#content"')).toBeLessThan(out.indexOf('role="tablist"'));
    expect(out.indexOf('href="#content"')).toBeLessThan(out.indexOf('Seitenbaum'));
  });

  it('still offers the reading preferences and the account menu', () => {
    const out = html();
    expect(out).toMatch(/aria-label="Farbschema"/);
    expect(out).toMatch(/aria-label="Schriftart"/);
    expect(out).toMatch(/aria-label="Konto"/);
    expect(out).toContain('Anmelden');
  });

  it('renders the routed view', () => {
    expect(html({}, '<p>Der Inhalt</p>')).toContain('Der Inhalt');
  });
});

describe('the page tree, now shell furniture rather than page furniture', () => {
  it('is in the shell, on every view, named as the one landmark it always was', () => {
    const out = html();
    expect(out.match(/aria-label="Seitenbaum"/g)).toHaveLength(1);
    expect(out).toContain('href="/rundgang"');
    expect(out).toContain('Tabellen');
  });

  it('marks the page being read, so the tree says where you are', () => {
    expect(html({ hier: '/rundgang' })).toMatch(/aria-current="page"/);
  });

  it('renders nothing but the frame when the tree is empty', () => {
    // A failed `/api/tree` and a wiki with no pages both arrive as an empty list here. The
    // shell must still render — the sidebar is furniture, never a precondition for a page.
    const out = html({ nodes: [] });
    expect(out).toContain('Inhalt');
  });
});

describe('the tab strip', () => {
  it('opens the page being read as a tab, even when the address names no workspace', () => {
    const out = html({ hier: '/rundgang' });
    expect(out).toMatch(/role="tablist"/);
    expect(out.match(/role="tab"/g)).toHaveLength(1);
    expect(out).toContain('Rundgang');
  });

  it('renders every tab the address names, and marks the one being read', () => {
    const out = html({ tabHrefs: ['/rundgang', '/graph', '/aufgaben'], hier: '/graph' });
    expect(out.match(/role="tab"/g)).toHaveLength(3);
    expect(out.match(/aria-selected="true"/g)).toHaveLength(1);
  });

  it('names the panel by the tab that is selected, and the tabs point back at it', () => {
    const out = html({ tabHrefs: ['/rundgang', '/graph'], hier: '/graph' });
    expect(out).toMatch(/role="tabpanel"/);
    expect(out).toMatch(/aria-labelledby="gw-reiter-1"/);
    expect(out).toMatch(/aria-controls="gw-panel"/);
    expect(out).toMatch(/id="gw-panel"/);
  });
});

describe('what the links in the shell carry', () => {
  /** The tab set a rendered `href` carries. `&amp;` is an entity, not a separator. */
  function setOf(out: string, pattern: RegExp): string[] {
    const href = out.match(pattern)?.[1] ?? '';
    return new URLSearchParams(href.replace(/&amp;/g, '&')).getAll(TAB_PARAM);
  }

  it('carries nothing extra on a navigation link while a single tab is open', () => {
    // The overwhelmingly common case, and the reason this is a rule rather than an
    // accident: a wiki nobody has opened a second tab in keeps exactly the addresses it
    // had, in the address bar and in every navigation link on the page.
    //
    // »Neuer Reiter« is deliberately not in this list. It is the one control whose whole
    // job is to make a second tab exist, so it has a two-entry set to carry even when
    // there is only one tab open — which is what the next test pins.
    const out = html({ hier: '/rundgang' });
    for (const href of ['/', '/aufgaben', '/projekte', '/graph', '/rundgang/tabellen']) {
      expect(out).toContain(`href="${href}"`);
    }
  });

  it('is the new-tab control, and only that, which carries a set from a single tab', () => {
    const out = html({ hier: '/rundgang' });
    expect(setOf(out, /href="\/\?([^"]*)"[^>]*aria-label="Neuen Reiter öffnen"/)).toEqual([
      '/rundgang',
      '/'
    ]);
  });

  it('carries the workspace on the tree and the header once a second tab exists', () => {
    // Without this, following a link in the sidebar would close every other tab — and it
    // would do it silently, which is the worst way for a workspace to end.
    const out = html({ tabHrefs: ['/rundgang', '/graph'], hier: '/graph' });
    expect(out.match(/href="\/rundgang\/tabellen[^"]*"/)?.[0]).toContain(`${TAB_PARAM}=`);
    expect(out.match(/href="\/aufgaben[^"]*"/)?.[0]).toContain(`${TAB_PARAM}=`);
  });

  it('replaces the active tab rather than opening one, when a shell link is followed', () => {
    // Following a link is navigation, not opening. The workspace is unchanged around it.
    const out = html({ tabHrefs: ['/rundgang', '/graph'], hier: '/graph' });
    expect(setOf(out, /href="\/aufgaben\?([^"]*)"/)).toEqual(['/rundgang', '/aufgaben']);
    expect(setOf(out, /href="\/rundgang\/tabellen\?([^"]*)"/)).toEqual([
      '/rundgang',
      '/rundgang/tabellen'
    ]);
  });

  it('leaves the skip link alone: it is a fragment, not a place', () => {
    const out = html({ tabHrefs: ['/rundgang', '/graph'], hier: '/graph' });
    expect(out).toContain('href="#content"');
  });
});
