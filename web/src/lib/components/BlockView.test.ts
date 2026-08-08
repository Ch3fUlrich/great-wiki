import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import BlockView from './BlockView.svelte';
import type { Block } from '$lib/blocks/render';

/// The component's markup, without the hydration markers Svelte interleaves — they are an
/// implementation detail and would make every assertion about structure unreadable.
function html(block: Block): string {
  return render(BlockView, { props: { block } }).body.replace(/<!--.*?-->/g, '');
}

/// A cell as the converter produces one: block content, so the text sits in a paragraph.
function cell(kind: 'tableHeader' | 'tableCell', text: string, align?: string): Block {
  return {
    kind,
    attrs: align ? { align } : undefined,
    content: [{ kind: 'paragraph', content: [{ kind: 'text', text }] }]
  };
}

const table: Block = {
  kind: 'table',
  content: [
    {
      kind: 'tableRow',
      content: [cell('tableHeader', 'Feld'), cell('tableHeader', 'Wert', 'right')]
    },
    { kind: 'tableRow', content: [cell('tableCell', 'Länge'), cell('tableCell', '42', 'right')] },
    { kind: 'tableRow', content: [cell('tableCell', 'Breite'), cell('tableCell', '7', 'right')] }
  ]
};

describe('BlockView', () => {
  it('renders a table as a table, not as paragraphs', () => {
    const out = html(table);
    expect(out).toContain('<table>');
    expect(out).toContain('<thead>');
    expect(out).toContain('<tbody>');
    expect(out.match(/<tr>/g)).toHaveLength(3);
    expect(out.match(/<th\b/g)).toHaveLength(2);
    expect(out.match(/<td\b/g)).toHaveLength(4);
    for (const text of ['Feld', 'Wert', 'Länge', '42', 'Breite', '7']) {
      expect(out).toContain(text);
    }
  });

  it('gives every header cell a scope, or the table is unlabelled cells to a screen reader', () => {
    const out = html(table);
    expect(out.match(/<th[^>]*scope="col"/g)).toHaveLength(2);
  });

  it('applies column alignment, and adds no style where the column states none', () => {
    const out = html(table);
    // The right-aligned column: its header and both of its body cells.
    expect(out.match(/text-align:\s*right/g)).toHaveLength(3);
    // The first column states no alignment, so it must not be styled into one.
    expect(out).not.toMatch(/<th(?![^>]*scope="col"[^>]*style)[^>]*style="[^"]*text-align:\s*left/);
    expect(out.match(/text-align/g)).toHaveLength(3);
  });

  it('puts the table in a scroll region a keyboard user can reach and a screen reader can name', () => {
    // A wide table must scroll inside its own box; the page body scrolling sideways is
    // what makes a document unreadable on a phone. A scrollable box is only reachable
    // without a mouse if it is focusable, and only announced if it is named.
    const out = html(table);
    const wrapper = out.match(/<div[^>]*>/)?.[0] ?? '';
    expect(wrapper).toContain('tabindex="0"');
    expect(wrapper).toContain('role="region"');
    expect(wrapper).toMatch(/aria-label="[^"]+"/);
  });

  it('still renders the blocks it always did', () => {
    expect(html({ kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] })).toContain(
      '<p>Ein Satz.</p>'
    );
    expect(
      html({ kind: 'heading', attrs: { level: 2 }, content: [{ kind: 'text', text: 'Größe' }] })
    ).toMatch(/<h2 id="groesse">/);
  });

  it('skips a kind it does not know rather than rendering it raw', () => {
    const unknown = { kind: 'mystery', text: '<script>' } as unknown as Block;
    expect(html(unknown).trim()).toBe('');
  });
});
