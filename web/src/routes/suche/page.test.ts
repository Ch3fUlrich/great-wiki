import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';
import { NO_RESULTS, type SearchResults } from '$lib/search';

function html(query: string, results: SearchResults = NO_RESULTS): string {
  return render(Page, { props: { data: { query, results } } }).body.replace(/<!--.*?-->/g, '');
}

describe('the results page', () => {
  it('asks for a term when there is none, and shows no groups', () => {
    const out = html('');
    expect(out).toContain('Suchbegriff');
    expect(out).not.toContain('Seiten</h2>');
  });

  it('shows all three groups with their own empty state', () => {
    const out = html('x');
    for (const h of ['Seiten', 'Themen', 'Aufgaben']) expect(out).toMatch(new RegExp(`<h2[^>]*>${h}</h2>`));
    expect(out).toContain('Keine Seiten gefunden.');
    expect(out).toContain('Keine Themen gefunden.');
    expect(out).toContain('Keine Aufgaben gefunden.');
    expect(out).toContain('Nichts gefunden.');
  });

  it('marks hits from segments and never turns text into markup', () => {
    const out = html('x', {
      pages: [
        {
          title: 'Seite',
          path: '/a/b',
          snippet: [
            { text: 'vor <img src=x onerror=alert(1)> ', hit: false },
            { text: 'Treffer', hit: true }
          ]
        }
      ],
      topics: [{ name: 'T', display_path: 'A/T', path: '/a/t', documents: 2 }],
      tasks: [
        { id: '1', title: 'Auf Seite', status: 'Offen', page_path: '/a/b' },
        { id: '2', title: 'Ohne Seite', status: 'Fertig', page_path: null }
      ]
    });
    expect(out).toMatch(/<mark[^>]*>Treffer<\/mark>/);
    expect(out).toContain('&lt;img src=x onerror=alert(1)');
    expect(out).not.toContain('<img');
    expect(out).toContain('href="/a/b"');
    expect(out).toContain('href="/themen/a/t"');
    expect(out).toContain('href="/aufgaben"');
    expect(out).not.toContain('Nichts gefunden.');
  });
});
