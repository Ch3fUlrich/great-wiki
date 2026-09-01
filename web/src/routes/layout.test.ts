import { describe, expect, it } from 'vitest';
import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import Layout from './+layout.svelte';
import { ANONYMOUS, type Me, type TreeNode } from '$lib/api';
import { TAB_PARAM } from '$lib/tabs';
import type { SidebarMode, TopicSummary } from '$lib/topics';

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

const themen: TopicSummary[] = [
  { path: '/format', name: 'Format', display_path: 'Format', documents: 1 },
  { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang', documents: 3 },
  { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen', documents: 1 }
];

function html(
  {
    me = ANONYMOUS,
    tabHrefs = [],
    hier = '/rundgang',
    nodes = tree,
    topics = themen,
    themenFehler = null,
    seitenleiste = 'seiten'
  }: {
    me?: Me;
    tabHrefs?: string[];
    hier?: string;
    nodes?: TreeNode[];
    topics?: TopicSummary[];
    themenFehler?: string | null;
    seitenleiste?: SidebarMode;
  } = {},
  inhalt = '<p>Inhalt</p>'
): string {
  return render(Layout, {
    props: {
      data: { me, tree: nodes, tabHrefs, hier, themen: topics, themenFehler, seitenleiste },
      children: createRawSnippet(() => ({ render: () => inhalt }))
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the header navigation', () => {
  it('names every whole-wiki view, and links each of them', () => {
    const out = html();
    for (const [label, href] of [
      ['Aufgaben', '/aufgaben'],
      ['Projekte', '/projekte'],
      // A page nothing links to is a page nobody finds, and that is sharper for `/themen`
      // than for the other three: D-4 made topics invisible in the graph, so a topic page is
      // the ONLY way a topic is reachable at all.
      ['Themen', '/themen'],
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

describe('the sidebar, and the two ways through the corpus it offers', () => {
  it('shows the page tree until the topics are asked for', () => {
    const out = html();
    expect(out).toMatch(/<nav[^>]*aria-label="Seitenbaum"/);
    expect(out).not.toMatch(/<nav[^>]*aria-label="Themen"/);
  });

  it('shows the topics instead when they are, and only then', () => {
    const out = html({ seitenleiste: 'themen' });
    expect(out).toMatch(/<nav[^>]*aria-label="Themen"/);
    expect(out).not.toMatch(/<nav[^>]*aria-label="Seitenbaum"/);
  });

  it('renders the topics it was handed, and asks for none of its own', () => {
    // The whole of "one query rendered twice": what the sidebar draws is the array the root
    // layout's load fetched once, the same one `/themen` renders. `layout.server.test.ts`
    // counts the requests; this pins that the shell adds nothing to them.
    const out = html({ seitenleiste: 'themen' });
    // Carrying `?seitenleiste=themen` is the point of the next test; what this one pins is
    // that every topic the layout was handed is a link, at the depth it belongs to.
    expect(out).toContain('href="/themen/rundgang?seitenleiste=themen"');
    expect(out).toContain('href="/themen/rundgang/tabellen?seitenleiste=themen"');
    expect(out).toContain('3 Seiten');
  });

  it('switches with links, so the choice works before any script arrives', () => {
    const out = html();
    expect(out).toMatch(/<a[^>]*href="\/rundgang\?seitenleiste=themen"/);
  });

  it('says which half you are looking at, and not by colour alone', () => {
    // `aria-current` is the fact; the styling is the second channel. Without it, the two
    // links are two identical words and nothing says which one you already took.
    const seiten = html();
    expect(seiten).toMatch(/aria-current="true"[^>]*>\s*Seiten|Seiten[\s\S]{0,40}aria-current="true"/);
    expect(html({ seitenleiste: 'themen' }).match(/aria-current="true"/g)).toHaveLength(1);
  });

  it('carries the choice onto every link the shell renders, so it survives a navigation', () => {
    const out = html({ seitenleiste: 'themen' });
    expect(out).toContain('href="/aufgaben?seitenleiste=themen"');
    expect(out).toContain('href="/themen?seitenleiste=themen"');
  });

  it('never conflates a failed request with a wiki nobody has filed anything in', () => {
    const out = html({
      seitenleiste: 'themen',
      topics: [],
      themenFehler: 'Die Themen konnten nicht geladen werden (Fehler 500).'
    });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Keine Themen');
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
