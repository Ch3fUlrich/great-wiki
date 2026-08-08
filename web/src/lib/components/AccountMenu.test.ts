import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import AccountMenu from './AccountMenu.svelte';
import { ANONYMOUS, type Me } from '$lib/api';

function html(me: Me): string {
  return render(AccountMenu, { props: { me } }).body.replace(/<!--.*?-->/g, '');
}

const signedIn: Me = {
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej Maul',
  email: null,
  groups: ['admins'],
  teams: [],
  baseline: 'admin',
  login_available: true
};

describe('AccountMenu', () => {
  it('offers a sign-in link when nobody is signed in', () => {
    const out = html({ ...ANONYMOUS, login_available: true });
    expect(out).toContain('href="/auth/login"');
    expect(out).toContain('Anmelden');
    expect(out).not.toContain('/auth/logout');
  });

  it('signs out with a POST form, not a link', () => {
    // Signing out deletes a server-side row. A link would let any image tag on any page
    // sign somebody out, and would not survive the server refusing GET.
    const out = html(signedIn);
    expect(out).toMatch(/<form[^>]*method="post"/);
    expect(out).toContain('action="/auth/logout"');
    expect(out).toContain('Abmelden');
  });

  it('shows who is signed in and what that reaches', () => {
    const out = html(signedIn);
    expect(out).toContain('Sergej Maul');
    expect(out).toContain('admins');
    expect(out).toContain('Alle Seiten');
  });

  it('offers no sign-in link when no provider is configured', () => {
    // Sending somebody to a login that answers 503 is worse than not offering one.
    const out = html(ANONYMOUS);
    expect(out).not.toContain('/auth/login');
    expect(out).toContain('Nicht angemeldet');
  });
});
