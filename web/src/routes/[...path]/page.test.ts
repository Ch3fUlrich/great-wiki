import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type StoredDocument, type TreeNode } from '$lib/api';
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

/**
 * `me` comes from the root layout's load and is part of this page's `data` type even
 * though the page itself never reads it; the anonymous reader is the case that matters
 * here anyway, since it is the one whose tree is filtered hardest.
 */
function html(doc: StoredDocument = container): string {
  return render(Page, { props: { data: { me: ANONYMOUS, doc, body, tree } } }).body.replace(
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
