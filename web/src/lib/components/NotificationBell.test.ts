import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NotificationBell from './NotificationBell.svelte';

const html = (count: number | null) =>
  render(NotificationBell, { props: { count } }).body.replace(/<!--.*?-->/g, '');

describe('NotificationBell', () => {
  it('links to the inbox with the number in the label', () => {
    const out = html(3);
    expect(out).toContain('href="/benachrichtigungen"');
    expect(out).toContain('aria-label="Benachrichtigungen, 3 ungelesen"');
  });
  it('renders nothing for zero or an unknown count', () => {
    expect(html(0)).not.toContain('benachrichtigungen');
    expect(html(null)).not.toContain('benachrichtigungen');
  });
});
