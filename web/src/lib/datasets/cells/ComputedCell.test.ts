import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ComputedCell from './ComputedCell.svelte';
import type { Field } from '../table';

const field = (kind: string): Field => ({ key: 'calc', label: 'Rechnung', kind, config: {}, position: 0 });
const html = (kind: string, value: unknown) =>
  render(ComputedCell, { props: { field: field(kind), value } }).body.replace(/<!--.*?-->/g, '');

describe('ComputedCell', () => {
  it('prints a number with the table formatting', () => {
    expect(html('formula', 1234.5)).toContain('1.234,5');
  });

  it('shows an error object as visible #ERR(reason) text', () => {
    const out = html('formula', { error: '#ERR(divide by zero)' });
    expect(out).toContain('#ERR(divide by zero)');
    expect(out).toContain('class="err"');
  });

  it('prints a markup-looking value as text', () => {
    const out = html('formula', '<img src=x onerror=alert(1)>');
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img');
  });

  it('joins a list rollup and leaves an empty value empty', () => {
    expect(html('rollup', ['a', 'b'])).toContain('a, b');
    expect(html('formula', null)).not.toContain('null');
  });
});
