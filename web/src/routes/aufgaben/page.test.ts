import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type Me } from '$lib/api';
import { type BoardNotice, type BoardResponse, type BoardTask } from '$lib/board';
import type { Project } from '$lib/projects';

/**
 * The global board's page, rendered exactly as the server renders it.
 *
 * The board itself is proved in `$lib/components/Board.test.ts` — it is the SAME component
 * the embedded board uses, which is how D-12's two placements are kept from disagreeing. What
 * is proved here is the frame this page puts around it: the project filter, which is a plain
 * GET form and therefore works before hydration, and the two things this page must not do —
 * conflate a failed request with an empty board, and put a number on an aggregate view.
 */
const NOW = Date.UTC(2026, 7, 24, 12, 0, 0);

const projects: Project[] = [
  {
    id: 'p1',
    home_path: '/rundgang/tabellen',
    home_title: 'Tabellen',
    tag_id: null,
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

function task(over: Partial<BoardTask> = {}): BoardTask {
  return {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
    assignee_name: null,
    due_at: null,
    position: 0,
    anchored: true,
    page: { path: '/rundgang/tabellen', title: 'Tabellen' },
    detached: false,
    created_at: '2026-08-20 09:00:00',
    updated_at: '2026-08-20 09:00:00',
    ...over
  };
}

const board: BoardResponse = {
  project: null,
  columns: [
    { status: 'Offen', tasks: [task()] },
    { status: 'Läuft', tasks: [] },
    { status: 'Fertig', tasks: [] }
  ]
};

const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  source: 'session'
};

interface Options {
  me?: Me;
  board?: BoardResponse;
  projects?: Project[];
  projekteFehler?: string | null;
  projekt?: Project | null;
  filterUnbekannt?: boolean;
  fehler?: string | null;
  hinweis?: BoardNotice | null;
  zurueck?: string;
}

function html(options: Options = {}): string {
  return render(Page, {
    props: {
      data: {
        me: options.me ?? signedIn,
        // The shell's own data, merged in from the root layout: the page tree the
        // sidebar draws, and the workspace the address named. This view reads none of it,
        // but it is part of `PageData` and the type says so — the same reason `me` is here.
        tree: [],
        tabHrefs: [],
        // `themen` is that one query, which the sidebar renders beside every view; this one
        // does not read it, but it is part of `PageData` and the type says so.
        themen: [],
        themenFehler: null,
        seitenleiste: 'seiten' as const,
        hier: '/aufgaben',
        board: options.board ?? board,
        projects: options.projects ?? projects,
        projekteFehler: options.projekteFehler ?? null,
        projekt: options.projekt ?? null,
        filterUnbekannt: options.filterUnbekannt ?? false,
        fehler: options.fehler ?? null,
        hinweis: options.hinweis ?? null,
        zurueck: options.zurueck ?? '/aufgaben',
        now: NOW
      }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the page', () => {
  it('is called Aufgaben and shows the board it was handed', () => {
    const out = html();
    expect(out).toMatch(/<h1[^>]*>Aufgaben<\/h1>/);
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('>Offen<');
    expect(out).toContain('>Läuft<');
    expect(out).toContain('>Fertig<');
  });

  it('renders no count and no hint that anything was left out', () => {
    // An aggregate view, so a number here would be a number about pages the reader may not
    // read. `/projekte` makes the same point about its own list; the API pins the absence
    // structurally on its keys.
    const out = html();
    expect(out).not.toMatch(/\d+\s*(Aufgaben|Karten|Einträge|Ergebnisse)/);
    for (const leak of ['verborgen', 'ausgeblendet', 'nicht sichtbar', 'weitere Aufgaben']) {
      expect(out).not.toContain(leak);
    }
  });

  it('states a failed request instead of an empty board', () => {
    const out = html({
      board: { project: null, columns: [] },
      fehler: 'Die Aufgaben konnten nicht geladen werden (Fehler 500).'
    });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Hier ist keine Aufgabe zu sehen');
  });
});

describe('the project filter', () => {
  it('is a plain GET form, so choosing a project needs no script', () => {
    const out = html();
    expect(out).toMatch(/<form[^>]*method="get"/i);
    expect(out).toMatch(/action="\/aufgaben"/);
    expect(out).not.toContain('onchange=');
    expect(out).not.toContain('onsubmit=');
  });

  it('labels the control and offers every project the reader was given, plus all of them', () => {
    const out = html();
    expect(out).toMatch(/<label[^>]*for="projekt"/);
    expect(out).toMatch(/<select[^>]*id="projekt"/);
    expect(out).toMatch(/name="projekt"/);
    expect(out).toContain('Alle Projekte');
    expect(out).toContain('Tabellen');
    expect(out).toContain('Verweisbeispiel');
    // One option per project, and one for "no filter". Nothing invented, nothing dropped.
    expect(out.match(/<option/g)).toHaveLength(projects.length + 1);
  });

  it('marks the project currently being shown as the chosen one', () => {
    const out = html({ projekt: projects[1] });
    expect(out).toMatch(/<option[^>]*value="p2"[^>]*selected/);
    expect(out).toMatch(/<h2[^>]*>Aufgaben zu »Verweisbeispiel«<\/h2>/);
  });

  it('offers no filter at all when there is no project to filter by', () => {
    const out = html({ projects: [] });
    expect(out).not.toContain('<select');
    // The board is still the page.
    expect(out).toContain('Kabel bestellen');
  });

  it('says the options are missing rather than claiming there are no projects', () => {
    const out = html({
      projects: [],
      projekteFehler: 'Die Projekte konnten nicht geladen werden (Fehler 500).'
    });
    expect(out).toContain('Fehler 500');
    expect(out).toMatch(/role="alert"/);
    expect(out).not.toMatch(/gibt (es )?(noch )?keine Projekte/);
    expect(out).toContain('Kabel bestellen');
  });

  it('says a filter it could not honour was not honoured, and confirms nothing about it', () => {
    const out = html({ filterUnbekannt: true });
    expect(out).toMatch(/Es werden alle\s+Aufgaben gezeigt/);
    expect(out).toMatch(/role="status"/);
    // Never "dieses Projekt gibt es nicht": that would be an answer about a project this
    // reader was not shown, which is the whole of what the filtering was hiding.
    expect(out).not.toMatch(/Projekt gibt es nicht/);
  });
});

describe('moving a card from here', () => {
  it('offers a real form per card to somebody signed in', () => {
    const out = html();
    expect(out).toMatch(/action="\/aufgaben\?\/verschieben"/);
    expect(out.match(/type="submit"/g)?.length).toBeGreaterThanOrEqual(2);
  });

  it('shows a card to a reader who cannot move it, marked, rather than hiding it', () => {
    const out = html({ me: ANONYMOUS });
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('Nur lesbar');
    expect(out).not.toMatch(/action="\/aufgaben\?\/verschieben"/);
  });

  it('carries the filter into the way back, so a move does not widen the board', () => {
    const out = html({ projekt: projects[0], zurueck: '/aufgaben?projekt=p1' });
    expect(out).toMatch(
      /name="zurueck"[^>]*value="\/aufgaben\?projekt=p1"|value="\/aufgaben\?projekt=p1"[^>]*name="zurueck"/
    );
  });
});
