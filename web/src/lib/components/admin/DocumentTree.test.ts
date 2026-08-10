import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import DocumentTree from './DocumentTree.svelte';
import type { TreeNode } from '$lib/api';

type Props = ComponentProps<typeof DocumentTree>;

function html(props: Props): string {
  return render(DocumentTree, { props }).body.replace(/<!--.*?-->/g, '');
}

function node(path: string, title: string, children: TreeNode[] = []): TreeNode {
  return {
    path,
    slug: path.slice(path.lastIndexOf('/') + 1),
    title,
    doc_type: 'page',
    visibility: 'internal',
    children
  };
}

const nodes: TreeNode[] = [
  node('/handbuch', 'Handbuch', [node('/handbuch/onboarding', 'Onboarding')]),
  node('/oeffentlich', 'Öffentlich')
];

describe('DocumentTree', () => {
  it('renders a tree with one treeitem per document', () => {
    const out = html({ nodes, selected: null, onSelect: () => {} });
    expect(out).toContain('role="tree"');
    expect(out).toContain('Handbuch');
    expect(out).toContain('Onboarding');
    expect(out).toContain('Öffentlich');
    expect((out.match(/role="treeitem"/g) ?? []).length).toBe(3);
  });

  it('opens the branches leading to the selected path', () => {
    // A deep link — /admin?pfad=/handbuch/onboarding — has to arrive with the branch
    // already open, or it lands on a tree that does not show what it selected.
    const out = html({ nodes, selected: '/handbuch/onboarding', onSelect: () => {} });
    expect(out).toMatch(/data-branch="\/handbuch"[^>]*aria-expanded="true"/);
  });

  it('leaves branches closed when nothing is selected', () => {
    const out = html({ nodes, selected: null, onSelect: () => {} });
    expect(out).toMatch(/data-branch="\/handbuch"[^>]*aria-expanded="false"/);
  });

  it('marks the selected node for assistive technology, not by colour alone', () => {
    const out = html({ nodes, selected: '/oeffentlich', onSelect: () => {} });
    expect(out).toMatch(/data-value="\/oeffentlich"[^>]*aria-selected="true"/);
  });

  it('renders a document with no children as a leaf, not an empty branch', () => {
    // `children: []` and no `children` key are different things to Ark: an empty array
    // still makes a branch, which would put a twisty on a page that opens onto nothing.
    const out = html({ nodes: [node('/allein', 'Allein')], selected: null, onSelect: () => {} });
    expect(out).toContain('data-part="item"');
    expect(out).not.toContain('data-part="branch"');
  });

  it('survives an empty tree', () => {
    const out = html({ nodes: [], selected: null, onSelect: () => {} });
    expect(out).toContain('role="tree"');
    expect(out).not.toContain('role="treeitem"');
  });
});
