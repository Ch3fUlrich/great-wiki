import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import BlockView from './BlockView.svelte';
import type { Block, MarkKind } from '$lib/blocks/render';
import type { Attachment } from '$lib/attachments';

/// The component's markup, without the hydration markers Svelte interleaves — they are an
/// implementation detail and would make every assertion about structure unreadable.
function html(block: Block, anhaenge: Attachment[] = []): string {
  return render(BlockView, { props: { block, anhaenge } }).body.replace(/<!--.*?-->/g, '');
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

  // --- Checklists ---------------------------------------------------------------------
  //
  // A kind this renderer does not know renders as NOTHING (the test above says so), which
  // for `taskList` means a checklist simply disappears from the page — no gap, no marker,
  // no way for a reader to tell that the author wrote one. These tests are what keep it
  // visible, and what keep it read-only: per design decision D-2 the record owns a task's
  // state and the page owns its words, so a checkbox in the READING view must never be
  // clickable. Toggling one here would file a revision nobody typed.

  /** A checklist as `gw_core::markdown` imports `- [x] Milch kaufen`. */
  function taskList(items: [boolean, string][]): Block {
    return {
      kind: 'taskList',
      content: items.map(([checked, text]) => ({
        kind: 'taskItem',
        attrs: { checked },
        content: [{ kind: 'paragraph', content: [{ kind: 'text', text }] }]
      }))
    };
  }

  it('renders a checklist as a list, not as nothing at all', () => {
    const out = html(taskList([[false, 'Milch kaufen']]));
    expect(out).toContain('<ul');
    expect(out).toContain('<li');
    expect(out).toContain('Milch kaufen');
  });

  it('gives every line a real checkbox that reflects `checked`', () => {
    const out = html(
      taskList([
        [true, 'erledigt'],
        [false, 'offen']
      ])
    );
    const inputs = out.match(/<input[^>]*>/g) ?? [];
    expect(inputs).toHaveLength(2);
    // The state is on the control itself, not only in a class a screen reader cannot see:
    // a native checkbox announces "checked"/"not checked" without any ARIA of its own.
    expect(inputs[0]).toMatch(/type="checkbox"/);
    expect(inputs[0]).toMatch(/\bchecked\b/);
    expect(inputs[1]).not.toMatch(/\bchecked\b/);
  });

  it('names each checkbox, so it is not an anonymous control in a list of them', () => {
    const out = html(taskList([[true, 'Milch kaufen']]));
    expect(out).toMatch(/<input[^>]*aria-label="Milch kaufen"/);
  });

  it('never lets a reader tick a box, because the page is not where that state lives', () => {
    // D-2: dragging a card or ticking a box changes the RECORD. A checkbox wired up here
    // would need write permission on the page for a click and would file a revision
    // nobody typed. Real interactivity waits for the board API.
    const out = html(taskList([[false, 'offen']]));
    expect(out).toMatch(/<input[^>]*disabled/);
    expect(out).not.toMatch(/onclick|onchange/i);
  });

  it('keeps a nested checklist nested', () => {
    const nested: Block = {
      kind: 'taskList',
      content: [
        {
          kind: 'taskItem',
          attrs: { checked: false },
          content: [
            { kind: 'paragraph', content: [{ kind: 'text', text: 'Einkauf' }] },
            taskList([[true, 'Milch']])
          ]
        }
      ]
    };
    const out = html(nested);
    expect(out.match(/<ul/g)).toHaveLength(2);
    expect(out.match(/<input/g)).toHaveLength(2);
    // The outer item's name is its own line, not its line plus everything under it.
    expect(out).toMatch(/aria-label="Einkauf"/);
    expect(out).toMatch(/aria-label="Milch"/);
  });

  it('treats a missing `checked` as unticked rather than as ticked', () => {
    // `gw_core` always writes `checked`, but the database is the source of truth and a
    // tree can reach here from anywhere. Nothing may invent a completed task.
    const out = html({
      kind: 'taskList',
      content: [
        { kind: 'taskItem', content: [{ kind: 'paragraph', content: [{ kind: 'text', text: 'x' }] }] }
      ]
    });
    const inputs = out.match(/<input[^>]*>/g) ?? [];
    expect(inputs).toHaveLength(1);
    expect(inputs[0]).not.toMatch(/\bchecked\b/);
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

// --- files placed in the prose (D-15) ----------------------------------------------------

describe('a placed file', () => {
  const row = (filename: string, media_type: string, byte_size = 1024): Attachment => ({
    filename,
    media_type,
    byte_size,
    uploaded_at: '2026-09-01 10:00:00',
    uploaded_by_name: 'Anna',
    // The API's own address: it names the PAGE and does not name the bytes, which is the
    // whole of D-16. This component prints it and never assembles one.
    href: `/api/attachment/${filename}/rundgang`
  });

  const placement = (filename: string, alt = ''): Block => ({
    kind: 'attachment',
    attrs: { filename, alt }
  });

  it('shows a picture where it was placed, at the address the API sent', () => {
    const out = html(placement('befund.png', 'Röntgenbild, seitlich'), [
      row('befund.png', 'image/png')
    ]);
    expect(out).toContain('<img');
    expect(out).toContain('src="/api/attachment/befund.png/rundgang"');
    expect(out).toContain('alt="Röntgenbild, seitlich"');
  });

  it('never puts a content address on the page', () => {
    // A placement carries a name, and the row carries an `href` that names the page. If a
    // digest ever reached either, a reader could go looking for the same bytes under a page
    // they may not read — which is the thing D-16 exists to make impossible rather than
    // merely unlikely.
    const out = html(placement('befund.png', 'x'), [row('befund.png', 'image/png')]);
    expect(out).not.toMatch(/[0-9a-f]{40}/i);
    expect(out).not.toMatch(/sha256/i);
  });

  it('falls back to the filename when nobody described the picture', () => {
    // Never `alt=""`. A placement is content somebody put in the middle of their prose, so
    // it is never decorative, and an empty alt makes it invisible to a screen reader
    // entirely. The filename is a poor description and it is the same string the `Anhänge`
    // list names the file by, so a reader who cannot see it can still find and fetch it.
    const out = html(placement('befund.png'), [row('befund.png', 'image/png')]);
    expect(out).toContain('alt="befund.png"');
    expect(out).not.toContain('alt=""');
  });

  it('renders an SVG through <img> and through nothing that could execute it', () => {
    // ADR 0014, and the one thing in this feature that has to be exactly right. An SVG is
    // XML that can carry `<script>`, event handlers and external references, and it is
    // stored exactly as uploaded because nothing sanitises it. `<img src>` is a context no
    // browser executes it in; `<object>`, `<embed>` and `<iframe>` all run it, and putting
    // its markup into this wiki's own DOM would run it IN THIS ORIGIN with the session
    // cookie in reach.
    //
    // Asserted on an SVG specifically rather than trusted to the image branch written for
    // PNGs, for the reason `content_disposition` names SVG in its own arm on the server:
    // a defence that depends on somebody remembering that SVG is an image does not survive
    // the next type being added.
    const out = html(placement('diagramm.svg', 'Ein Diagramm'), [
      row('diagramm.svg', 'image/svg+xml')
    ]);
    expect(out).toContain('<img');
    expect(out).toContain('src="/api/attachment/diagramm.svg/rundgang"');
    for (const forbidden of ['<object', '<embed', '<iframe', '<svg', '<script', 'srcdoc']) {
      expect(out.toLowerCase(), forbidden).not.toContain(forbidden);
    }
  });

  it('offers everything that is not a picture as a card that downloads', () => {
    const out = html(placement('laborwerte.csv', 'Laborwerte 2026'), [
      row('laborwerte.csv', 'text/plain; charset=utf-8', 2048)
    ]);
    // A LINK, so it works before hydration, opens in a new tab and saves with a right-click
    // — and the server sends `Content-Disposition: attachment` for it, so following it saves
    // the file rather than replacing the page.
    expect(out).toContain('<a');
    expect(out).toContain('href="/api/attachment/laborwerte.csv/rundgang"');
    expect(out).not.toContain('<img');
    // What somebody needs before deciding to fetch it, in words rather than as an icon.
    expect(out).toContain('Laborwerte 2026');
    expect(out).toContain('laborwerte.csv');
    expect(out).toContain('text/plain');
    expect(out).toContain('2,0 kB');
  });

  it('shows a PDF as a card rather than in the middle of the prose', () => {
    // The owner's decision: pictures are shown, everything else is offered. A PDF is served
    // inline by the download route and is still not a picture here.
    const out = html(placement('befund.pdf', 'Der Befund'), [
      row('befund.pdf', 'application/pdf')
    ]);
    expect(out).not.toContain('<img');
    expect(out).toContain('PDF');
    expect(out).toContain('href="/api/attachment/befund.pdf/rundgang"');
  });

  it('says a file is not attached rather than drawing a broken picture', () => {
    // Two ordinary things produce this and neither is a fault: somebody detached the file,
    // which deliberately leaves the prose alone (D-15), or the page was imported from
    // markdown naming a file nobody has uploaded. A missing `<img>` would render as an icon
    // and read as "the network failed", which is neither true nor actionable.
    const out = html(placement('gibtsnicht.png', 'Fehlt'), [row('befund.png', 'image/png')]);
    expect(out).not.toContain('<img');
    expect(out).not.toContain('<a');
    expect(out).toContain('gibtsnicht.png');
    expect(out).toMatch(/entfernt|hochgeladen/);
  });

  it('says the same thing when it was given no list at all', () => {
    // The honest reading of an empty list: this page carries no such file. It must never
    // fall back to guessing an address from the name — that would be this interface
    // assembling a download address, which is exactly what D-16 forbids it.
    const out = html(placement('befund.png', 'Röntgenbild'));
    expect(out).not.toContain('<img');
    expect(out).not.toContain('/api/attachment');
    expect(out).toContain('befund.png');
  });

  it('draws nothing for a placement that names no file', () => {
    // A malformed block. Drawing the "not attached" sentence about a file called "" would
    // be the interface reporting a fault in a page as a fault in its attachments.
    const out = html({ kind: 'attachment', attrs: {} }, [row('befund.png', 'image/png')]);
    expect(out.trim()).toBe('');
  });

  it('resolves a placement anywhere in the tree, not only at the root', () => {
    // The list is threaded through every level of the recursion rather than only the top
    // one. Nothing the importer or the editor can produce puts a placement below the root —
    // both refuse it — but a component that silently rendered nothing wherever the list did
    // not reach would hide exactly the case somebody hand-edited a body into.
    const out = html(
      { kind: 'doc', content: [{ kind: 'doc', content: [placement('befund.png', 'tief')] }] },
      [row('befund.png', 'image/png')]
    );
    expect(out).toContain('src="/api/attachment/befund.png/rundgang"');
  });
});

// --- code blocks -------------------------------------------------------------------------
//
// Whitespace IS the content here, and it used to be thrown away: the code branch rendered
// `plainText(block)`, whose last act is `.replace(/\s+/g, ' ').trim()`, so every fenced
// block on the site arrived as one line with its indentation gone. Nothing caught it
// because until now no test in this suite rendered a `codeBlock` at all.
//
// `plainText` is NOT the place to fix that — it is a byte-for-byte mirror of
// `gw_core::Block::plain_text` that feeds heading anchor ids, the outline and a table's
// column labels — so the code branch reads the block's text leaves itself, through
// `codeText`.

describe('a code block', () => {
  /** A fence as `gw_core::markdown` imports one: the info string's first word, and the text. */
  const fence = (text: string, language?: string): Block => ({
    kind: 'codeBlock',
    attrs: language ? { language } : undefined,
    content: [{ kind: 'text', text }]
  });

  it('keeps every newline and every space the author typed', () => {
    const source = 'fn main() {\n    println!("hallo");\n}';
    const out = html(fence(source, 'rust'));
    expect(out).toContain('<pre><code>');
    // Exact, not `contains('println')`: the bug was one line with the indentation gone, and
    // that passes any assertion that only asks whether the words are present.
    expect(out).toContain('fn main() {\n    println!("hallo");\n}');
  });

  it('keeps the newlines a diagram is delimited by, which are the whole of its syntax', () => {
    // `graph TD; A-->B;` on one line is not the same source as two lines, and a renderer
    // handed the collapsed form draws nothing. This is why the fix is a step zero rather
    // than part of the diagram work.
    const out = html(fence('graph TD;\n  A-->B;', 'mermaid'));
    expect(out).toContain('graph TD;\n  A-->B;');
  });

  it('escapes what it prints rather than putting it into the page as markup', () => {
    // The reader constructs no HTML from stored content, and a fence is the one place where
    // somebody would obviously try. Svelte escapes the interpolation; this pins it, because
    // the branch now reads the text leaves itself instead of going through a helper.
    const out = html(fence('<script>alert(1)</script>', 'html'));
    expect(out).not.toContain('<script');
    expect(out).toContain('&lt;script');
  });

  it('renders an empty fence as an empty block rather than as nothing', () => {
    const out = html({ kind: 'codeBlock', attrs: { language: 'text' } });
    expect(out).toContain('<pre><code></code></pre>');
  });
});
