import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Backlinks from './Backlinks.svelte';
import type { Backlink } from '$lib/api';

/** The component's markup, without Svelte's hydration markers. */
function html(backlinks: Backlink[]): string {
  return render(Backlinks, { props: { backlinks } }).body.replace(/<!--.*?-->/g, '');
}

const backlinks: Backlink[] = [
  { path: '/rundgang/tabellen', title: 'Tabellen' },
  { path: '/rundgang/import-export', title: 'Import und Export' }
];

describe('Backlinks', () => {
  it('renders nothing at all when nothing links here', () => {
    // Most pages in this corpus legitimately have no backlinks; an empty "Verweist
    // hierher" heading with a rule above it would be furniture on nearly every page.
    expect(html([]).trim()).toBe('');
  });

  it('links every backlink, in the order it was given', () => {
    const out = html(backlinks);
    for (const link of backlinks) {
      expect(out).toContain(`href="${link.path}"`);
      expect(out).toContain(link.title);
    }
    expect(out.indexOf('Tabellen')).toBeLessThan(out.indexOf('Import und Export'));
  });

  it('is a named landmark with the German heading the brief specifies', () => {
    const out = html(backlinks);
    expect(out).toMatch(/<nav[^>]*aria-labelledby="gw-backlinks"/);
    expect(out).toMatch(/<h2 id="gw-backlinks"[^>]*>\s*Verweist hierher\s*<\/h2>/);
  });

  it('needs no JavaScript: one anchor per backlink and nothing to press', () => {
    const out = html(backlinks);
    expect(out).not.toContain('<button');
    expect(out).not.toContain('onclick');
    expect(out.match(/<a /g)).toHaveLength(backlinks.length);
  });
});
