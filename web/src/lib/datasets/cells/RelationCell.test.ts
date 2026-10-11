import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import RelationCell from './RelationCell.svelte';
import type { Field } from '../table';

const field = (config: unknown): Field => ({
  key: 'wer',
  label: 'Wer',
  kind: 'relation',
  config,
  position: 0
});
const cfg = { multiple: false, target_path: '/d2' };

function html(props: Record<string, unknown>): string {
  return render(RelationCell, {
    props: { field: field(cfg), value: '', error: null, ...props }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('RelationCell', () => {
  it('renders one select for a single relation', () => {
    const out = html({});
    expect(out).toContain('<select');
    expect(out).toContain('name="wer"');
    expect(out).not.toContain('type="checkbox"');
  });

  it('shows a selected row that the fetch no longer lists as absent', () => {
    const out = html({ value: 'r9' });
    expect(out).toContain('(nicht mehr vorhanden)');
    expect(out).toMatch(/<option value="r9"[^>]*selected/);
    expect(out).toContain('value="r9"');
  });

  it('renders checkboxes for a multiple relation and marks the stored ids', () => {
    const out = html({ field: field({ ...cfg, multiple: true }), value: ['a', 'b'] });
    expect(out).not.toContain('<select');
    expect(out).toMatch(/value="a"[^>]*checked/);
    expect(out).toMatch(/value="b"[^>]*checked/);
  });
});
