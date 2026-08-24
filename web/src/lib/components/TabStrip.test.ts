import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import TabStrip from './TabStrip.svelte';
import { buildTabs, TAB_PARAM } from '$lib/tabs';
import type { TreeNode } from '$lib/api';

/**
 * The tab strip, server-rendered.
 *
 * `render()` from `svelte/server` is the first response — there is no DOM environment in
 * this project — and that is exactly the point of testing this component through it. The
 * requirement is not "the strip works"; it is that **the strip works before any script
 * arrives**: every tab is a link, every close is a link, and nothing in the first response
 * takes a control out of the keyboard's reach on the promise that a bundle will put it
 * back. A tablist that only becomes operable at hydration is the same defect this
 * repository already records about its own edit button.
 */

const tree: TreeNode[] = [
  {
    path: '/rundgang',
    slug: 'rundgang',
    title: 'Rundgang',
    doc_type: 'page',
    visibility: 'restricted',
    children: []
  }
];

const PANEL = 'gw-panel';

function html(hrefs: string[], hier: string): string {
  const { tabs, active } = buildTabs(hrefs, hier, tree);
  return render(TabStrip, { props: { tabs, active, panelId: PANEL } }).body.replace(
    /<!--.*?-->/g,
    ''
  );
}

/** Every `<a …>` opening tag, so an assertion can be about links and not about text. */
function anchors(out: string): string[] {
  return out.match(/<a\b[^>]*>/g) ?? [];
}

describe('the tab strip, as ARIA', () => {
  it('is a real tablist, and says what it is a list of', () => {
    const out = html(['/', '/graph'], '/graph');
    expect(out).toMatch(/role="tablist"/);
    expect(out).toMatch(/aria-label="Geöffnete Reiter"/);
  });

  it('gives every tab the tab role and points it at the panel it controls', () => {
    const out = html(['/', '/graph', '/aufgaben'], '/graph');
    expect(out.match(/role="tab"/g)).toHaveLength(3);
    expect(out.match(new RegExp(`aria-controls="${PANEL}"`, 'g'))).toHaveLength(3);
  });

  it('announces exactly one tab as the selected one', () => {
    const out = html(['/', '/graph', '/aufgaben'], '/graph');
    expect(out.match(/aria-selected="true"/g)).toHaveLength(1);
    expect(out.match(/aria-selected="false"/g)).toHaveLength(2);
  });

  it('gives the selected tab an id, so the panel can be named by it', () => {
    const out = html(['/', '/graph'], '/graph');
    expect(out).toMatch(/id="gw-reiter-1"/);
  });

  it('marks the selected tab in the markup as well, not by colour alone', () => {
    // The stylesheet gives the active tab weight and a raised, connected shape. What is
    // pinned here is the hook those rules hang on, so a restyle cannot quietly reduce the
    // difference to a hue.
    const out = html(['/', '/graph'], '/graph');
    expect(out).toMatch(/class="[^"]*\baktiv\b/);
  });
});

describe('the tab strip, before any script has arrived', () => {
  it('makes every tab a link carrying the whole workspace', () => {
    const out = html(['/', '/graph'], '/');
    const tabs = anchors(out).filter((tag) => tag.includes('role="tab"'));
    expect(tabs).toHaveLength(2);
    for (const tag of tabs) {
      const href = tag.match(/href="([^"]*)"/)?.[1] ?? '';
      expect(href).toContain(`${TAB_PARAM}=`);
      // Two entries, so the set survives being followed.
      expect(href.match(new RegExp(`${TAB_PARAM}=`, 'g'))).toHaveLength(2);
    }
  });

  it('leaves every tab reachable by keyboard, rather than promising hydration will fix it', () => {
    // The roving tabindex the ARIA pattern asks for is applied ON MOUNT and never
    // server-rendered: `tabindex="-1"` in the first response would take every unselected
    // tab out of the tab order for anybody whose bundle has not arrived, or never will.
    const out = html(['/', '/graph', '/aufgaben'], '/graph');
    expect(out).not.toContain('tabindex="-1"');
  });

  it('closes a tab with a link, named after the tab it closes', () => {
    const out = html(['/', '/rundgang'], '/');
    expect(out).toMatch(/aria-label="Reiter »Rundgang« schließen"/);
    expect(out).toMatch(/aria-label="Reiter »Start« schließen"/);
  });

  it('offers no way to close the only tab there is', () => {
    const out = html([], '/rundgang');
    expect(out).not.toContain('schließen');
  });

  it('opens a new tab with a link, and says so in words', () => {
    const out = html(['/rundgang'], '/rundgang');
    expect(out).toMatch(/aria-label="Neuen Reiter öffnen"/);
    const neu = anchors(out).find((tag) => tag.includes('Neuen Reiter'));
    expect(neu).toMatch(/href="\//);
  });
});

describe('reordering', () => {
  it('offers both directions for a tab with a neighbour on each side', () => {
    const out = html(['/', '/graph', '/aufgaben'], '/graph');
    expect(out).toMatch(/aria-label="Reiter »Graph« nach links verschieben"/);
    expect(out).toMatch(/aria-label="Reiter »Graph« nach rechts verschieben"/);
  });

  it('is a link, so a workspace can be rearranged without a script', () => {
    const out = html(['/', '/graph', '/aufgaben'], '/graph');
    const move = anchors(out).find((tag) => tag.includes('nach links'));
    expect(move).toBeDefined();
    expect(move).toMatch(/href="/);
  });

  it('offers no move past the end, and does not pretend to', () => {
    const out = html(['/', '/graph'], '/');
    expect(out).not.toContain('nach links verschieben');
    expect(out).toContain('nach rechts verschieben');
  });

  it('offers no reordering at all while a single tab is open', () => {
    const out = html([], '/rundgang');
    expect(out).not.toContain('verschieben');
  });
});

describe('what the strip shows', () => {
  it('names a page by its title, and the whole-wiki views by their own names', () => {
    const out = html(['/rundgang', '/aufgaben', '/graph'], '/graph');
    expect(out).toContain('Rundgang');
    expect(out).toContain('Aufgaben');
    expect(out).toContain('Graph');
  });

  it('carries the kind of each tab, so a plugin can style one without parsing its address', () => {
    const out = html(['/rundgang', '/graph'], '/graph');
    expect(out).toMatch(/data-art="dokument"/);
    expect(out).toMatch(/data-art="graph"/);
  });

  it('is interface, not document: it does not print', () => {
    expect(html(['/', '/graph'], '/')).toMatch(/class="[^"]*no-print/);
  });
});
