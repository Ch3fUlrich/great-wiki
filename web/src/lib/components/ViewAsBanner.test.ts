import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ViewAsBanner from './ViewAsBanner.svelte';
import { ANONYMOUS, type Me } from '$lib/api';

function html(me: Me): string {
  return render(ViewAsBanner, { props: { me } }).body.replace(/<!--.*?-->/g, '');
}

/** Signed in as the administrator, showing the guest's view. */
const viewing: Me = {
  ...ANONYMOUS,
  authenticated: true,
  // Every identity field of `/api/me` describes the TARGET, because that is who the
  // permission engine ran as. That is exactly why the banner has to exist.
  username: 'gast',
  display_name: 'Gast Konto',
  source: 'session',
  view_as: {
    viewer: { id: 'p1', username: 'chef', display_name: 'Chef' },
    target: { id: 'p2', username: 'gast', display_name: 'Gast Konto' }
  }
};

describe('ViewAsBanner', () => {
  it('renders nothing at all when no substitution is active', () => {
    // On every page of an ordinary session. A banner that leaves an empty bar behind would
    // be a permanent cost paid by everybody for a rare mode.
    expect(html(ANONYMOUS).trim()).toBe('');
    expect(html({ ...ANONYMOUS, authenticated: true, username: 'chef' }).trim()).toBe('');
  });

  it('names both identities', () => {
    // The target alone does not say whose session this is; the viewer alone does not say
    // what is being shown. D-M2-17 asks for both, and this is the assertion that keeps
    // somebody from "simplifying" it to one.
    const out = html(viewing);
    expect(out).toContain('Gast Konto');
    expect(out).toContain('gast');
    expect(out).toContain('Chef');
    expect(out).toContain('chef');
  });

  it('says that writing is refused, rather than leaving it to be discovered', () => {
    expect(html(viewing)).toContain('Änderungen sind in dieser Ansicht gesperrt');
  });

  it('offers the exit as a plain POST form, so it works with no JavaScript', () => {
    // The one failure this mode must not have is an administrator who cannot get out. A
    // button wired up in script is a way out that depends on the script having loaded.
    const out = html(viewing);
    expect(out).toMatch(/<form[^>]*method="post"/);
    expect(out).toContain('action="/api/admin/view-as/exit"');
    expect(out).toContain('Eigene Ansicht wiederherstellen');
  });

  it('announces itself to a screen reader', () => {
    const out = html(viewing);
    expect(out).toMatch(/role="status"/);
    expect(out).toMatch(/aria-live="polite"/);
  });
});
