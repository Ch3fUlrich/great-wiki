import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import InvitesPanel from './InvitesPanel.svelte';
import type { CreatedInvite, Invite, Team } from '$lib/adminApi';
import type { TreeNode } from '$lib/api';

type Props = ComponentProps<typeof InvitesPanel>;

function html(props: Props): string {
  return render(InvitesPanel, { props }).body.replace(/<!--.*?-->/g, '');
}

const tree: TreeNode[] = [
  {
    id: 'd1',
    path: '/familie',
    slug: 'familie',
    title: 'Familie',
    doc_type: 'page',
    visibility: 'restricted',
    children: [
      {
        id: 'd2',
        path: '/familie/rezepte',
        slug: 'rezepte',
        title: 'Rezepte',
        doc_type: 'page',
        visibility: 'restricted',
        children: []
      }
    ]
  }
];

const teams: Team[] = [{ slug: 'redaktion', name: 'Redaktion', members: [] }];

const pending: Invite = {
  id: 'i1',
  username: 'oma',
  email: null,
  invited_by: 'p1',
  invited_by_name: 'Sergej Maul',
  path: '/familie',
  permission: 'write',
  team: null,
  created_at: '2026-09-17 10:00:00',
  expires_at: '2026-10-17 10:00:00',
  state: 'pending',
  accepted_principal_id: null
};

const noop = async () => true;

function props(over: Partial<Props> = {}): Props {
  return {
    invites: [pending],
    tree,
    teams,
    error: null,
    created: null,
    canInviteTeam: true,
    onCreate: async () => null,
    onRevoke: noop,
    onDismissCreated: () => {},
    ...over
  };
}

describe('InvitesPanel', () => {
  it('offers the control the console did not have at all: creating an invitation', () => {
    // The whole reason this component exists. `POST /api/admin/invites` shipped with 42
    // Rust tests behind it and no caller, so the only way the interface offered to bring a
    // second person in was "Person anlegen" — which means the owner chooses their password
    // and sends it through chat, exactly what D-M2-3 exists to prevent.
    const out = html(props());
    expect(out).toContain('Einladung erstellen');
  });

  it('lists an outstanding invitation with what it carries and when it runs out', () => {
    const out = html(props());
    expect(out).toContain('oma');
    expect(out).toContain('/familie');
    expect(out).toContain('Schreiben');
    expect(out).toContain('Sergej Maul');
    expect(out).toContain('17.10.2026');
    expect(out).toContain('Offen');
  });

  it('never renders a token or a digest, because the listing carries neither', () => {
    // Belt and braces against a future field: if `InviteSummary` ever grew one, this
    // panel must not be the thing that puts it on screen.
    const out = html(props());
    expect(out).not.toMatch(/\/auth\/invite\/[A-Za-z0-9_-]{8}/);
  });

  it('shows the link exactly once, and says so in words', () => {
    const created: CreatedInvite = { ...pending, delivery: { how: 'shown-once', url: '/auth/invite/tok123' } };
    const out = html(props({ created }));
    expect(out).toContain('/auth/invite/tok123');
    // The sentence a person needs BEFORE they navigate away, not after.
    expect(out).toContain('nur dieses eine Mal');
    expect(out).toContain('Kopieren');
  });

  it('warns that the link is itself the credential', () => {
    const created: CreatedInvite = { ...pending, delivery: { how: 'shown-once', url: '/auth/invite/tok123' } };
    const out = html(props({ created }));
    // Whoever holds it can create the account. That is not obvious from "here is a link".
    expect(out).toContain('Wer den Link hat');
  });

  it('offers withdrawal only for an invitation that is still open', () => {
    const spent: Invite = { ...pending, id: 'i2', username: 'opa', state: 'accepted' };
    const out = html(props({ invites: [pending, spent] }));
    // One row can be withdrawn, the other cannot: a button per open invitation only.
    // Counted by the per-row aria-label, because the word itself also appears in the
    // column header and in the confirmation dialog.
    const buttons = out.match(/aria-label="Einladung für [^"]+ zurückziehen"/g) ?? [];
    expect(buttons.length).toBe(1);
    expect(buttons[0]).toContain('oma');
    expect(out).toContain('Angenommen');
  });

  it('says what to do about an invitation that has already been accepted', () => {
    const spent: Invite = { ...pending, state: 'accepted' };
    const out = html(props({ invites: [spent] }));
    // Withdrawing does nothing to an account that already exists.
    expect(out).toContain('Konto');
  });

  it('withholds the team field from somebody who may not attach one', () => {
    // D-M2-2: a team reaches wherever it has been granted, instance-wide, so a
    // team-carrying invite is instance admins only. A space admin offered the field would
    // fill it in and be refused.
    const out = html(props({ canInviteTeam: false }));
    expect(out).not.toContain('Redaktion');
  });

  it('renders the failure in German rather than an empty panel', () => {
    const out = html(props({ invites: null, error: 'Die Einladungen konnten nicht geladen werden: Dafür fehlen die Rechte (403).' }));
    expect(out).toContain('Dafür fehlen die Rechte (403)');
    expect(out).not.toContain('undefined');
  });

  it('distinguishes "none outstanding" from "could not load them"', () => {
    const none = html(props({ invites: [] }));
    expect(none).toContain('Es ist derzeit niemand eingeladen');
    expect(none).not.toContain('konnten nicht geladen werden');
  });

  it('gives the table a scope on every header, like every other panel here', () => {
    const out = html(props());
    const headers = out.match(/<th\b[^>]*>/g) ?? [];
    expect(headers.length).toBeGreaterThan(0);
    for (const header of headers) expect(header).toContain('scope=');
  });

  it('is written in German throughout, with no English leaking through', () => {
    // Proof 4 of the walkthrough, enforced here rather than by reading it once.
    const out = html(props({ created: { ...pending, delivery: { how: 'shown-once', url: '/auth/invite/t' } } }));
    // The link itself is stripped first: it is a URL, not prose, and `/auth/invite/…` is
    // the address the API serves — translating it would break it.
    const text = out.replace(/<[^>]*>/g, ' ').replace(/\/auth\/invite\/\S*/g, ' ');
    for (const word of ['pending', 'accepted', 'revoked', 'expired', 'invite', 'Invite', 'read', 'write']) {
      expect(text).not.toMatch(new RegExp(`\\b${word}\\b`));
    }
  });
});
