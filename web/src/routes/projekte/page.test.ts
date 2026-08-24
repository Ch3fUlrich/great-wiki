import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type Me } from '$lib/api';
import type { Project } from '$lib/projects';

/**
 * The projects page, rendered exactly as the server renders it.
 *
 * There is no DOM environment in this project, so `render()` from `svelte/server` is the
 * only thing there is — which is why every state this page has lives in the URL or in the
 * loader's return value: the confirmation before a deletion, the notice after a creation and
 * the refusal of a creation are all decided server-side, so all of them arrive in the first
 * response and all of them can be asserted here.
 *
 * **What is NOT tested here is who may see a project.** `/api/projects` answers only the
 * projects whose home page the caller may read, per document, and that property belongs to
 * `Store::projects_for` where it is mutation-tested. What these tests do assert is the half
 * that is this file's business: that the page renders the list it was handed and nothing
 * else — no row it invented, no count, and no hint that anything was left out. A count is
 * the one thing an aggregate view can add that says something about what it hid.
 */
const projects: Project[] = [
  {
    id: 'p1',
    home_path: '/rundgang/tabellen',
    home_title: 'Tabellen',
    tag_id: 't-umbau',
    created_at: '2026-08-20 09:00:00'
  },
  {
    id: 'p2',
    home_path: '/verweisbeispiel',
    home_title: 'Verweisbeispiel',
    tag_id: null,
    created_at: '2026-08-21 11:30:00'
  }
];

const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  source: 'session'
};

interface Options {
  me?: Me;
  list?: Project[];
  error?: string | null;
  confirming?: Project | null;
  created?: Project | null;
  form?: { wo: 'anlegen' | 'loeschen'; fehler: string; startseite: string } | null;
}

function html(options: Options = {}): string {
  return render(Page, {
    props: {
      data: {
        me: options.me ?? signedIn,
        projects: options.list ?? projects,
        error: options.error ?? null,
        confirming: options.confirming ?? null,
        created: options.created ?? null
      },
      form: options.form ?? null
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the list', () => {
  it('names every project by its home page, and links there', () => {
    const out = html();
    expect(out).toContain('Tabellen');
    expect(out).toContain('href="/rundgang/tabellen"');
    expect(out).toContain('Verweisbeispiel');
    expect(out).toContain('href="/verweisbeispiel"');
  });

  it('renders exactly the rows it was handed and invents none', () => {
    // The structural half of "this page does not widen the filter": one row per project the
    // API answered with, and no row from anywhere else. The API decides which projects those
    // are; this file may not add to them and may not drop one either.
    const out = html();
    expect(out.match(/data-projekt="/g)).toHaveLength(projects.length);
    expect(out).toContain('data-projekt="p1"');
    expect(out).toContain('data-projekt="p2"');
  });

  it('renders no count of projects and no hint that anything was left out', () => {
    // The requirement, and the reason it is asserted on the rendered page rather than
    // trusted: `/projekte` is an aggregate view, so a number here would be a number about
    // pages the reader may not read. `gw_api::routes::tasks` makes the same point about a
    // board's response and pins it on the keys; this is the interface's half of it.
    const out = html();
    expect(out).not.toMatch(/\d+\s*(Projekte|Einträge|Ergebnisse)/);
    for (const leak of ['verborgen', 'ausgeblendet', 'nicht sichtbar', 'weitere Projekte']) {
      expect(out).not.toContain(leak);
    }
  });

  it('says in words when a project carries no tag, rather than leaving a blank', () => {
    const out = html();
    expect(out).toContain('t-umbau');
    expect(out).toContain('kein Etikett');
  });

  it('says nothing is here without claiming that no project exists', () => {
    const out = html({ list: [] });
    expect(out).toContain('Hier ist kein Projekt zu sehen');
    // "Es gibt keine Projekte" would be a claim about pages this reader may not read. The
    // empty wiki and the wiki whose every project is somebody else's read the same, exactly
    // as the graph page's empty state does, and the conflation is the point.
    expect(out).not.toMatch(/gibt (es )?(noch )?keine Projekte/);
  });

  it('states a failed request instead of rendering an empty list', () => {
    const out = html({ list: [], error: 'Die Projekte konnten nicht geladen werden (Fehler 500).' });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Hier ist kein Projekt zu sehen');
  });
});

describe('creating one', () => {
  it('is a real form that posts to a form action, so it works before hydration', () => {
    const out = html();
    // A POST to a named action — not a click handler. A control that needs JavaScript to
    // arrive is a control that looks live and does nothing, which is what this repository
    // says about its own edit link.
    expect(out).toMatch(/<form[^>]*method="post"/i);
    expect(out).toMatch(/action="\?\/anlegen"/);
    expect(out).not.toContain('onclick');
    expect(out).not.toContain('onsubmit');
  });

  it('labels the field and says what belongs in it', () => {
    const out = html();
    expect(out).toMatch(/<label[^>]*for="startseite"/);
    expect(out).toMatch(/<input[^>]*id="startseite"/);
    expect(out).toMatch(/name="startseite"/);
    expect(out).toContain('Startseite');
    expect(out).toContain('Neues Projekt');
  });

  it('does not offer the form to somebody who is not signed in, and says why', () => {
    const out = html({ me: ANONYMOUS });
    expect(out).not.toMatch(/action="\?\/anlegen"/);
    expect(out).toContain('angemeldet');
    // The list is still the page, and an anonymous reader gets the projects they may read.
    expect(out).toContain('Tabellen');
  });

  it('shows a refusal in words, announced, and keeps what was typed', () => {
    const out = html({
      form: {
        wo: 'anlegen',
        fehler:
          '»/rundgang/tabellen« ist bereits die Startseite eines Projekts. Für ein neues ' +
          'Projekt bitte eine andere Seite wählen.',
        startseite: '/rundgang/tabellen'
      }
    });
    expect(out).toContain('bereits die Startseite eines Projekts');
    expect(out).toContain('andere Seite');
    // Announced rather than merely coloured: the message is in text and carries a role, so a
    // reader who cannot see the red border is told the same thing.
    expect(out).toMatch(/role="alert"/);
    expect(out).toMatch(/aria-invalid="true"/);
    expect(out).toMatch(/aria-describedby="[^"]*startseite-fehler"/);
    expect(out).toMatch(/id="startseite-fehler"/);
    // Retyping a path somebody already typed is the small insult a failed form usually adds.
    expect(out).toMatch(/value="\/rundgang\/tabellen"/);
  });

  it('still says why a creation failed when the form is no longer offered', () => {
    // Found by submitting the form against a server that was not answering: `me` falls back
    // to nobody, the form is not rendered — and the refusal went with it, so the person got a
    // page that said nothing at all about what they had just pressed. A session that expires
    // between the render and the submit is the same shape and is not hypothetical.
    const out = html({
      me: ANONYMOUS,
      form: {
        wo: 'anlegen',
        fehler: 'Nicht angemeldet — bitte erneut anmelden. Es wurde nichts angelegt.',
        startseite: '/rundgang/tabellen'
      }
    });
    expect(out).toContain('Es wurde nichts angelegt');
    expect(out).toMatch(/role="alert"/);
  });

  it('confirms a creation against the list rather than against the address bar', () => {
    const out = html({ created: projects[0] });
    expect(out).toContain('ist jetzt die Startseite eines Projekts');
    expect(out).toContain('Tabellen');
  });
});

describe('deleting one', () => {
  it('offers the deletion as a link that puts the question in the URL', () => {
    const out = html();
    expect(out.match(/loeschen=/g)).toHaveLength(projects.length);
  });

  it('asks first, names the project, and says what goes with it', () => {
    const out = html({ confirming: projects[1] });
    expect(out).toContain('Verweisbeispiel');
    expect(out).toContain('Löschen');
    // The consequence, stated: a project's own cards go with it, and the pages do not.
    expect(out).toContain('Karten');
    expect(out).toContain('Seiten bleiben');
    expect(out).toMatch(/action="\?\/loeschen"/);
    expect(out).toMatch(/name="id"[^>]*value="p2"|value="p2"[^>]*name="id"/);
    expect(out).toContain('Abbrechen');
  });

  it('offers no deletion at all to somebody who is not signed in', () => {
    expect(html({ me: ANONYMOUS })).not.toContain('loeschen=');
  });
});
