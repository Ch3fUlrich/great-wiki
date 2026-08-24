import { describe, expect, it } from 'vitest';
import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import Layout from './+layout.svelte';
import { ANONYMOUS } from '$lib/api';

/**
 * The header's own navigation — the three views that are about the whole wiki rather than
 * about one page.
 *
 * This exists for one requirement that has no other home: **a page nothing links to is a
 * page nobody finds.** `/projekte` records that about itself, and it is sharper for
 * `/aufgaben`: D-12 also put a board on every project's home page, so it would be easy to
 * think the global one is a convenience — but a task belonging to no project has no home
 * page to appear on, and the global board is the only place it exists at all. A to-do that
 * exists in exactly one place nobody navigates to is exactly how a to-do goes missing, which
 * is the failure D-6 exists to prevent.
 */
function html(): string {
  return render(Layout, {
    props: {
      data: { me: ANONYMOUS },
      children: createRawSnippet(() => ({ render: () => '<p>Inhalt</p>' }))
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
