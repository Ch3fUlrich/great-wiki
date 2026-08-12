import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type Me, type StoredDocument, type TreeNode } from '$lib/api';
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
  { me = ANONYMOUS, edit = false }: { me?: Me; edit?: boolean } = {}
): string {
  return render(Page, { props: { data: { me, doc, body, tree, edit } } }).body.replace(
    /<!--.*?-->/g,
    ''
  );
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
    expect(out).toMatch(/<a[^>]*href="\?edit=1"/);
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
