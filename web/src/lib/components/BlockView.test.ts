import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import BlockView from './BlockView.svelte';
import type { Block, MarkKind } from '$lib/blocks/render';

/// The component's markup, without the hydration markers Svelte interleaves — they are an
/// implementation detail and would make every assertion about structure unreadable.
function html(block: Block): string {
  return render(BlockView, { props: { block } }).body.replace(/<!--.*?-->/g, '');
}

/// A single formatted leaf, standalone: `BlockView` accepts any block kind at its root,
/// `'text'` included, so this needs no paragraph wrapper to reach the marks-rendering path.
function textWithMark(text: string, kind: MarkKind, attrs?: Record<string, unknown>): Block {
  return { kind: 'text', text, marks: [{ kind, attrs }] };
}

/// A link mark carrying `href` — an external link, per `gw_core::Mark::link_to_url`.
function linkTo(href: string): Block {
  return textWithMark('Link', 'link', { href });
}

/// A cell as the converter produces one: block content, so the text sits in a paragraph.
function cell(kind: 'tableHeader' | 'tableCell', text: string, align?: string): Block {
  return {
    kind,
    attrs: align ? { align } : undefined,
    content: [{ kind: 'paragraph', content: [{ kind: 'text', text }] }]
  };
}

const table: Block = {
  kind: 'table',
  content: [
    {
      kind: 'tableRow',
      content: [cell('tableHeader', 'Feld'), cell('tableHeader', 'Wert', 'right')]
    },
    { kind: 'tableRow', content: [cell('tableCell', 'Länge'), cell('tableCell', '42', 'right')] },
    { kind: 'tableRow', content: [cell('tableCell', 'Breite'), cell('tableCell', '7', 'right')] }
  ]
};

describe('BlockView', () => {
  it('renders a table as a table, not as paragraphs', () => {
    const out = html(table);
    expect(out).toContain('<table>');
    expect(out).toContain('<thead>');
    expect(out).toContain('<tbody>');
    expect(out.match(/<tr>/g)).toHaveLength(3);
    expect(out.match(/<th\b/g)).toHaveLength(2);
    expect(out.match(/<td\b/g)).toHaveLength(4);
    for (const text of ['Feld', 'Wert', 'Länge', '42', 'Breite', '7']) {
      expect(out).toContain(text);
    }
  });

  it('gives every header cell a scope, or the table is unlabelled cells to a screen reader', () => {
    const out = html(table);
    expect(out.match(/<th[^>]*scope="col"/g)).toHaveLength(2);
  });

  it('applies column alignment, and adds no style where the column states none', () => {
    const out = html(table);
    // The right-aligned column: its header and both of its body cells.
    expect(out.match(/text-align:\s*right/g)).toHaveLength(3);
    // The first column states no alignment, so it must not be styled into one.
    expect(out).not.toMatch(/<th(?![^>]*scope="col"[^>]*style)[^>]*style="[^"]*text-align:\s*left/);
    expect(out.match(/text-align/g)).toHaveLength(3);
  });

  it('puts the table in a scroll region a keyboard user can reach and a screen reader can name', () => {
    // A wide table must scroll inside its own box; the page body scrolling sideways is
    // what makes a document unreadable on a phone. A scrollable box is only reachable
    // without a mouse if it is focusable, and only announced if it is named.
    const out = html(table);
    // Located by the role it claims, not by being the first `<div>` in the output — the
    // table now sits inside an outer wrapper that carries the controls, and "first div"
    // silently started pointing at that instead.
    const wrapper = out.match(/<div[^>]*role="region"[^>]*>/)?.[0] ?? '';
    expect(wrapper).toContain('tabindex="0"');
    expect(wrapper).toMatch(/aria-label="[^"]+"/);
  });

  // --- Progressive enhancement -------------------------------------------------------
  //
  // Sorting and filtering are added by TableView AFTER the component mounts. Everything
  // below is about the other half of that bargain: what a reader with no JavaScript gets,
  // which is the complete table and no control that does nothing.

  /// A table with `rows` body rows — above the control threshold when asked for.
  function bigTable(rows: number): Block {
    return {
      kind: 'table',
      content: [
        {
          kind: 'tableRow',
          content: [cell('tableHeader', 'Stamm'), cell('tableHeader', 'Dosis', 'right')]
        },
        ...Array.from({ length: rows }, (_, i) => ({
          kind: 'tableRow' as const,
          content: [cell('tableCell', `Stamm ${i}`), cell('tableCell', `${i} mg`, 'right')]
        }))
      ]
    };
  }

  it('renders every row of a long table on the server, in document order', () => {
    const out = html(bigTable(26));
    expect(out.match(/<tr>/g)).toHaveLength(27);
    for (let i = 0; i < 26; i += 1) expect(out).toContain(`Stamm ${i}`);
    // Document order, not the order some default sort would produce.
    expect(out.indexOf('Stamm 0')).toBeLessThan(out.indexOf('Stamm 1'));
    expect(out.indexOf('Stamm 24')).toBeLessThan(out.indexOf('Stamm 25'));
  });

  it('offers no control at all before it can work', () => {
    // A filter box that does nothing is worse than no filter box: it invites a reader to
    // type into it and then silently shows them everything.
    const out = html(bigTable(26));
    expect(out).not.toContain('<button');
    expect(out).not.toContain('<input');
    expect(out).not.toContain('aria-sort');
    expect(out).not.toContain('von 26 Zeilen');
  });

  it('keeps column alignment in the long table too', () => {
    const out = html(bigTable(26));
    // The right-aligned column: its header and all 26 of its body cells.
    expect(out.match(/text-align:\s*right/g)).toHaveLength(27);
  });

  it('leaves a short table exactly as it was', () => {
    // Three rows do not need a toolbar, and one in front of them is noise where the whole
    // table is already in view.
    const out = html(bigTable(3));
    expect(out.match(/<tr>/g)).toHaveLength(4);
    expect(out).not.toContain('<button');
    expect(out).not.toContain('<input');
  });

  it('still renders the blocks it always did', () => {
    expect(html({ kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] })).toContain(
      '<p>Ein Satz.</p>'
    );
    expect(
      html({ kind: 'heading', attrs: { level: 2 }, content: [{ kind: 'text', text: 'Größe' }] })
    ).toMatch(/<h2 id="groesse">/);
  });

  it('skips a kind it does not know rather than rendering it raw', () => {
    const unknown = { kind: 'mystery', text: '<script>' } as unknown as Block;
    expect(html(unknown).trim()).toBe('');
  });

  // --- Marks --------------------------------------------------------------------------
  //
  // The other half of Task 5: the editor grew a toolbar for these because gw-collab can now
  // carry them all the way to a revision, and a reader who never sees the formatting is the
  // failure this section exists to catch — the editor would show bold, the page would not.

  it('renders a bold run as <strong>', () => {
    expect(html(textWithMark('fett', 'strong'))).toContain('<strong>fett</strong>');
  });

  it('renders each mark kind as the tag TipTap itself parses back out of pasted HTML', () => {
    // Not an arbitrary choice: `<em>`, `<code>` and `<s>` are literally the tags
    // `@tiptap/extension-italic`, `-code` and `-strike`'s own `parseHTML`/`renderHTML` use
    // (verified against the installed source), so editing and reading agree on more than
    // just which words are marked.
    expect(html(textWithMark('kursiv', 'em'))).toContain('<em>kursiv</em>');
    expect(html(textWithMark('code', 'code'))).toContain('<code>code</code>');
    expect(html(textWithMark('durch', 'strike'))).toContain('<s>durch</s>');
  });

  it('nests multiple marks on one leaf in the order the server sorted them, outermost first', () => {
    // `gw_core::MARK_ORDER` sorts a leaf's `marks` outermost-first before it ever reaches the
    // wire; this only has to trust that order, not re-derive it — reproducing the ordering
    // here would be the "second ordering" the server-side docs warn against.
    const both: Block = { kind: 'text', text: 'beides', marks: [{ kind: 'strong' }, { kind: 'em' }] };
    expect(html(both)).toContain('<strong><em>beides</em></strong>');
  });

  it('never renders an external link without rel protection', () => {
    expect(html(linkTo('https://example.org'))).toMatch(/rel="[^"]*noopener/);
  });

  it('renders an href link as an anchor with both noopener and noreferrer', () => {
    // The exact pin, not just the substring match above: `noreferrer` matters just as much as
    // `noopener` (it is what keeps the referrer header from naming this wiki to the target
    // site) and a looser regex would not notice it going missing.
    expect(html(linkTo('https://example.org'))).toContain(
      '<a href="https://example.org" rel="noopener noreferrer">Link</a>'
    );
  });

  it('renders no anchor at all for a scheme a browser would execute', () => {
    // I2. `Mark::attrs`' `href` is a plain string that reached the database from a page's
    // markdown, from the editor's Link control (which calls ProseMirror's `setMark`
    // directly, so TipTap's own `isAllowedUri` never sees it) or from any later writer, and
    // NOTHING between there and here validated it. `<a href="javascript:…">` is a stored
    // cross-site-scripting payload that runs on click, for every reader of a public wiki,
    // written by anyone who can edit one page. There is no Content-Security-Policy to fall
    // back on (a known gap, recorded in docs/operations/running-in-production.md), so this
    // renderer is the only thing standing there. It is also the RIGHT place: it is the sink,
    // and guarding it covers every producer, including ones not written yet.
    for (const href of [
      'javascript:alert(1)',
      'JaVaScRiPt:alert(1)',
      '  javascript:alert(1)',
      'java\nscript:alert(1)', // the URL parser strips newlines, exactly as a browser does
      'data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==',
      'vbscript:msgbox(1)',
      'file:///etc/passwd'
    ]) {
      const out = html(linkTo(href));
      expect(out, `${href} reached the DOM as a link`).not.toContain('<a ');
      // Refused as a link, not as content: the words are still on the page, the same way an
      // unknown mark kind renders its text and nothing else.
      expect(out).toContain('Link');
    }
  });

  it('still renders the schemes a wiki actually links with', () => {
    // The other direction of the same check. A guard that also swallowed ordinary links
    // would be a worse bug than the one it fixed, and a silent one: the text stays, so a
    // page reads almost right.
    for (const href of [
      'https://example.org/seite',
      'http://192.168.178.159:4000/v1',
      'mailto:jemand@example.org',
      '/rundgang/tabellen', // relative: no scheme to abuse, resolves against this wiki
      '../nachbar',
      '#abschnitt'
    ]) {
      expect(html(linkTo(href)), `${href} lost its anchor`).toContain(`<a href="${href}"`);
    }
  });

  it('renders a doc link as a non-navigating span, since resolving it is Task 7', () => {
    // `gw_core::Mark::link_to_doc` stores the target under `doc`, an internal document id the
    // server has not resolved to a path yet. Emitting a real `<a href>` here would need that
    // resolution; emitting one with no `href` would be a link that does nothing when clicked,
    // which reads as broken rather than as "not implemented yet". A `<span>` is neither: it
    // carries the text and the id for whenever Task 7 wires the resolution in, and it does
    // not invite a click it cannot honour.
    const out = html(textWithMark('Zieltext', 'link', { doc: '019ff0' }));
    expect(out).not.toContain('<a ');
    expect(out).toContain('data-doc="019ff0"');
    expect(out).toContain('Zieltext');
  });

  it('leaves an unmarked leaf exactly as before', () => {
    expect(html({ kind: 'text', text: 'nichts Besonderes' })).toBe('nichts Besonderes');
  });
});
