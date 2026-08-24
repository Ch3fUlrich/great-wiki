import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type Backlink, type Me, type StoredDocument, type TreeNode } from '$lib/api';
import type { BoardNotice, BoardResponse, BoardTask } from '$lib/board';
import type { Block } from '$lib/blocks/render';

/**
 * The whole reader page, rendered exactly as the server renders it.
 *
 * This file exists because the component tests beside it each prove one part in
 * isolation, and the requirement is about the WHOLE page arriving complete in the first
 * response: a reader with JavaScript switched off, or with it still loading, must get the
 * breadcrumb and the subpage list, not an empty container that fills in after hydration.
 * `render()` from `svelte/server` is that first response — there is no DOM environment in
 * this project, so it is also the only thing there is.
 */
function node(path: string, title: string, children: TreeNode[] = []): TreeNode {
  return {
    path,
    slug: path.slice(path.lastIndexOf('/') + 1),
    title,
    doc_type: 'page',
    visibility: 'restricted',
    children
  };
}

const tree: TreeNode[] = [
  node('/rundgang', 'Rundgang', [
    node('/rundgang/import-export', 'Import und Export', [
      node('/rundgang/import-export/heikler-text', 'Heikler Text')
    ]),
    node('/rundgang/tabellen', 'Tabellen')
  ])
];

/** The container page the complaint was about: a parent whose content is its children. */
const container: StoredDocument = {
  id: 'd1',
  path: '/rundgang/import-export',
  parent_path: '/rundgang',
  slug: 'import-export',
  doc_type: 'page',
  title: 'Import und Export',
  language: 'de',
  visibility: 'restricted',
  body: '',
  sort_key: 2
};

const body: Block = {
  kind: 'doc',
  content: [{ kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] }]
};

/** Somebody signed in. Says nothing about whether they may WRITE anything — see below. */
const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  baseline: 'admin',
  source: 'session'
};

/**
 * `me` comes from the root layout's load. The anonymous reader is still the case that
 * matters most here — it is the one whose tree is filtered hardest — but the editor's
 * affordance is decided from `me`, so both are needed.
 */
function html(
  doc: StoredDocument = container,
  {
    me = ANONYMOUS,
    edit = false,
    backlinks = [],
    board = null,
    boardFehler = null,
    hinweis = null
  }: {
    me?: Me;
    edit?: boolean;
    backlinks?: Backlink[];
    board?: BoardResponse | null;
    boardFehler?: string | null;
    hinweis?: BoardNotice | null;
  } = {}
): string {
  return render(Page, {
    props: {
      data: {
        me,
        // From the root layout, like `me`: the workspace the address named. This view
        // renders no strip of its own — the shell does — but it is part of `PageData`.
        tabHrefs: [],
        hier: doc.path,
        doc,
        body,
        tree,
        backlinks,
        edit,
        board,
        boardFehler,
        hinweis,
        zurueck: doc.path,
        now: NOW
      }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

/** One instant for the whole file — the loader captures one for the same reason. */
const NOW = Date.UTC(2026, 7, 24, 12, 0, 0);

/** A board homed at the container page, with one card on it. */
function boardFor(over: Partial<BoardTask> = {}): BoardResponse {
  const card: BoardTask = {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
    assignee_name: null,
    due_at: null,
    position: 0,
    anchored: true,
    page: { path: container.path, title: container.title },
    detached: false,
    created_at: '2026-08-20 09:00:00',
    updated_at: '2026-08-20 09:00:00',
    ...over
  };
  return {
    project: {
      id: 'p1',
      home_path: container.path,
      home_title: container.title,
      tag_id: null,
      created_at: '2026-08-20 09:00:00'
    },
    columns: [
      { status: 'Offen', tasks: card.status === 'Offen' ? [card] : [] },
      { status: 'Läuft', tasks: card.status === 'Läuft' ? [card] : [] },
      { status: 'Fertig', tasks: card.status === 'Fertig' ? [card] : [] }
    ]
  };
}

describe('the reader page, server-rendered', () => {
  it('carries the full breadcrumb in the first response', () => {
    const out = html();
    expect(out).toMatch(/<nav[^>]*aria-label="Pfad"/);
    expect(out).toContain('href="/rundgang"');
    expect(out).toContain('Rundgang');
    expect(out).toContain('href="/rundgang/import-export"');
  });

  it('carries the subpage list in the first response', () => {
    // The complaint this answers: a container page renders a back-link and nothing else,
    // which reads as a broken page. Its children ARE its content.
    const out = html();
    expect(out).toContain('Unterseiten');
    expect(out).toContain('href="/rundgang/import-export/heikler-text"');
    expect(out).toContain('Heikler Text');
  });

  it('states the visibility on the page itself, not only in the admin console', () => {
    const out = html();
    expect(out).toContain('Sichtbarkeit');
    expect(out).toContain('Eingeschränkt');
  });

  it('needs no script: the metadata is markup, not a mount point', () => {
    const out = html();
    expect(out).not.toContain('<script');
    // The three parts, all present with content already in them.
    expect(out).toMatch(/aria-label="Pfad"/);
    expect(out).toMatch(/aria-label="Angaben zu dieser Seite"/);
    expect(out).toMatch(/aria-labelledby="gw-subpages"/);
  });

  it('puts the document language on the document, not on the German chrome', () => {
    // `lang` used to sit on `<main>`, which contained only the body. It now contains a
    // German breadcrumb and a German metadata panel too, and claiming those are English
    // makes a screen reader pronounce "Sichtbarkeit" with English phonemes on 29 of the
    // 35 imported pages.
    const out = html({ ...container, language: 'en' });
    const main = out.match(/<main[^>]*>/)?.[0] ?? '';
    const article = out.match(/<article[^>]*>/)?.[0] ?? '';
    expect(main).not.toContain('lang=');
    expect(article).toContain('lang="en"');
    expect(article).toContain('prose');
  });

  it('renders no subpage section on a leaf, and still renders everything else', () => {
    const leaf: StoredDocument = {
      ...container,
      path: '/rundgang/tabellen',
      slug: 'tabellen',
      title: 'Tabellen'
    };
    const out = html(leaf);
    expect(out).not.toContain('Unterseiten');
    expect(out).toMatch(/aria-label="Pfad"/);
    expect(out).toContain('Sichtbarkeit');
  });

  it('renders no backlinks section when nothing points here — the common case', () => {
    // Most pages in this corpus legitimately have no backlinks yet; the panel must not be
    // furniture paid for by every page that has nothing to show.
    const out = html();
    expect(out).not.toContain('Verweist hierher');
  });

  it('carries the backlinks panel in the first response when there is something to show', () => {
    const out = html(container, {
      backlinks: [{ path: '/rundgang/tabellen', title: 'Tabellen' }]
    });
    expect(out).toMatch(/<h2 id="gw-backlinks"[^>]*>\s*Verweist hierher\s*<\/h2>/);
    expect(out).toContain('href="/rundgang/tabellen"');
    expect(out).toContain('Tabellen');
  });
});

describe('reaching the history', () => {
  it('links to the page history, for a reader as well as for somebody signed in', () => {
    // Reading the history follows reading the page (D-M3-5), so this is not an editing
    // affordance and is not hidden behind one. A history nothing links to is a history
    // nobody finds, which is the complaint this whole feature answers.
    expect(html()).toContain('href="/rundgang/import-export/history"');
    expect(html(container, { me: signedIn })).toContain('href="/rundgang/import-export/history"');
  });

  it('calls it Verlauf, in the interface language', () => {
    expect(html()).toContain('Verlauf');
  });
});

describe('offering the editor', () => {
  it('offers nothing to somebody who is not signed in', () => {
    // Nobody anonymous can write anything in this deployment: write comes only from an
    // explicit grant (D-M2-8), and no grant names `anyone`. A control that can only ever be
    // refused is worse than no control.
    expect(html()).not.toContain('Bearbeiten');
  });

  it('offers a signed-in reader a link, not a button that needs a bundle first', () => {
    // A real `href`, so it works before hydration and in a new tab. Hydration replaces the
    // navigation with an in-place switch; without one, the control is visible and dead for
    // as long as the JavaScript takes to arrive.
    const out = html(container, { me: signedIn });
    expect(out).toContain('Bearbeiten');
    // Absolute rather than the bare `?edit=1` it used to be, for the reason
    // `history/+page.svelte` gives about its own links: a link is then the same string
    // wherever it is rendered. It has to be absolute here anyway — the workspace is
    // appended to it, and appending a tab set to a relative query string would produce an
    // address that means something different depending on where the browser thinks it is.
    expect(out).toMatch(/<a[^>]*href="\/rundgang\/import-export\?edit=1"/);
  });

  it('is honest that the offer is not the answer', () => {
    // The page cannot know whether this person may write — no endpoint says so. The socket
    // decides, on press. So the SSR HTML must not contain an editing surface, a toolbar or
    // anything else that would let somebody start typing before that decision was taken.
    const out = html(container, { me: signedIn, edit: true });
    expect(out).not.toContain('contenteditable');
    expect(out).not.toContain('role="textbox"');
    expect(out).not.toContain('role="toolbar"');
  });

  it('still ships the whole page in the first response when the editor was asked for', () => {
    // The requirement that outranks the feature: a reader must never meet a blank page
    // waiting for JavaScript, and "the reader" includes the person who just clicked
    // Bearbeiten. The editor is a few hundred kilobytes that arrive afterwards; the
    // document is in the HTML either way.
    const out = html(container, { me: signedIn, edit: true });
    expect(out).toContain('Ein Satz.');
    expect(out).toMatch(/<article[^>]*class="prose/);
    expect(out).toMatch(/aria-label="Pfad"/);
    expect(out).toContain('Unterseiten');
  });
});

/**
 * D-12's second placement, on the page itself.
 *
 * The board is proved in `$lib/components/Board.test.ts` — it is the SAME component
 * `/aufgaben` renders, which is how the two are kept from disagreeing. What is proved here
 * is where it sits and when it appears at all: only on a page the endpoint named as a
 * project's home, above the subpage list, and never as furniture on the pages that are
 * nobody's home — which is nearly every page in this wiki.
 */
describe('the board embedded in a project home page', () => {
  it('renders nothing at all on a page that is nobody s home', () => {
    const out = html();
    expect(out).not.toContain('tafel-titel');
    expect(out).not.toContain('>Offen<');
  });

  it('carries the whole board in the first response when this page is a home', () => {
    const out = html(container, { board: boardFor() });
    expect(out).toMatch(/<h2[^>]*>Aufgaben<\/h2>/);
    expect(out).toContain('>Offen<');
    expect(out).toContain('>Läuft<');
    expect(out).toContain('>Fertig<');
    expect(out).toContain('Kabel bestellen');
  });

  it('puts the tasks above the subpage list, because that is what you came for', () => {
    const out = html(container, { board: boardFor() });
    expect(out.indexOf('Kabel bestellen')).toBeLessThan(out.indexOf('Unterseiten'));
  });

  it('moves a card back to THIS page, not to the global board', () => {
    const out = html(container, { me: signedIn, board: boardFor() });
    expect(out).toMatch(/action="\/aufgaben\?\/verschieben"/);
    expect(out).toMatch(
      /name="zurueck"[^>]*value="\/rundgang\/import-export"|value="\/rundgang\/import-export"[^>]*name="zurueck"/
    );
  });

  it('shows a card it cannot move, marked, rather than hiding it', () => {
    const out = html(container, { board: boardFor() });
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('Nur lesbar');
  });

  it('announces a move where it happened', () => {
    const out = html(container, {
      board: boardFor(),
      hinweis: { art: 'ok', text: '»Kabel bestellen« steht jetzt in Läuft.' }
    });
    expect(out).toContain('id="aufgaben-hinweis"');
    expect(out).toContain('steht jetzt in Läuft');
  });

  it('does not claim a page has a board when it merely failed to ask', () => {
    // A failed request cannot tell a project's home page from any other, and nearly every
    // page here is the other. The sentence therefore says "if one belongs here".
    const out = html(container, {
      boardFehler:
        'Falls zu dieser Seite eine Aufgabentafel gehört, konnte sie nicht geladen werden: Fehler 500.'
    });
    expect(out).toContain('Falls zu dieser Seite');
    expect(out).toMatch(/role="alert"/);
    expect(out).not.toContain('>Offen<');
  });
});

/**
 * The reader page inside the workspace shell.
 *
 * The shell (`+layout.svelte`) now owns the page tree and the tab strip, so this view owns
 * exactly two things: the reading column, and the column of facts about the page beside
 * it. What is pinned here is the division — anything this file draws that the shell also
 * draws is a duplicate landmark and a second answer to the same question.
 */
describe('the reader page, as a view inside the shell', () => {
  it('draws no page tree of its own: the shell has one, on every view', () => {
    // Two `nav[aria-label="Seitenbaum"]` on one page is two landmarks with one name, and
    // the second copy of a filtered tree is a second thing that can be wrong about it.
    expect(html()).not.toContain('Seitenbaum');
  });

  it('puts what is true ABOUT the page beside it rather than stacked under it', () => {
    const out = html(container, { backlinks: [{ path: '/rundgang', title: 'Rundgang' }] });
    const kontext = out.match(/<div class="[^"]*kontext[\s\S]*$/)?.[0] ?? '';
    expect(kontext).toContain('Angaben zu dieser Seite');
    expect(kontext).toContain('Unterseiten');
    expect(kontext).toContain('Verweist hierher');
  });

  it('keeps the document itself, and the board, in the reading column', () => {
    const out = html(container, { board: boardFor() });
    const main = out.match(/<main[\s\S]*?<\/main>/)?.[0] ?? '';
    expect(main).toMatch(/<article[^>]*class="prose/);
    expect(main).toContain('Kabel bestellen');
  });

  it('still carries everything in the first response, wherever it now sits', () => {
    const out = html(container, { backlinks: [{ path: '/rundgang', title: 'Rundgang' }] });
    expect(out).not.toContain('<script');
    expect(out).toMatch(/aria-label="Pfad"/);
    expect(out).toMatch(/aria-label="Angaben zu dieser Seite"/);
    expect(out).toMatch(/aria-labelledby="gw-subpages"/);
    expect(out).toMatch(/aria-labelledby="gw-backlinks"/);
  });
});
