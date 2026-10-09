import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import type { Notification } from '$lib/notifications';

const mk = (over: Partial<Notification> = {}): Notification => ({
  id: 'e1',
  kind: 'mention',
  created_at: '2026-10-01 10:00:00',
  read: false,
  actor_name: 'Ada',
  page: { path: '/a/b', title: 'Titel' },
  ...over
});

function html(data: { notifications: Notification[]; fehler: string | null }): string {
  return render(Page, { props: { data } } as never).body.replace(/<!--.*?-->/g, '');
}

describe('/benachrichtigungen', () => {
  it('lists, links the page and styles unread', () => {
    const out = html({
      notifications: [mk(), mk({ id: 'e2', read: true })],
      fehler: null
    });
    expect(out).toContain('href="/a/b"');
    expect(out).toContain('Ada');
    expect(out).toContain('unread');
    expect(out).toContain('Alle als gelesen markieren');
  });
  it('shows an empty state', () => {
    const out = html({ notifications: [], fehler: null });
    expect(out).toContain('Keine Benachrichtigungen');
    expect(out).not.toContain('Alle als gelesen');
  });
  it('states a failure instead of an empty inbox', () => {
    const out = html({ notifications: [], fehler: 'Bitte melden Sie sich an' });
    expect(out).toContain('Bitte melden Sie sich an');
    expect(out).not.toContain('Keine Benachrichtigungen');
  });
});
