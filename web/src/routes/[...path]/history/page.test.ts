import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { ANONYMOUS, type Me, type StoredDocument } from '$lib/api';
import type { RevisionDiff, RevisionSource, RevisionSummary, View } from '$lib/history';

/**
 * The history page, rendered exactly as the server renders it.
 *
 * There is no DOM environment in this project, so `render()` from `svelte/server` is the
 * only thing there is — which is why the page keeps its state in the URL rather than in
 * component state: which two revisions are being compared, which tab is open and whether a
 * restore is being confirmed are all decided in the loader, so all of it arrives in the first
 * response and all of it can be asserted here.
 *
 * What is NOT tested here is who may see any of it. That belongs to the API and is pinned in
 * `crates/gw-api/tests/revisions.rs`; a fixture in the browser could only prove that this
 * file can render a list somebody handed it.
 */
const doc: StoredDocument = {
  id: 'd1',
  path: '/rundgang/tabellen',
  parent_path: '/rundgang',
  slug: 'tabellen',
  doc_type: 'page',
  title: 'Tabellen',
  language: 'de',
  visibility: 'restricted',
  body: '',
  sort_key: 2
};

const NOW = Date.UTC(2026, 7, 20, 12, 0, 0);

const revisions: RevisionSummary[] = [
  {
    id: 'r3',
    parent_id: 'r2',
    summary: 'Spalte ergänzt',
    author_name: 'Sergej',
    author_is_account: true,
    byte_size: 1200,
    created_at: '2026-08-20 09:00:00'
  },
  {
    id: 'r2',
    parent_id: 'r1',
    summary: null,
    author_name: 'Anna',
    author_is_account: true,
    byte_size: 1000,
    created_at: '2026-08-18 09:00:00'
  },
  {
    id: 'r1',
    parent_id: null,
    summary: 'Import',
    author_name: 'Import (kein Konto)',
    author_is_account: false,
    byte_size: 900,
    created_at: '2026-08-01 09:00:00'
  }
];

const diff: RevisionDiff = {
  from: revisions[1],
  to: revisions[0],
  prose: [
    { kind: 'removed', text: 'schnelle' },
    { kind: 'added', text: 'langsame' }
  ],
  structure: [
    {
      kind: 'moved',
      block: 'paragraph',
      text: 'Ein Absatz.',
      from_index: 0,
      to_index: 2
    }
  ],
  design: [
    {
      block: 'heading',
      text: 'Titel',
      attribute: 'level',
      before: '2',
      after: '4'
    }
  ]
};

const source: RevisionSource = {
  revision: revisions[0],
  markdown: '# Tabellen\n\nEin Absatz.\n',
  problem: null,
  meta: 'title: Tabellen\nvisibility: restricted\n',
  design: '{\n  "kind": "doc"\n}'
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
  view?: View;
  from?: RevisionSummary | null;
  to?: RevisionSummary | null;
  diff?: RevisionDiff | null;
  diffError?: string | null;
  source?: RevisionSource | null;
  sourceError?: string | null;
  confirming?: RevisionSummary | null;
  list?: RevisionSummary[];
  error?: string | null;
}

function html(options: Options = {}): string {
  return render(Page, {
    props: {
      data: {
        me: options.me ?? signedIn,
        doc,
        revisions: options.list ?? revisions,
        error: options.error ?? null,
        from: options.from ?? revisions[1],
        to: options.to ?? revisions[0],
        view: options.view ?? 'prosa',
        diff: options.diff === undefined ? diff : options.diff,
        diffError: options.diffError ?? null,
        source: options.source ?? null,
        sourceError: options.sourceError ?? null,
        confirming: options.confirming ?? null,
        now: NOW
      }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the timeline', () => {
  it('lists every revision, newest first, with author, age, summary and size delta', () => {
    const out = html();
    expect(out).toContain('Sergej');
    expect(out).toContain('Anna');
    expect(out).toContain('Spalte ergänzt');
    expect(out).toContain('vor 3 Stunden');
    expect(out).toContain('+200 B');
    // Newest first is the order the API answers in and the order this renders.
    expect(out.indexOf('Sergej')).toBeLessThan(out.indexOf('Anna'));
  });

  it('calls the first revision new rather than reporting the page as growth', () => {
    expect(html()).toContain('neu');
  });

  it('does not render the import as a person', () => {
    // `author_is_account: false` is the import that ran with no account. It must be named
    // honestly and never dressed up as somebody's edit.
    const out = html();
    expect(out).toContain('Import (kein Konto)');
    expect(out).not.toMatch(/von Import \(kein Konto\)/);
  });

  it('lets two revisions be chosen and compared without a script', () => {
    const out = html();
    // A GET form with two radio columns: the selection is in the URL afterwards, so a
    // comparison is a link somebody can send.
    expect(out).toMatch(/<form[^>]*method="get"/i);
    expect(out).toContain('name="von"');
    expect(out).toContain('name="bis"');
    expect(out).toContain('value="r1"');
    expect(out).toContain('Vergleichen');
  });

  it('states an error instead of rendering an empty history for a failed request', () => {
    const out = html({ list: [], error: 'Der Verlauf konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Diese Seite hat noch keine Fassungen');
  });

  it('says a page has no history yet when it genuinely has none', () => {
    const out = html({ list: [], from: null, to: null, diff: null });
    expect(out).toContain('Diese Seite hat noch keine Fassungen');
  });

  it('links back to the page it is the history of', () => {
    expect(html()).toContain('href="/rundgang/tabellen"');
  });
});

describe('the four views', () => {
  it('offers all four and marks the one being shown', () => {
    const out = html({ view: 'struktur' });
    for (const label of ['Prosa', 'Struktur', 'Design', 'Quelltext']) {
      expect(out).toContain(label);
    }
    expect(out).toMatch(/aria-current="page"[^>]*>\s*Struktur|Struktur\s*<\/a>/);
    expect(out).toContain('ansicht=design');
  });

  it('marks additions and removals in text as well as in colour', () => {
    // The requirement, and the reason the marker and the word are both rendered: a diff
    // that distinguishes them by colour alone says nothing to a reader who cannot see it.
    const out = html({ view: 'prosa' });
    expect(out).toContain('Entfernt');
    expect(out).toContain('Hinzugefügt');
    expect(out).toContain('−');
    expect(out).toContain('+');
    expect(out).toContain('schnelle');
    expect(out).toContain('langsame');
  });

  it('reports a move as one row naming both positions', () => {
    const out = html({ view: 'struktur' });
    expect(out).toContain('Verschoben');
    expect(out).toContain('Absatz');
    // Positions are rendered as people count, from one.
    expect(out).toContain('1');
    expect(out).toContain('3');
    expect(out).not.toContain('Hinzugefügt');
  });

  it('names the attribute and both of its values in the design view', () => {
    const out = html({ view: 'design' });
    expect(out).toContain('Überschrift');
    expect(out).toContain('Ebene');
    expect(out).toContain('2');
    expect(out).toContain('4');
  });

  it('says that a view found nothing rather than rendering an empty box', () => {
    const out = html({
      view: 'design',
      diff: { ...diff, design: [] }
    });
    expect(out).toContain('Keine Änderungen');
  });

  it('renders the export triple in the source view, each file named', () => {
    const out = html({ view: 'quelle', source });
    expect(out).toContain('tabellen.md');
    expect(out).toContain('tabellen.meta.yml');
    expect(out).toContain('tabellen.design.json');
    expect(out).toContain('Ein Absatz.');
    expect(out).toContain('title: Tabellen');
    // Either escaping is fine; what matters is that the stored tree is on the page.
    expect(out).toMatch(/(&quot;|")kind(&quot;|"): (&quot;|")doc(&quot;|")/);
  });

  it('says why the markdown is missing rather than showing an empty file', () => {
    const out = html({
      view: 'quelle',
      source: { ...source, markdown: null, problem: 'ein Bild lässt sich nicht schreiben' }
    });
    expect(out).toContain('ein Bild lässt sich nicht schreiben');
    // The stored tree is complete whatever markdown can do, so it is still shown.
    expect(out).toContain('tabellen.design.json');
  });
});

describe('restoring', () => {
  it('asks first, and the question says what will happen to the current version', () => {
    const out = html({ confirming: revisions[2] });
    expect(out).toContain('Wiederherstellen');
    expect(out).toContain('bleibt');
    // Named, not "diese Fassung": the whole value of the step is reading back what it is.
    expect(out).toContain('Import (kein Konto)');
  });

  it('is offered on every revision to somebody signed in', () => {
    const out = html();
    expect(out.match(/wiederherstellen=/g)?.length).toBe(revisions.length);
  });

  it('is not offered to somebody who is not signed in', () => {
    const out = html({ me: ANONYMOUS });
    expect(out).not.toContain('wiederherstellen=');
  });
});
