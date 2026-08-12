import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Breadcrumb from './Breadcrumb.svelte';
import type { Crumb } from '$lib/pagemeta';

/** The component's markup, without Svelte's hydration markers. */
function html(crumbs: Crumb[]): string {
  return render(Breadcrumb, { props: { crumbs } }).body.replace(/<!--.*?-->/g, '');
}

const crumbs: Crumb[] = [
  { path: '/rundgang', title: 'Rundgang' },
  { path: '/rundgang/import-export', title: 'Import und Export' },
  { path: '/rundgang/import-export/heikler-text', title: 'Heikler Text' }
];

describe('Breadcrumb', () => {
  it('starts at the home page, which is not a document and so is not in the trail', () => {
    const out = html(crumbs);
    expect(out).toMatch(/<a href="\/"[^>]*>\s*Start\s*<\/a>/);
    // First in the list, or it is not a path from the root.
    expect(out.indexOf('Start')).toBeLessThan(out.indexOf('Rundgang'));
  });

  it('renders every level as a real link, in order', () => {
    const out = html(crumbs);
    for (const crumb of crumbs) {
      expect(out).toContain(`href="${crumb.path}"`);
      expect(out).toContain(crumb.title);
    }
    expect(out.indexOf('Rundgang')).toBeLessThan(out.indexOf('Import und Export'));
    expect(out.indexOf('Import und Export')).toBeLessThan(out.indexOf('Heikler Text'));
  });

  it('marks the page you are on, and only that one', () => {
    const out = html(crumbs);
    expect(out.match(/aria-current="page"/g)).toHaveLength(1);
    // On the LAST entry. Matching the count alone would pass with the marker on "Start".
    const marked = out.match(/<a href="([^"]+)"[^>]*aria-current="page"/);
    expect(marked?.[1]).toBe('/rundgang/import-export/heikler-text');
  });

  it('is a named landmark, so it can be skipped and found', () => {
    expect(html(crumbs)).toMatch(/<nav[^>]*aria-label="Pfad"/);
  });

  it('works with the one-crumb fallback, where the tree knew nothing', () => {
    const out = html([{ path: '/geheim/unterseite', title: 'Unterseite' }]);
    expect(out).toContain('href="/geheim/unterseite"');
    expect(out.match(/<a /g)).toHaveLength(2); // Start, and the page itself
  });

  it('needs no JavaScript: every entry is an anchor with an href', () => {
    const out = html(crumbs);
    expect(out).not.toContain('<button');
    expect(out).not.toContain('onclick');
    // Four anchors, four hrefs — nothing here is a link only once a script has run.
    expect(out.match(/<a /g)).toHaveLength(4);
    expect(out.match(/href="/g)).toHaveLength(4);
  });
});
