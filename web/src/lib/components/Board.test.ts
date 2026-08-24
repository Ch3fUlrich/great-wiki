import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Board from './Board.svelte';
import { ANONYMOUS, type Me } from '$lib/api';
import { BOARD_NOTICE_ID, type BoardNotice, type BoardResponse, type BoardTask } from '$lib/board';
import type { Project } from '$lib/projects';

/**
 * The board itself — **the one component both of D-12's placements render**.
 *
 * D-12 put a board at `/aufgaben` and another on every project's home page, and named the
 * cost in the same breath: two things that must agree. They agree here by being the same
 * file. So this test is where the card states are proved, once, rather than twice on two
 * pages that could drift: a card you may see but not move, a detached card, an overdue one.
 *
 * There is no DOM environment in this project, so `render()` from `svelte/server` is the
 * first response exactly as a reader receives it — which is also the point being made: every
 * state below is in that first response, before any script runs, including the control that
 * moves a card.
 *
 * **What is NOT tested here is who may see a card.** `GET /api/board` answers only the cards
 * whose governing page the caller may read, per document, and that belongs to
 * `Store::board_for` where it is mutation-tested. What is tested here is the half that is
 * this file's business: that it renders the cards it was handed, invents none, drops none,
 * and adds no number about the ones it never saw.
 */
const NOW = Date.UTC(2026, 7, 24, 12, 0, 0); // 2026-08-24 12:00 UTC

const projekt: Project = {
  id: 'p1',
  home_path: '/rundgang/tabellen',
  home_title: 'Tabellen',
  tag_id: null,
  created_at: '2026-08-20 09:00:00'
};

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

/** A board carrying exactly the cards given, sorted into the column each one names. */
function board(tasks: BoardTask[], project: Project | null = null): BoardResponse {
  return {
    project,
    columns: [
      { status: 'Offen', tasks: tasks.filter((entry) => entry.status === 'Offen') },
      { status: 'Läuft', tasks: tasks.filter((entry) => entry.status === 'Läuft') },
      { status: 'Fertig', tasks: tasks.filter((entry) => entry.status === 'Fertig') }
    ]
  };
}

const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  source: 'session'
};

interface Options {
  tasks?: BoardTask[];
  project?: Project | null;
  me?: Me | null;
  zurueck?: string;
  titel?: string;
  ebene?: 2 | 3;
  hinweis?: BoardNotice | null;
  fehler?: string | null;
}

function html(options: Options = {}): string {
  return render(Board, {
    props: {
      board: board(options.tasks ?? [task()], options.project ?? null),
      me: options.me === undefined ? signedIn : options.me,
      now: NOW,
      zurueck: options.zurueck ?? '/aufgaben',
      titel: options.titel ?? 'Aufgaben',
      ebene: options.ebene ?? 2,
      hinweis: options.hinweis ?? null,
      fehler: options.fehler ?? null
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the three columns', () => {
  it('are always all three, named as D-9 names them', () => {
    const out = html({ tasks: [] });
    expect(out).toContain('>Offen<');
    expect(out).toContain('>Läuft<');
    expect(out).toContain('>Fertig<');
    // In board order, left to right.
    expect(out.indexOf('>Offen<')).toBeLessThan(out.indexOf('>Läuft<'));
    expect(out.indexOf('>Läuft<')).toBeLessThan(out.indexOf('>Fertig<'));
  });

  it('puts each card in the column its status names', () => {
    const out = html({
      tasks: [
        task({ id: 'a', title: 'Erstens', status: 'Offen' }),
        task({ id: 'b', title: 'Zweitens', status: 'Fertig' })
      ]
    });
    const offen = out.indexOf('>Offen<');
    const fertig = out.indexOf('>Fertig<');
    expect(out.indexOf('Erstens')).toBeGreaterThan(offen);
    expect(out.indexOf('Erstens')).toBeLessThan(fertig);
    expect(out.indexOf('Zweitens')).toBeGreaterThan(fertig);
  });

  it('renders exactly the cards it was handed and invents none', () => {
    const out = html({ tasks: [task({ id: 'a' }), task({ id: 'b', status: 'Läuft' })] });
    // On the CARDS, not on the move buttons, which carry the same id so a drop can find
    // the button it would have pressed.
    expect(out.match(/<li[^>]*data-karte="/g)).toHaveLength(2);
    expect(out).toMatch(/<li[^>]*data-karte="a"/);
    expect(out).toMatch(/<li[^>]*data-karte="b"/);
  });

  it('renders no count of cards and no hint that anything was left out', () => {
    // A board is an aggregate view, so a number here would be a number about pages the
    // reader may not read — the one thing the filtering cannot take back. The API pins the
    // same absence on its own keys; this is the interface's half of it.
    const out = html({ tasks: [task({ id: 'a' }), task({ id: 'b' })] });
    expect(out).not.toMatch(/\d+\s*(Aufgaben|Karten|Einträge|Ergebnisse)/);
    for (const leak of ['verborgen', 'ausgeblendet', 'nicht sichtbar', 'weitere Aufgaben']) {
      expect(out).not.toContain(leak);
    }
  });

  it('says nothing is here without claiming that no task exists', () => {
    const out = html({ tasks: [] });
    expect(out).toContain('Hier ist keine Aufgabe zu sehen');
    expect(out).not.toMatch(/gibt (es )?(noch )?keine Aufgaben/);
  });

  it('states a failed request instead of rendering an empty board', () => {
    const out = html({ tasks: [], fehler: 'Die Aufgaben konnten nicht geladen werden (Fehler 500).' });
    expect(out).toContain('Fehler 500');
    expect(out).toMatch(/role="alert"/);
    expect(out).not.toContain('Hier ist keine Aufgabe zu sehen');
  });
});

describe('a card', () => {
  it('names its page and links to it', () => {
    const out = html({ tasks: [task()] });
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('href="/rundgang/tabellen"');
    expect(out).toContain('Tabellen');
  });

  it('says a standalone card belongs to no page rather than leaving a hole', () => {
    // A card made on a board names no page, and that is not an omission — naming the
    // project's home page would claim a line exists somewhere that never held one.
    const out = html({ tasks: [task({ anchored: false, page: null })] });
    expect(out).toMatch(/keine Seite|auf der Tafel angelegt/i);
    expect(out).not.toContain('undefined');
    expect(out).not.toContain('null<');
  });

  it('names the person it rests on when there is one', () => {
    const out = html({ tasks: [task({ assignee: 'pr-7', assignee_name: 'Petra Reuter' })] });
    expect(out).toContain('Zuständig');
    expect(out).toContain('Petra Reuter');
  });

  it('falls back to the id when the viewer may not learn who that is', () => {
    // `assignee_name` being null is NOT an error and NOT a missing account: it means this
    // viewer may not be told who the person is — they may no longer read the page the card
    // is governed by, or the account is suspended. The id is what the card carried before
    // any name existed, and it stays, because a card that forgot its assignee would leave
    // nothing for anybody to clear.
    const out = html({ tasks: [task({ assignee: 'pr-7', assignee_name: null })] });
    expect(out).toContain('Zuständig');
    expect(out).toContain('pr-7');
    // No invented stand-in. "Unbekannt" would be a claim about the account rather than
    // about what this viewer may be told.
    expect(out).not.toContain('Unbekannt');
  });

  it('says nothing at all about an assignee when the card rests on nobody', () => {
    const out = html({ tasks: [task({ assignee: null, assignee_name: null })] });
    expect(out).not.toContain('Zuständig');
  });
});

describe('a due date', () => {
  it('is shown, and an overdue one says so in words as well as in colour', () => {
    // The requirement, and the reason it is asserted on the rendered card: this codebase
    // holds the line everywhere — the diff views mark every change with a word as well as a
    // background. A red border alone is nothing at all to a reader who cannot see it.
    const out = html({ tasks: [task({ due_at: '2026-08-20' })] });
    expect(out).toContain('Überfällig');
    expect(out).toContain('20.08.2026');
    // The colour hangs off this attribute, so the word above is what carries the meaning
    // and the colour is the redundant channel rather than the only one.
    expect(out).toMatch(/data-faellig="überfällig"/);
  });

  it('marks a card due today apart from one due later, in words', () => {
    expect(html({ tasks: [task({ due_at: '2026-08-24' })] })).toContain('Fällig heute');
    expect(html({ tasks: [task({ due_at: '2026-09-30' })] })).toContain('Fällig 30.09.2026');
  });

  it('shows no due date furniture on a card that has none', () => {
    expect(html({ tasks: [task({ due_at: null })] })).not.toContain('Fällig');
  });
});

describe('a card you may see but not move', () => {
  it('is SHOWN, marked read-only in words, and offers no move', () => {
    // The owner's decision, and the right way round: the checkbox is already visible on the
    // page the line was written on, so hiding the card hides nothing — and a task that
    // silently vanishes from a board is the failure this whole design keeps trying to avoid.
    const out = html({ me: ANONYMOUS, tasks: [task({ title: 'Kabel bestellen' })] });
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('Nur lesbar');
    expect(out).toContain('anmelden');
    expect(out).not.toContain('<button');
  });

  it('marks one card read-only without withdrawing the controls from the others', () => {
    // Per card, not per board: the day the endpoint answers "may I write this", one card
    // being read-only must not take the board's controls away from the rest.
    const out = html({
      tasks: [task({ id: 'a', title: 'Gesperrt', movable: false }), task({ id: 'b', title: 'Frei' })]
    });
    expect(out).toContain('Gesperrt');
    expect(out).toContain('Nur lesbar');
    expect(out).toContain('Schreibrecht');
    expect(out).toMatch(/name="karte"[^>]*value="b"|value="b"[^>]*name="karte"/);
    expect(out).not.toMatch(/name="karte"[^>]*value="a"|value="a"[^>]*name="karte"/);
  });
});

describe('a detached card', () => {
  it('stays on the board and says so in words', () => {
    // D-8: its page no longer mentions the line, but its due date and its assignee are
    // still somebody's. Deleting it would discard those silently, so it is kept and marked.
    const out = html({
      tasks: [task({ detached: true, due_at: '2026-09-01', assignee: 'pr-7' })]
    });
    expect(out).toContain('Kabel bestellen');
    expect(out).toContain('Abgelöst');
    expect(out).toContain('Tabellen');
    expect(out).toMatch(/Fälligkeit|Zuständigkeit/);
    // Still a card, with everything a card has: the state it holds is still real.
    expect(out).toContain('01.09.2026');
    expect(out).toContain('pr-7');
  });

  it('can still be moved — detached is not read-only', () => {
    const out = html({ tasks: [task({ id: 'a', detached: true })] });
    expect(out).toMatch(/name="karte"[^>]*value="a"|value="a"[^>]*name="karte"/);
  });
});

describe('moving a card without a pointer', () => {
  it('is a real form submission, so it works before hydration', () => {
    const out = html({ tasks: [task({ id: 'a' })] });
    expect(out).toMatch(/<form[^>]*method="post"/i);
    expect(out).toMatch(/action="\/aufgaben\?\/verschieben"/);
    expect(out).not.toContain('onclick=');
    expect(out).not.toContain('onsubmit=');
  });

  it('offers a labelled button for each of the other two columns', () => {
    const out = html({ tasks: [task({ id: 'a', title: 'Kabel bestellen', status: 'Offen' })] });
    // Two, not three: a card is already in its own column.
    expect(out.match(/type="submit"/g)).toHaveLength(2);
    expect(out).toMatch(/name="status"[^>]*value="Läuft"/);
    expect(out).toMatch(/name="status"[^>]*value="Fertig"/);
    expect(out).not.toMatch(/name="status"[^>]*value="Offen"/);
    // Named per card, not "Verschieben" four times down a column: a control read on its own
    // has to say which card it moves and where to.
    expect(out).toContain('aria-label="»Kabel bestellen« nach Läuft verschieben"');
  });

  it('carries the card and the way back, so both placements return where they were', () => {
    const out = html({ tasks: [task({ id: 'a' })], zurueck: '/rundgang/tabellen' });
    expect(out).toMatch(/name="karte"[^>]*value="a"|value="a"[^>]*name="karte"/);
    expect(out).toMatch(
      /name="zurueck"[^>]*value="\/rundgang\/tabellen"|value="\/rundgang\/tabellen"[^>]*name="zurueck"/
    );
  });

  it('lets a card be dragged as well, without that being the only way', () => {
    // Dragging is an addition on top of the buttons and uses the same submission — a
    // drag-only board is unusable without a pointer, and this codebase ships no such control.
    const out = html({ tasks: [task({ id: 'a' })] });
    expect(out).toContain('draggable="true"');
    expect(out.match(/type="submit"/g)).toHaveLength(2);
  });

  it('makes a read-only card undraggable too', () => {
    expect(html({ me: ANONYMOUS })).not.toContain('draggable="true"');
  });
});

describe('what the board says about what just happened', () => {
  it('announces a move in a region the redirect can put the reader inside', () => {
    // A live region that is already there when the page loads announces nothing — a live
    // region announces what CHANGES. The redirect after a move carries this fragment and
    // the element takes focus, so the sentence is read out because it is where the reader
    // now is. That needs no JavaScript, which is the requirement.
    const out = html({
      hinweis: { art: 'ok', text: '»Kabel bestellen« steht jetzt in Läuft.' }
    });
    expect(out).toContain(`id="${BOARD_NOTICE_ID}"`);
    expect(out).toMatch(/role="status"/);
    expect(out).toMatch(/tabindex="-1"/);
    expect(out).toContain('steht jetzt in Läuft');
  });

  it('says a refused move refused, and interrupts for it', () => {
    const out = html({
      hinweis: {
        art: 'fehler',
        text: 'Dafür fehlt das Schreibrecht auf der Seite, zu der die Karte gehört. Die Karte wurde nicht verschoben.'
      }
    });
    expect(out).toMatch(/role="alert"/);
    expect(out).toContain('nicht verschoben');
    expect(out).toContain(`id="${BOARD_NOTICE_ID}"`);
  });
});

describe('one component, two placements', () => {
  it('takes its heading and its level from whoever placed it', () => {
    // `/aufgaben` puts the board under the page's own h1; a project's home page puts it
    // under the page title, one level further down. Same markup, same rules, one file.
    expect(html({ titel: 'Aufgaben', ebene: 2 })).toMatch(/<h2[^>]*>Aufgaben<\/h2>/);
    const embedded = html({ titel: 'Aufgaben dieser Seite', ebene: 3 });
    expect(embedded).toMatch(/<h3[^>]*>Aufgaben dieser Seite<\/h3>/);
    // The column headings move with it, so the outline never skips a level.
    expect(embedded).toMatch(/<h4[^>]*>Offen<\/h4>/);
  });

  it('names the project when the board is bound to one, and links to its home page', () => {
    const out = html({ project: projekt, ebene: 3, tasks: [] });
    expect(out).toContain('Tabellen');
    expect(out).toContain('href="/rundgang/tabellen"');
  });

  it('is a named region either way, so it is reachable as a landmark', () => {
    expect(html()).toMatch(/aria-labelledby="/);
  });
});
