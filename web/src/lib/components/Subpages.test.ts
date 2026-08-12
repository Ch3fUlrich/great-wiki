import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Subpages from './Subpages.svelte';
import type { TreeNode } from '$lib/api';

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

function html(nodes: TreeNode[]): string {
  return render(Subpages, { props: { nodes } }).body.replace(/<!--.*?-->/g, '');
}

const nodes: TreeNode[] = [
  node('/rundgang/was-schon-geht', 'Was schon geht'),
  node('/rundgang/import-export', 'Import und Export', [
    node('/rundgang/import-export/heikler-text', 'Heikler Text')
  ]),
  node('/rundgang/wer-was-sieht', 'Wer was sieht', [
    node('/rundgang/wer-was-sieht/a', 'A'),
    node('/rundgang/wer-was-sieht/b', 'B')
  ])
];

describe('Subpages', () => {
  it('renders nothing at all for a page with no children', () => {
    // An empty "Unterseiten" heading with a rule above it on every leaf page is a
    // permanent cost paid by every page for a section that has nothing in it.
    expect(html([]).trim()).toBe('');
  });

  it('links every child, in the order the tree gives them', () => {
    const out = html(nodes);
    for (const child of nodes) {
      expect(out).toContain(`href="${child.path}"`);
      expect(out).toContain(child.title);
    }
    expect(out.indexOf('Was schon geht')).toBeLessThan(out.indexOf('Import und Export'));
    expect(out.indexOf('Import und Export')).toBeLessThan(out.indexOf('Wer was sieht'));
  });

  it('says how much is below a child, and counts in German', () => {
    const out = html(nodes);
    expect(out).toContain('1 Unterseite');
    expect(out).toContain('2 Unterseiten');
    // The childless one gets no count, rather than "0 Unterseiten".
    expect(out).not.toContain('0 Unterseiten');
  });

  it('is a named landmark with a heading, not a loose list', () => {
    const out = html(nodes);
    expect(out).toMatch(/<nav[^>]*aria-labelledby="gw-subpages"/);
    expect(out).toMatch(/<h2 id="gw-subpages"[^>]*>\s*Unterseiten\s*<\/h2>/);
  });

  it('needs no JavaScript: one anchor per child and nothing to press', () => {
    const out = html(nodes);
    expect(out).not.toContain('<button');
    expect(out).not.toContain('onclick');
    expect(out.match(/<a /g)).toHaveLength(nodes.length);
  });
});
