// Deterministic browser behaviour checks: assert and exit, no screenshots and no human
// in the loop. Mirrors shots.mjs — same base-URL variable, same "point at the local dev
// server" posture, same container invocation (see `just behaviour`).
import { chromium } from 'playwright';

const BASE = process.env.SHOT_BASE ?? 'http://127.0.0.1:5173';

/** Thrown by a check to fail it with a short, specific reason. */
class CheckFailure extends Error {}

function assert(condition, message) {
  if (!condition) throw new CheckFailure(message);
}

/** @type {import('playwright').Browser} */
let browser;

const results = [];

/**
 * Runs one named check in its own browser context (so no check can leak state — focus,
 * cookies, an open dialog — into the next one), records ok/FAIL, and never lets one
 * check's throw stop the run.
 */
async function check(name, fn) {
  const ctx = await browser.newContext();
  try {
    const page = await ctx.newPage();
    await fn(page);
    results.push({ name, ok: true });
    console.log(`ok   ${name}`);
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    results.push({ name, ok: false, reason });
    console.log(`FAIL ${name} — ${reason}`);
  } finally {
    await ctx.close();
  }
}

/**
 * Navigates to the isolated Dialog mount (see routes/_behaviour) and, unless told
 * otherwise, opens it — the shared setup for every Group B check below. Returns the
 * locators each check needs so it doesn't have to re-derive them.
 */
async function loadDialogPage(page, { open } = { open: true }) {
  // networkidle, not domcontentloaded: the trigger's click handler is wired up by
  // Svelte's client-side hydration, which is a dynamic import that resolves after
  // domcontentloaded fires. Clicking too early lands on a button that is visible but
  // not yet interactive — the click does nothing and every check below times out
  // waiting for a dialog that was never asked to open. shots.mjs hits the same
  // requirement for the same reason.
  await page.goto(BASE + '/_behaviour', { waitUntil: 'networkidle' });
  const trigger = page.getByRole('button', { name: 'Dialog öffnen' });
  await trigger.waitFor({ state: 'visible' });
  const dialog = page.locator('.gw-dialog');
  const backdrop = page.locator('.gw-dialog-backdrop');
  if (open) {
    await trigger.click();
    await dialog.waitFor({ state: 'visible', timeout: 5_000 });
    // Focus entering the dialog (what B3 asserts) is driven by a separate effect from
    // the one that flips .gw-dialog visible, and so is closeOnEscape's own listener
    // registration. Pressing Escape (B5-B8) right after the visibility wait sometimes
    // beat that registration and the keypress was silently dropped — confirmed by
    // tracing attribute state over 5s: the dialog just stayed open with no further
    // transition. Waiting for focus to land is a real readiness signal (same one B3
    // checks), not a fixed sleep, and it reliably lands after both effects have run.
    await page
      .waitForFunction(() => document.activeElement?.closest('.gw-dialog') != null, { timeout: 5_000 })
      .catch(() => {});
  }
  return { trigger, dialog, backdrop };
}

browser = await chromium.launch();

// Reachability gate. Every check below assumes BASE answers; if it doesn't, every one of
// them would fail for the same uninformative reason ("net::ERR_CONNECTION_REFUSED" on
// each of thirteen lines). Fail loudly once, here, and stop — this script must never
// exit 0 because it had nothing to check.
try {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  const response = await page.goto(BASE + '/', { waitUntil: 'domcontentloaded', timeout: 10_000 });
  if (!response || !response.ok()) {
    throw new Error(`unexpected status ${response ? response.status() : '(no response)'}`);
  }
  await ctx.close();
} catch (err) {
  const reason = err instanceof Error ? err.message : String(err);
  console.error(`FAIL: ${BASE} is unreachable — ${reason}`);
  console.error('Cannot run any check against an unreachable site. Is the dev server running?');
  await browser.close();
  process.exit(1);
}

// ---------------------------------------------------------------------------------------
// Group A — the reader (paths that exist today)
// ---------------------------------------------------------------------------------------

await check('A1 home page has a document tree with a link', async (page) => {
  const response = await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });
  assert(response !== null && response.status() === 200, `expected 200, got ${response?.status()}`);

  const tree = page.locator('nav[aria-label="Seitenbaum"]');
  await tree.waitFor({ state: 'visible' });
  const links = tree.locator('a');
  await links.first().waitFor({ state: 'visible' });
  const count = await links.count();
  assert(count >= 1, `expected at least one tree link, found ${count}`);
});

await check('A2 tree links read as links, by hue or underline', async (page) => {
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });

  const link = page.locator('nav[aria-label="Seitenbaum"] a').first();
  await link.waitFor({ state: 'visible' });

  // "Different from body text" is NOT the requirement, and asserting it made this check
  // useless: the actual regression was tree links set to `--ink-muted`, a grey that is
  // perfectly distinct from body ink while reading as de-emphasised label text rather
  // than as something clickable. Reverting the fix therefore passed this check.
  //
  // The requirement is that a navigation link is IDENTIFIABLE as a link, which this
  // design does by hue. An underline is the other legitimate way, so either satisfies it
  // — but muted grey with no underline satisfies neither, which is the case that must
  // fail.
  const { linkColor, accent, underlined } = await link.evaluate((el) => {
    const cs = getComputedStyle(el);
    // Resolve the token through a probe element, so the comparison is against whatever
    // --accent currently is rather than a hex literal copied out of tokens.css.
    const probe = document.createElement('span');
    probe.style.color = 'var(--accent)';
    el.appendChild(probe);
    const accent = getComputedStyle(probe).color;
    probe.remove();
    return {
      linkColor: cs.color,
      accent,
      underlined: cs.textDecorationLine.includes('underline')
    };
  });

  assert(
    linkColor === accent || underlined,
    `a tree link is neither accent-coloured nor underlined, so it does not read as a link ` +
      `(color ${linkColor}, accent ${accent}, underline ${underlined})`
  );
});

await check('A3 a document page renders a real table with scoped headers', async (page) => {
  const response = await page.goto(BASE + '/rundgang/tabellen-was-heute-passiert', {
    waitUntil: 'domcontentloaded'
  });
  assert(response !== null && response.status() === 200, `expected 200, got ${response?.status()}`);

  const table = page.locator('table');
  await table.first().waitFor({ state: 'visible' });
  assert((await table.count()) >= 1, 'expected at least one <table> element');

  const scopedHeader = table.first().locator('th[scope="col"]');
  await scopedHeader.first().waitFor({ state: 'attached' });
  assert((await scopedHeader.count()) >= 1, 'expected at least one <th scope="col">');
});

await check('A4 no horizontal scroll at 390px width', async (page) => {
  await page.setViewportSize({ width: 390, height: 844 });
  // The table page is the sharpest test of this: a wide table is exactly what pushed the
  // page sideways before BlockView.svelte gave it its own scroll box.
  await page.goto(BASE + '/rundgang/tabellen-was-heute-passiert', { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

await check('A5 skip link is the first focusable element', async (page) => {
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });

  // Start from a known position (nothing focused) before the single Tab.
  await page.evaluate(() => document.body.focus());
  await page.keyboard.press('Tab');

  const focused = await page.evaluate(() => ({
    tag: document.activeElement?.tagName ?? null,
    href: document.activeElement instanceof HTMLAnchorElement ? document.activeElement.href : null,
    className: document.activeElement?.className ?? null
  }));
  assert(focused.tag === 'A', `expected an <a> to be focused first, got <${focused.tag}>`);
  // The real markup (web/src/routes/+layout.svelte) is `<a class="skip" href="#content">`,
  // not literally "#main" — the spec allows "or similar", and #content is what this app uses.
  assert(
    focused.href !== null && /#(content|main)$/.test(focused.href),
    `expected the skip link's href to end with #content or #main, got ${focused.href}`
  );
  assert(
    typeof focused.className === 'string' && focused.className.includes('skip'),
    `expected the focused link to carry the "skip" class, got "${focused.className}"`
  );
});

// ---------------------------------------------------------------------------------------
// Group B — the dialog component (web/src/lib/components/Dialog.svelte, Ark UI)
// Mounted in isolation at /_behaviour (see routes/_behaviour/+page.svelte and its
// +page.server.ts guard) because no real route uses Dialog yet.
// ---------------------------------------------------------------------------------------

await check('B1 dialog is not visible before opening', async (page) => {
  const { dialog } = await loadDialogPage(page, { open: false });
  assert(!(await dialog.isVisible()), 'expected .gw-dialog to be hidden on first load');
});

await check('B2 clicking the trigger makes the dialog visible', async (page) => {
  const { dialog } = await loadDialogPage(page, { open: true });
  assert(await dialog.isVisible(), 'expected .gw-dialog to be visible after clicking the trigger');
});

await check('B3 opening the dialog moves focus into it', async (page) => {
  await loadDialogPage(page, { open: true });
  const focusedInside = await page.evaluate(() => document.activeElement?.closest('.gw-dialog') != null);
  assert(focusedInside, 'expected document.activeElement to be inside .gw-dialog after opening');
});

await check('B4 Tab twelve times keeps focus inside the dialog', async (page) => {
  await loadDialogPage(page, { open: true });
  for (let i = 0; i < 12; i += 1) {
    await page.keyboard.press('Tab');
  }
  const focusedInside = await page.evaluate(() => document.activeElement?.closest('.gw-dialog') != null);
  assert(focusedInside, 'focus escaped .gw-dialog after twelve Tab presses (the focus trap did not hold)');
});

await check('B5 Escape makes the dialog not visible', async (page) => {
  // This is the check that caught the real bug: Ark closes the dialog by setting the
  // `hidden` attribute and data-state="closed", relying on the user-agent rule
  // `[hidden] { display: none }`. An author `display` rule with no `:not([hidden])` guard
  // silently outranks that UA rule, so the dialog stayed visible and interactive with
  // data-state="closed" sitting right there in the DOM — nothing threw, nothing logged.
  // `.isVisible()` reads actual rendering (computed display/visibility plus a non-empty
  // bounding box), not the `hidden` attribute or `data-state`, so it cannot be fooled by
  // the same bug that fooled the CSS.
  const { dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  assert(!(await dialog.isVisible()), '.gw-dialog is still visible after Escape');
});

await check('B6 after Escape, the backdrop is also not visible', async (page) => {
  const { backdrop } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  // Wait on the backdrop's own hidden state, not the content's: they are separate Ark
  // Presence regions and do not necessarily flip on the exact same tick. Waiting on
  // .gw-dialog here and then asserting on .gw-dialog-backdrop was a real race in an
  // earlier version of this script — it failed about 1 run in 8 under load even though
  // the component has no bug, because the backdrop occasionally hides one tick later.
  await backdrop.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  assert(!(await backdrop.isVisible()), '.gw-dialog-backdrop is still visible after Escape');
});

await check('B7 after Escape, focus returns to the trigger', async (page) => {
  const { trigger, dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  // Poll for the actual condition (focus-trap restoreFocus completing) rather than
  // trusting that it already happened by the time .gw-dialog went hidden — same class of
  // race as B6, just for a different pair of independently-timed effects.
  const triggerHandle = await trigger.elementHandle();
  await page
    .waitForFunction((el) => el === document.activeElement, triggerHandle, { timeout: 5_000 })
    .catch(() => {});
  const returnedToTrigger = await trigger.evaluate((el) => el === document.activeElement);
  assert(returnedToTrigger, 'focus did not return to the trigger after Escape');
});

await check('B8 the dialog can be reopened after being closed', async (page) => {
  const { trigger, dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  await trigger.click();
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });
  assert(await dialog.isVisible(), '.gw-dialog did not reopen after being closed once');
});

// ---------------------------------------------------------------------------------------
// Group C — sorting and filtering (web/src/lib/components/TableView.svelte)
//
// Against the tour page, which carries two tables on purpose: a two-row one that must stay
// plain, and an eight-row one whose cells were chosen to exercise every rule the comparator
// has (German umlauts, a thousands point, a decimal comma, a comparator prefix, a range,
// ticks and crosses, and one empty cell in each of two columns).
//
// Every assertion below is an EXACT expected value or a structural fact. None of them is of
// the "differs from the previous value" shape that made A2 a false pass for months: a
// regression that reorders rows differently would satisfy "the order changed" just as well
// as the fix does.
// ---------------------------------------------------------------------------------------

const TABLE_PAGE = '/rundgang/tabellen-was-heute-passiert';

/**
 * Polls for a condition instead of sleeping, and reports what it last actually saw — a
 * fixed sleep either flakes or wastes time, and "timed out" with no observed value is the
 * least useful failure a check can produce.
 */
async function until(read, message, timeout = 5_000) {
  const deadline = Date.now() + timeout;
  let last;
  for (;;) {
    last = await read();
    if (last.ok) return last;
    if (Date.now() > deadline) {
      throw new CheckFailure(`${message} — last saw ${JSON.stringify(last.saw)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

/**
 * Loads the tour page and returns both tables. `networkidle` for the reason loadDialogPage
 * gives: the controls are added by client-side hydration, which is a dynamic import that
 * resolves after domcontentloaded — and here that is the whole point, because the SSR HTML
 * deliberately has no controls at all.
 */
async function loadTablePage(page) {
  await page.goto(BASE + TABLE_PAGE, { waitUntil: 'networkidle' });
  const plain = page.locator('.gw-tbl').first();
  const table = page.locator('.gw-tbl').nth(1);
  await table.locator('.gw-tbl-count').waitFor({ state: 'visible', timeout: 5_000 });
  return { plain, table };
}

/** One column of the body, top to bottom, as trimmed text. */
const column = (table, index) =>
  table
    .locator('tbody tr')
    .evaluateAll((rows, i) => rows.map((row) => row.cells[i]?.textContent.trim() ?? ''), index);

const ariaSorts = (table) =>
  table.locator('thead th').evaluateAll((ths) => ths.map((th) => th.getAttribute('aria-sort')));

/** Clicks a column's sort button and waits until `aria-sort` actually says so. */
async function sortBy(table, index, expected) {
  await table.locator('.gw-tbl-sort').nth(index).click();
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[index] === expected, saw };
    },
    `column ${index} never reported aria-sort="${expected}"`
  );
}

await check('C1 a short table gets no controls at all', async (page) => {
  // Two rows do not need a toolbar; a filter box in front of a table you can already see
  // whole is noise, and this one must stay exactly as it was.
  const { plain } = await loadTablePage(page);
  assert((await plain.locator('.gw-tbl-bar').count()) === 0, 'the two-row table grew a toolbar');
  assert((await plain.locator('.gw-tbl-sort').count()) === 0, 'the two-row table grew sort buttons');
  assert(
    (await plain.locator('th[aria-sort]').count()) === 0,
    'the two-row table claims a sort state it cannot change'
  );
});

await check('C2 a long table gets named controls and a row count', async (page) => {
  const { table } = await loadTablePage(page);

  assert(
    (await table.locator('.gw-tbl-count').textContent()) === '8 von 8 Zeilen',
    'the unfiltered row count must still state the total, not just the visible rows'
  );

  // Real buttons in the header, not click handlers on the `th`: only a button is reachable
  // by Tab, operable with both Enter and Space, and announced as something to press.
  const names = await table
    .locator('.gw-tbl-sort')
    .evaluateAll((els) => els.map((el) => el.textContent.replace(/\s+/g, ' ').trim()));
  assert(names.length === 4, `expected four sort buttons, found ${names.length}`);
  assert(
    names[0] === 'Probe, aufsteigend sortieren ⇅',
    `a sort button must say which column AND what pressing it does, got "${names[0]}"`
  );

  // Every column filter is a real <label for=…> naming its own column. Eight boxes all
  // called "Filter" are eight boxes a screen-reader user has to count along.
  const labelled = await table.locator('.gw-tbl-filter').evaluateAll((fields) =>
    fields.map((field) => {
      const label = field.querySelector('label');
      const input = field.querySelector('input');
      return label && input && label.htmlFor === input.id ? label.textContent.trim() : null;
    })
  );
  assert(
    JSON.stringify(labelled) ===
      JSON.stringify(['Probe filtern', 'Menge filtern', 'Geprüft filtern', 'Anteil filtern']),
    `each column filter must be labelled with its column, got ${JSON.stringify(labelled)}`
  );

  const live = await table.locator('.gw-tbl-count').getAttribute('aria-live');
  assert(live === 'polite', `the row count must be a polite live region, got ${live}`);
});

await check('C3 aria-sort cycles ascending, descending, none — one column at a time', async (page) => {
  const { table } = await loadTablePage(page);
  assert(
    JSON.stringify(await ariaSorts(table)) === JSON.stringify(['none', 'none', 'none', 'none']),
    'every sortable column must state a sort of "none" before anything is sorted'
  );

  await sortBy(table, 0, 'ascending');
  assert(
    JSON.stringify(await ariaSorts(table)) ===
      JSON.stringify(['ascending', 'none', 'none', 'none']),
    'exactly one column may claim to be sorted'
  );

  await sortBy(table, 0, 'descending');
  await sortBy(table, 0, 'none');

  // Off means the order the author wrote, which in a document is itself information.
  const back = await column(table, 0);
  assert(
    JSON.stringify(back) ===
      JSON.stringify(['Öl', 'Apfel', 'Ähre', 'Zucker', 'Möhre', 'Äpfel', 'Bohne', 'Nuss']),
    `a third press must restore document order, got ${JSON.stringify(back)}`
  );
});

await check('C4 a numeric column sorts by value, with empties last both ways', async (page) => {
  const { table } = await loadTablePage(page);

  await sortBy(table, 1, 'ascending');
  const up = await column(table, 1);
  assert(
    JSON.stringify(up) ===
      JSON.stringify(['<0,5 g', '1,5 g', '3-5 g', '42 g', '80 g', '900 g', '1.200 g', '']),
    `units, comparator prefixes, a range and a thousands point must all be read, got ${JSON.stringify(up)}`
  );

  await sortBy(table, 1, 'descending');
  const down = await column(table, 1);
  assert(
    JSON.stringify(down) ===
      JSON.stringify(['1.200 g', '900 g', '80 g', '42 g', '3-5 g', '1,5 g', '<0,5 g', '']),
    `descending must reverse the values, got ${JSON.stringify(down)}`
  );
  // The one that a negated comparator gets wrong: the gap must NOT float to the top, where
  // it reads as "these are the matches" and pushes the rows being hunted for off the bottom.
  assert(down[down.length - 1] === '', 'an empty cell must stay last when sorting descending');
});

await check('C5 German text sorts as a German reader expects', async (page) => {
  const { table } = await loadTablePage(page);
  await sortBy(table, 0, 'ascending');
  const names = await column(table, 0);
  assert(
    JSON.stringify(names) ===
      JSON.stringify(['Ähre', 'Apfel', 'Äpfel', 'Bohne', 'Möhre', 'Nuss', 'Öl', 'Zucker']),
    `codepoint order would put every umlaut after Z; got ${JSON.stringify(names)}`
  );
});

await check('C6 a second sort keeps the first one inside its ties', async (page) => {
  const { table } = await loadTablePage(page);
  await sortBy(table, 0, 'ascending'); // by name
  await sortBy(table, 2, 'ascending'); // then by the tick column, which has three ties

  const ticks = await column(table, 2);
  const names = await column(table, 0);
  const combined = ticks.map((tick, i) => `${tick}:${names[i]}`);
  assert(
    JSON.stringify(combined) ===
      JSON.stringify([
        '❌:Apfel',
        '❌:Nuss',
        '❌:Zucker',
        '✅:Ähre',
        '✅:Bohne',
        '✅:Möhre',
        '✅:Öl',
        '—:Äpfel'
      ]),
    `inside each group the previous sort must survive, and a lone dash counts as empty; got ${JSON.stringify(combined)}`
  );
  assert(combined.length === 8, 'no row may be lost or duplicated by sorting');
});

await check('C7 filtering changes the count and never hides the total', async (page) => {
  const { table } = await loadTablePage(page);
  const search = table.locator('.gw-tbl-bar input');

  await search.fill('öl');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '1 von 8 Zeilen', saw };
    },
    'the row count must state both the visible rows and the total'
  );
  assert((await table.locator('tbody tr').count()) === 1, 'exactly one row matches "öl"');

  // Case and umlaut marks are folded, so a filter typed in a hurry still finds the row.
  await search.fill('OL');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '1 von 8 Zeilen', saw };
    },
    '"OL" must find "Öl" — a filter that only matches perfect spelling is a filter nobody uses'
  );

  // A column filter confines itself to its column.
  await search.fill('');
  await table.locator('.gw-tbl-filter input').nth(2).fill('✅');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '4 von 8 Zeilen', saw };
    },
    'the tick column filter must keep exactly the four ticked rows'
  );

  // Reset puts everything back and then has nothing left to do.
  await table.locator('.gw-tbl-reset').click();
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '8 von 8 Zeilen', saw };
    },
    'resetting the filters must restore every row'
  );
  assert(
    await table.locator('.gw-tbl-reset').isDisabled(),
    'with nothing filtered the reset button must not offer to do anything'
  );
});

await check('C8 a filter that matches nothing says so', async (page) => {
  // An empty tbody looks like a broken table. Saying "0 von 8" and naming the reason is the
  // difference between a filtered table and a lost one.
  const { table } = await loadTablePage(page);
  await table.locator('.gw-tbl-bar input').fill('zzzz');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '0 von 8 Zeilen', saw };
    },
    'the count must report zero against the total'
  );
  const empty = table.locator('.gw-tbl-empty');
  await empty.waitFor({ state: 'visible', timeout: 5_000 });
  assert(
    (await empty.getAttribute('colspan')) === '4',
    'the message must span the table rather than sitting under the first column'
  );
});

await check('C9 a sort button works from the keyboard, with Enter and with Space', async (page) => {
  const { table } = await loadTablePage(page);
  const button = table.locator('.gw-tbl-sort').nth(1);
  await button.focus();

  await page.keyboard.press('Enter');
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[1] === 'ascending', saw };
    },
    'Enter did not sort the column'
  );

  // Space too. A div with a click handler answers Enter through no mechanism at all and
  // Space never — which is exactly the failure a real <button> exists to prevent.
  await page.keyboard.press(' ');
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[1] === 'descending', saw };
    },
    'Space did not sort the column'
  );

  assert(
    await button.evaluate((el) => el === document.activeElement),
    'focus must stay on the button that was pressed'
  );
});

await check('C10 the header stays put while the table scrolls under it', async (page) => {
  // Sized so eight rows cannot fit in the box's 70vh, which is what makes the box a real
  // scrollport — a sticky header in a box that never scrolls looks correct and does nothing.
  await page.setViewportSize({ width: 1280, height: 320 });
  const { table } = await loadTablePage(page);
  const box = table.locator('.gw-tbl-scroll');

  const scrollable = await box.evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  assert(scrollable, 'the scroll box must actually scroll vertically at this height');

  const geometry = await box.evaluate(async (el) => {
    el.scrollTop = 200;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const th = el.querySelector('thead th');
    const firstRow = el.querySelector('tbody tr');
    return {
      offset: th.getBoundingClientRect().top - el.getBoundingClientRect().top,
      firstRowTop: firstRow.getBoundingClientRect().top,
      headerTop: th.getBoundingClientRect().top,
      background: getComputedStyle(th).backgroundColor
    };
  });
  assert(
    Math.abs(geometry.offset) < 2,
    `the header must sit at the top of the scrolled box, it sat ${geometry.offset}px away`
  );
  assert(
    geometry.firstRowTop < geometry.headerTop,
    'the first row must have scrolled up behind the header, not stayed below it'
  );
  // Without an opaque background the rows scroll THROUGH the header and both become
  // unreadable — which is a rendering bug that a geometry assertion alone cannot see.
  assert(
    geometry.background !== 'rgba(0, 0, 0, 0)' && geometry.background !== 'transparent',
    `the sticky header must be opaque, computed background was ${geometry.background}`
  );
});

// ---------------------------------------------------------------------------------------
// Group D — the page metadata (Breadcrumb.svelte, PageMeta.svelte, Subpages.svelte)
//
// Against the tour corpus, which has what this needs: a container page with six children,
// a branch three levels deep, and pages that are actually `public` — so every check below
// works for an anonymous visitor. D5 is the one check that names `/rundgang/nur-intern`,
// and only to assert its ABSENCE from an anonymous visitor's subpage list: it is
// `restricted` on purpose (no `visibility:` in its frontmatter — fail closed), so
// `/rundgang` has six children in the store and five visible ones here. Nothing below
// navigates to `/rundgang/nur-intern` itself, which would need a dev identity these
// checks do not assume.
//
// The colour assertions resolve the token through a probe element rather than comparing
// against a hex copied out of tokens.css, and they assert EQUALITY with the resolved
// value. "Differs from something else" is the shape that made A2 a false pass for months.
// ---------------------------------------------------------------------------------------

const CONTAINER_PAGE = '/rundgang'; // public, six children
const NESTED_PAGE = '/rundgang/import-export'; // public, one child, two levels down
const DEEP_PAGE = '/rundgang/import-export/heikel'; // public, the deepest page there is

/** The computed value of a custom property, as this page currently resolves it. */
const resolveToken = (locator, token) =>
  locator.evaluate((el, name) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${name})`;
    el.appendChild(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, token);

await check('D1 the whole panel is in the HTML the server sends, before any script runs', async (page) => {
  // `page.request` is a plain HTTP fetch through the browser's networking stack: nothing
  // is rendered, nothing hydrates, no module is imported. What comes back is exactly what
  // a reader with JavaScript switched off receives — which is the requirement, and which
  // no amount of poking at the live DOM can demonstrate, because by then the app has run.
  const response = await page.request.get(BASE + CONTAINER_PAGE);
  assert(response.ok(), `expected 200 from ${CONTAINER_PAGE}, got ${response.status()}`);
  const html = await response.text();

  assert(/<nav[^>]*aria-label="Pfad"/.test(html), 'no breadcrumb in the server-rendered HTML');
  assert(
    /aria-label="Angaben zu dieser Seite"/.test(html),
    'no metadata panel in the server-rendered HTML'
  );
  assert(
    html.includes('Öffentlich im Internet'),
    'the server-rendered HTML does not state the visibility'
  );

  // The subpage links, checked INSIDE the subpage list. Every one of these paths also
  // appears in the sidebar tree on the same page, so searching the whole document would
  // pass with the list missing entirely.
  const section = html.match(/<nav[^>]*aria-labelledby="gw-subpages"[\s\S]*?<\/nav>/)?.[0];
  assert(section !== undefined, 'no subpage list in the server-rendered HTML');
  assert(section.includes('Unterseiten'), 'the subpage list is not headed');
  assert(
    section.includes(`href="${NESTED_PAGE}"`),
    'the subpage list does not link the child it is supposed to list'
  );
});

await check('D2 breadcrumb links read as chrome, and still read as links', async (page) => {
  // Two failures in one check, because the fix for either alone reintroduces the other.
  //
  // The colour is muted ink DELIBERATELY — accent blue above every heading competes with
  // the heading. But muted with no underline is exactly the regression A2 documents: it
  // reads as a row of labels rather than as something you can click. So: muted AND
  // underlined, both asserted.
  //
  // Equality with the resolved `--ink-muted` is also the only thing that would notice a
  // cascade-layer regression here. `@layer base` sets `a { color: var(--accent) }`; the
  // breadcrumb's own rule lives in `@layer components` and wins only because `components`
  // comes later in the order app.css declares. Load the page stylesheet before the
  // layout's `@layer` statement and the order silently inverts — every breadcrumb turns
  // accent blue and nothing else in this suite notices.
  await page.goto(BASE + NESTED_PAGE, { waitUntil: 'domcontentloaded' });
  const link = page.locator('nav[aria-label="Pfad"] a').first();
  await link.waitFor({ state: 'visible' });

  const muted = await resolveToken(link, '--ink-muted');
  const { color, underlined } = await link.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { color: cs.color, underlined: cs.textDecorationLine.includes('underline') };
  });

  assert(color === muted, `a breadcrumb link is ${color}, expected --ink-muted (${muted})`);
  assert(underlined, 'a breadcrumb link is muted AND unmarked, so it does not read as a link');
});

await check('D3 the breadcrumb names every level and marks only the current one', async (page) => {
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  const crumbs = page.locator('nav[aria-label="Pfad"] a');
  await crumbs.first().waitFor({ state: 'visible' });

  const hrefs = await crumbs.evaluateAll((els) => els.map((el) => el.getAttribute('href')));
  assert(
    JSON.stringify(hrefs) ===
      JSON.stringify(['/', '/rundgang', NESTED_PAGE, DEEP_PAGE]),
    `the breadcrumb must run root-first through every ancestor, got ${JSON.stringify(hrefs)}`
  );

  const marked = await page
    .locator('nav[aria-label="Pfad"] a[aria-current="page"]')
    .evaluateAll((els) => els.map((el) => el.getAttribute('href')));
  assert(
    JSON.stringify(marked) === JSON.stringify([DEEP_PAGE]),
    `exactly one crumb — the last — may claim to be the current page, got ${JSON.stringify(marked)}`
  );
});

await check('D4 a breadcrumb ancestor and a subpage link both actually navigate', async (page) => {
  // The two directions a reader moves from this page. Asserted by navigating, not by
  // reading the href: an anchor with a correct href inside a container that swallows the
  // click is a link that looks right in the markup and does nothing.
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  await page.locator(`nav[aria-label="Pfad"] a[href="${NESTED_PAGE}"]`).click();
  await page.waitForURL(`**${NESTED_PAGE}`, { timeout: 5_000 });

  const down = page.locator(`nav[aria-labelledby="gw-subpages"] a[href="${DEEP_PAGE}"]`);
  await down.waitFor({ state: 'visible' });
  await down.click();
  await page.waitForURL(`**${DEEP_PAGE}`, { timeout: 5_000 });

  const heading = (await page.locator('h1').first().textContent()) ?? '';
  assert(
    heading.trim().startsWith('Heikler Text'),
    `the subpage link did not land on the subpage, the heading says "${heading.trim()}"`
  );
});

await check('D5 a container page lists its children, a leaf offers no empty section', async (page) => {
  // The tour page has SIX children in content-example, one of which — /rundgang/nur-intern
  // — carries no `visibility:` and is therefore `restricted` by default (fail closed).
  // An anonymous visitor correctly sees the other five: that is the permission filter
  // working, not a bug, so the assertion has to say "five, and specifically not the
  // restricted one" rather than a bare count that a filtering regression could still
  // satisfy by coincidence (e.g. hiding some OTHER child instead).
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'domcontentloaded' });
  const list = page.locator('nav[aria-labelledby="gw-subpages"] a');
  await list.first().waitFor({ state: 'visible' });

  const hrefs = await list.evaluateAll((els) => els.map((el) => el.getAttribute('href')));
  assert(
    hrefs.length === 5,
    `expected 5 visible children (six exist, one restricted), got ${hrefs.length}: ${JSON.stringify(hrefs)}`
  );
  assert(
    !hrefs.includes('/rundgang/nur-intern'),
    `the restricted child /rundgang/nur-intern must not appear to an anonymous visitor, got ${JSON.stringify(hrefs)}`
  );
  assert(
    hrefs.includes(NESTED_PAGE),
    `expected the public child ${NESTED_PAGE} in the list, got ${JSON.stringify(hrefs)}`
  );

  // The one child that has a child of its own says so, and says it in German.
  const note = await page
    .locator(`nav[aria-labelledby="gw-subpages"] li:has(a[href="${NESTED_PAGE}"]) .count`)
    .textContent();
  assert(note?.trim() === '1 Unterseite', `expected "1 Unterseite", got "${note?.trim()}"`);

  // A page with no children gets no heading, no rule and no empty list — a permanent cost
  // for every leaf otherwise, and an empty section reads as a section that failed to load.
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  const sections = await page.locator('nav[aria-labelledby="gw-subpages"]').count();
  assert(sections === 0, 'a page with no children still rendered a subpage section');
});

await check('D6 a world-readable page says so, in the colour that means "notice this"', async (page) => {
  // Not a value judgement about being public — it is the state whose consequences are
  // irreversible if unintended, on a wiki whose every imported page is `restricted`. The
  // words carry the meaning on their own; the tint only makes it visible from across the
  // room, and is mixed from `--warn` so a theme repaints it along with everything else.
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'domcontentloaded' });
  const chip = page.locator('[aria-label="Angaben zu dieser Seite"] .chip');
  await chip.waitFor({ state: 'visible' });

  const label = (await chip.textContent())?.trim();
  assert(
    label === 'Öffentlich im Internet',
    `"Öffentlich" alone reads as "everyone in the organisation" in an intranet; got "${label}"`
  );
  assert(
    (await chip.getAttribute('data-visibility')) === 'public',
    'the chip does not state which of the three levels it is showing'
  );

  const detail = await page
    .locator('[aria-label="Angaben zu dieser Seite"] .detail')
    .textContent();
  assert(
    detail?.trim() === 'Jede Person kann diese Seite ohne Anmeldung lesen.',
    `the sentence that makes the label unmistakable is missing, got "${detail?.trim()}"`
  );

  const warn = await resolveToken(chip, '--warn');
  const border = await chip.evaluate((el) => getComputedStyle(el).borderTopColor);
  assert(border === warn, `the public chip is bordered ${border}, expected --warn (${warn})`);
});

await check('D7 no horizontal scroll at 390px with the panel and the subpage grid', async (page) => {
  // A4 checks this for the widest thing a document can contain. This checks it for the
  // two new grids: the panel's `max-content` label column, which a long term can push
  // wide, and the subpage list's `minmax(14rem, 1fr)` columns.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

// ---------------------------------------------------------------------------------------
// Group E — the editor (web/src/lib/editor/**)
//
// E1-E6 work for BOTH answers to "may this developer write this page" on purpose: `seed`
// creates no write grants and no migration inserts any, so a fresh, ungranted instance —
// the state nearly every developer and every reader actually meets — is a real case this
// suite must cover, not one it is allowed to assume away. What must be true either way is
// that the reading page never regresses, that no editable surface exists before the server
// has agreed to one, and that whatever the editor says about the work is not a lie.
//
// `just behaviour` provisions its own fixture (see the justfile) and that fixture grants
// `group:editors` write on `/rundgang`, so under it EDIT_PAGE below is ALWAYS live for the
// `GW_DEV_IDENTITY` this harness runs the server as. E1-E6 still tolerate a refused session
// defensively — nothing here is deleted for being "the branch that never fires" — but E7
// needs a real editing surface to type into and MUST NOT be one of the checks that quietly
// passed for months by exiting before it asserted anything: see its own comment below.
// ---------------------------------------------------------------------------------------

const EDIT_PAGE = '/rundgang';

/** Waits for the session to settle into a state that has a headline. */
async function loadEditor(page) {
  await page.goto(BASE + EDIT_PAGE + '?edit=1', { waitUntil: 'networkidle' });
  const region = page.locator('section[aria-label="Seite bearbeiten"]');
  await region.waitFor({ state: 'visible', timeout: 10_000 });
  const head = region.locator('.gw-ed-status-head');
  await head.waitFor({ state: 'visible', timeout: 10_000 });
  // Not "connecting": that is the state before an answer, and asserting against it would
  // pass whatever the answer turned out to be.
  const settled = await until(
    async () => {
      const saw = (await head.textContent())?.trim() ?? '';
      return { ok: saw.length > 0 && !saw.includes('wird geöffnet'), saw };
    },
    'the editing session never settled into an answer',
    10_000
  );
  return { region, headline: settled.saw };
}

await check('E1 asking to edit still serves the whole document in the first response', async (page) => {
  // The requirement that outranks the feature. `page.request` is a plain fetch: nothing
  // hydrates, no module is imported, so this is exactly what a reader with JavaScript off
  // receives — and it must be the page, not a mount point for one.
  const response = await page.request.get(BASE + EDIT_PAGE + '?edit=1');
  assert(response.ok(), `expected 200, got ${response.status()}`);
  const html = await response.text();

  assert(/<article[^>]*class="prose/.test(html), 'the document is not in the server-rendered HTML');
  assert(/<nav[^>]*aria-label="Pfad"/.test(html), 'the breadcrumb went missing while editing');
  // And nothing editable, because the server has not been asked yet whether this caller may.
  //
  // The ATTRIBUTE, not the word. This ran against `html.includes('contenteditable')` until a
  // CSS comment explaining which element carries `.prose` said "contenteditable" in prose —
  // and this harness drives the DEV server, where Vite inlines the stylesheet, comments and
  // all. It failed for a page that had no editable surface at all, which is the worst kind
  // of check: one that is loud, correct-looking and about the wrong thing. A real surface is
  // always `contenteditable="true"`, so requiring the `=` loses nothing and stops the next
  // person having to word a comment around a test.
  assert(!/contenteditable\s*=/.test(html), 'the SSR HTML contains an editable surface');
  assert(!/role\s*=\s*"textbox"/.test(html), 'the SSR HTML claims an editing surface exists');
});

await check('E2 the editor never both refuses and offers a place to type', async (page) => {
  // The one thing that must never happen, whichever way the permission goes: an editor that
  // appears and silently throws keystrokes away. Either the session is live and there is a
  // surface, or it is not and there is none — never a surface without a live session.
  const { region, headline } = await loadEditor(page);
  const live = headline.includes('Verbunden');
  const surfaces = await region.locator('[contenteditable="true"]').count();

  if (live) {
    assert(surfaces === 1, `a live session must have exactly one surface, found ${surfaces}`);
  } else {
    assert(
      surfaces === 0,
      `the session says "${headline}" and yet offers ${surfaces} place(s) to type`
    );
    // A refusal has to say what to do about it, and must not read as a network fault.
    const detail = await region.locator('.gw-ed-status-detail').textContent();
    assert(
      /Berechtigung|Verbindung|Server/.test(detail ?? ''),
      `the refusal explains nothing: "${detail}"`
    );
  }
});

await check('E3 the reading page is intact underneath, whatever the session decided', async (page) => {
  // A refused session must leave a readable page behind, not a blank frame where the
  // document was. This is the failure a reader would notice first and forgive least.
  const { region, headline } = await loadEditor(page);
  if (headline.includes('Verbunden')) {
    const text = await region.locator('[contenteditable="true"]').textContent();
    assert((text ?? '').trim().length > 0, 'the live editor mounted with no content in it');
  } else {
    const article = page.locator('article.prose');
    await article.waitFor({ state: 'visible', timeout: 5_000 });
    const text = await article.textContent();
    assert((text ?? '').trim().length > 0, 'a refused session left an empty page behind');
  }
});

await check('E4 the history warning is on screen the whole time somebody is editing', async (page) => {
  // D-M2-9 and D-M3-5 require this AT THE POINT OF EDITING rather than in documentation:
  // anyone who may read the page may read every revision, so removing a sentence is an edit
  // and not a redaction. Somebody pasting a password needs to learn that now, not later.
  const { region } = await loadEditor(page);
  const warning = region.locator('.gw-ed-history');
  await warning.waitFor({ state: 'visible', timeout: 5_000 });
  const text = (await warning.textContent()) ?? '';
  assert(/Versionsgeschichte/.test(text), `the warning does not mention the history: "${text}"`);
  assert(/lösch/i.test(text), `the warning does not mention deleting: "${text}"`);
  // Permanent, not a toast: nothing may dismiss it.
  assert(
    (await region.locator('.gw-ed-history button').count()) === 0,
    'the history warning can be dismissed, which makes it a notification rather than a fact'
  );
});

await check('E5 the toolbar offers exactly what a revision can actually store', async (page) => {
  // `gw_core::Block` grew a `marks` field (Task 5) and `gw-collab` now writes and reads them,
  // so the five marks below joined the block controls that were always here. The check keeps
  // its original shape — an exact list, not a superset check — because that shape is what
  // catches EITHER direction of drift: a control added for something the server cannot store,
  // or one silently missing for something it can.
  const { region } = await loadEditor(page);
  const toolbar = region.locator('[role="toolbar"]');
  await toolbar.waitFor({ state: 'visible', timeout: 5_000 });

  const labels = await toolbar
    .locator('button')
    .evaluateAll((els) => els.map((el) => el.getAttribute('aria-label')));
  assert(
    JSON.stringify(labels) ===
      JSON.stringify([
        'Überschrift 2',
        'Überschrift 3',
        'Überschrift 4',
        'Aufzählung',
        'Nummerierte Liste',
        'Zitat',
        'Codeblock',
        'Fett',
        'Kursiv',
        'Code',
        'Durchgestrichen',
        'Link'
      ]),
    `the toolbar offers something the server cannot store, or lost something it can: ${JSON.stringify(labels)}`
  );
  // `MarkKind` has no `underline` — that control must still never appear, for the same
  // reason a bold button could not before this task: publishing would throw its effect away.
  assert(!labels.includes('Unterstrichen'), 'the toolbar offers "Unterstrichen", which publishing drops');
});

await check('E6 a control that cannot reach the document says so by being disabled', async (page) => {
  // A refused session's toolbar must not look operable. An enabled button that does nothing
  // is the same lie as an editor that discards keystrokes, in a smaller box.
  const { region, headline } = await loadEditor(page);
  const buttons = region.locator('[role="toolbar"] button');
  await buttons.first().waitFor({ state: 'attached', timeout: 5_000 });
  const disabled = await buttons.evaluateAll((els) => els.map((el) => el.disabled));

  if (headline.includes('Verbunden')) {
    assert(
      disabled.every((d) => d === false),
      'a live session left its toolbar disabled'
    );
  } else {
    assert(
      disabled.every((d) => d === true),
      `the session says "${headline}" and yet its toolbar is operable`
    );
  }
});

await check('E7 toggling Fett on a live selection marks it up, and the toolbar agrees', async (page) => {
  // The live counterpart to `extensions.test.ts`'s CRDT-level proof. That test calls the
  // exact conversion `@tiptap/extension-collaboration` uses to sync into a Y.Doc directly, so
  // it already pins the wire keys; this exercises the part it cannot reach — a real browser,
  // a real ContentEditable, the real ProseMirror keymaps — to confirm the renamed `Strong`
  // extension still behaves like a normal mark all the way through typing and toggling it.
  //
  // This needs a live session to type into, and — unlike E1-E6 — has nothing meaningful to
  // assert without one. `just behaviour`'s fixture grants write on EDIT_PAGE precisely so
  // this precondition holds; the assertion below is what used to be a silent `return` on a
  // refused session, which is exactly the shape that let 17 failures pass unnoticed for
  // months. A check that cannot run must fail, not exit quietly.
  const { region, headline } = await loadEditor(page);
  assert(
    headline.includes('Verbunden'),
    `E7 needs a live editing session on ${EDIT_PAGE} to type into and there isn't one — ` +
      `headline says "${headline}". Is the behaviour fixture's write grant missing (see ` +
      `\`just behaviour\` / \`just behaviour-fixture\`)?`
  );

  const surface = region.locator('[contenteditable="true"]');
  await surface.click();

  // The document is cleared first, so what is typed becomes the WHOLE document rather than
  // a run inside the seeded prose — and selecting it is then `Mod-a`, not `Shift+Home` or a
  // count of `Shift+ArrowLeft` presses. Both of those were tried and both were genuinely
  // flaky (confirmed over repeated runs against a freshly-seeded fixture, not an artefact of
  // reusing one document across runs): a selection built by moving the native caret is only
  // as current as the browser's last `selectionchange` event, which ProseMirror's view picks
  // up asynchronously — so a command issued right after can act on the STALE selection from
  // before the caret moved, silently doing nothing. `Mod-a` and `Mod-b` are both bound as
  // ProseMirror commands (`prosemirror-commands`' `selectAll`, and `Strong`/`Bold`'s own
  // keymap — see extensions.ts), which read and write `state.selection` directly inside the
  // SAME synchronous keydown handler; neither one waits on the DOM to tell it what changed.
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Backspace');

  const marker = 'fettmarke';
  await page.keyboard.type(marker);
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Control+b');

  const fettButton = region.locator('[role="toolbar"] button[aria-label="Fett"]');
  await until(
    async () => {
      const saw = await fettButton.getAttribute('data-state');
      return { ok: saw === 'on', saw };
    },
    'the toolbar never reported Fett pressed after Mod-b, so it disagrees with the editor'
  );

  const html = await surface.innerHTML();
  assert(
    new RegExp(`^<p><strong[^>]*>${marker}</strong></p>$`).test(html.trim()),
    `expected the document to be exactly one bolded "${marker}" run, got: ${html}`
  );
});

// ---------------------------------------------------------------------------------------
// Group F — the graph (web/src/routes/graph/+page.svelte, web/src/lib/graph/layout.ts)
//
// The one screen in this plan that is a PICTURE rather than text or a control, so it is the
// one none of groups A-E can stand in for: a wrong arrowhead direction, an unreachable node
// or a broken focus style are all things `page.test.ts`'s server-rendered-string assertions
// cannot see, because they are about what the browser actually does with the markup, not
// about which tags are in it.
//
// Against `/verweisbeispiel` and its child `/verweisbeispiel/verweist-zurueck`
// (content-example), added alongside this task for exactly this purpose: `content-example`
// otherwise has no links at all between its pages (every existing tour page is prose with no
// `[text](/…)` in it), so the graph route had nothing real to draw in a fresh clone. The
// child is seeded AFTER its parent — collect_markdown's shallowest-first order guarantees
// that regardless of alphabetical position among root files — so the one link in it
// (`[Verweisbeispiel](/verweisbeispiel)`) always resolves to a document that already exists,
// giving a graph of exactly two nodes and one edge, deterministically, from `content-example`
// alone. `content-darm` is NOT used here: every page in it is `restricted`, which would need
// a dev identity these checks do not assume, and this task is explicit that `content-darm/`
// is not to be touched.
// ---------------------------------------------------------------------------------------

const GRAPH_SOURCE = '/verweisbeispiel/verweist-zurueck'; // the edge starts here
const GRAPH_TARGET = '/verweisbeispiel'; // and points here

await check('F1 an arrowhead marker is present and the edge points from source to target', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });
  const svg = page.locator('svg');
  await svg.waitFor({ state: 'visible' });

  // The marker itself: a triangle, referenced by the line as `marker-end`. Not merely
  // "some <marker> exists" — the exact id the line points at, and a path inside it, or a
  // marker with nothing to draw would satisfy "a marker is present" just as well.
  const markerPath = svg.locator('marker#gw-graph-pfeil path');
  assert((await markerPath.count()) === 1, 'expected exactly one <path> inside marker#gw-graph-pfeil');

  const line = svg.locator('.edges line');
  assert((await line.count()) === 1, `expected exactly one edge, found ${await line.count()}`);
  assert(
    (await line.getAttribute('marker-end')) === 'url(#gw-graph-pfeil)',
    'the edge line does not reference the arrowhead marker'
  );

  // Direction: `edgeLine` starts the line at the SOURCE node's rim and ends it at the
  // TARGET node's rim (see web/src/lib/graph/layout.ts), so (x1,y1) must sit closer to the
  // source's centre than to the target's, and (x2,y2) the other way round. A marker on the
  // wrong end of a correctly-drawn line would pass every check above and still point the
  // wrong way, which is why this is checked separately from "a marker exists".
  const centre = async (href) => {
    const circle = svg.locator(`a[href="${href}"] circle`);
    const [cx, cy] = await Promise.all([circle.getAttribute('cx'), circle.getAttribute('cy')]);
    return { x: Number(cx), y: Number(cy) };
  };
  const source = await centre(GRAPH_SOURCE);
  const target = await centre(GRAPH_TARGET);
  const [x1, y1, x2, y2] = await Promise.all([
    line.getAttribute('x1'),
    line.getAttribute('y1'),
    line.getAttribute('x2'),
    line.getAttribute('y2')
  ]).then((vals) => vals.map(Number));

  const dist = (ax, ay, b) => Math.hypot(ax - b.x, ay - b.y);
  assert(
    dist(x1, y1, source) < dist(x1, y1, target),
    `the line's start (${x1},${y1}) is not closer to the source node ${JSON.stringify(source)} than to the target ${JSON.stringify(target)}`
  );
  assert(
    dist(x2, y2, target) < dist(x2, y2, source),
    `the line's end (${x2},${y2}), where the arrowhead sits, is not closer to the target node ${JSON.stringify(target)} than to the source ${JSON.stringify(source)}`
  );
});

await check('F2 a node is a real link and navigating it reaches that page', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });
  const node = page.locator(`svg a[href="${GRAPH_TARGET}"]`);
  await node.waitFor({ state: 'visible' });
  await node.click();
  await page.waitForURL(`**${GRAPH_TARGET}`, { timeout: 5_000 });

  const heading = (await page.locator('h1').first().textContent())?.trim() ?? '';
  assert(
    heading === 'Verweisbeispiel',
    `clicking the node did not land on its page: heading reads "${heading}"`
  );
});

await check('F3 keyboard focus reaches a node, with a focus style that is visibly distinct', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });

  // Start from the last thing before the diagram in DOM order — the filter button — and
  // Tab once. The <line>s and the marker's <path> carry no tabindex, so the very next stop
  // after the button is the first node's own <a>.
  await page.locator('.filter button[type="submit"]').focus();
  await page.keyboard.press('Tab');

  const focused = page.locator('svg .nodes a:focus');
  assert(
    (await focused.count()) === 1,
    'Tab from the filter button did not land on a node link inside svg .nodes'
  );

  // The design's own rule (+page.svelte): `.nodes a:focus-visible circle { fill: var(--ink) }`
  // against a default of `var(--accent)`. Resolved through a probe element and compared for
  // EQUALITY with --ink specifically — not merely "different from before" — because a focus
  // style that changed to some OTHER wrong colour would pass an inequality check too.
  const circle = focused.locator('circle');
  const ink = await resolveToken(circle, '--ink');
  const fill = await circle.evaluate((el) => getComputedStyle(el).fill);
  assert(fill === ink, `a focused node's circle is filled ${fill}, expected --ink (${ink})`);
});

await check('F4 no horizontal scroll at 390px width', async (page) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(BASE + '/graph', { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

await check('F5 the empty state renders its German sentence and no <svg>', async (page) => {
  // A root naming a subtree that does not exist answers an empty graph, not an error (see
  // `within_root` in crates/gw-store/src/links.rs) — a deterministic, content-independent
  // way to reach the empty state without relying on the whole wiki having no links.
  await page.goto(BASE + '/graph?wurzel=%2Fnichts-hier-xyz-123', { waitUntil: 'domcontentloaded' });

  const empty = page.locator('p.empty');
  await empty.waitFor({ state: 'visible' });
  const text = (await empty.textContent())?.replace(/\s+/g, ' ').trim() ?? '';
  assert(
    text.startsWith('Noch keine Verweise'),
    `expected the empty-state paragraph to start with "Noch keine Verweise", got "${text}"`
  );
  assert((await page.locator('svg').count()) === 0, 'the empty state must render no <svg> at all');
});

// ---------------------------------------------------------------------------------------
// Group G — the Content-Security-Policy (web/vite.config.ts `kit.csp`, web/src/hooks.server.ts,
// crates/gw-api/src/csp.rs)
//
// A header is a deterministic thing to assert and needs no dev server state, which is why it
// is here rather than in vitest: the value only exists once something has actually rendered
// a page, and half of what matters about it is whether the BROWSER honours it — a policy
// that is sent and then quietly ignored looks identical from `curl`.
//
// Two of these run against `just dev`, and dev is NOT production: SvelteKit adds
// `'unsafe-inline'` to `style-src` in development so it can inject component styles, so
// nothing below may assert that `style-src` is strict. What is asserted is what holds in
// both — `default-src 'self'`, a nonce in `script-src` with no `'unsafe-inline'` beside it,
// and the directives that have no dev/prod difference at all.
// ---------------------------------------------------------------------------------------

/** The `content-security-policy` header of a plain GET, split into directive → sources. */
async function policyOf(page, path) {
  const response = await page.goto(BASE + path, { waitUntil: 'domcontentloaded' });
  const header = (await response.headerValue('content-security-policy')) ?? '';
  assert(header !== '', `${path} answered ${response.status()} with no Content-Security-Policy`);
  return Object.fromEntries(
    header
      .split(';')
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const [name, ...sources] = part.split(/\s+/);
        return [name.toLowerCase(), sources];
      })
  );
}

await check('G1 every rendered page carries a policy with a default-src of self', async (page) => {
  // The 404 is in the list on purpose: SvelteKit renders it like any other page, and a
  // policy that covered only the routes somebody remembered is the class of gap this whole
  // header exists to close.
  for (const path of ['/', '/rundgang', '/graph', '/gibt-es-nicht-xyz']) {
    const policy = await policyOf(page, path);
    assert(
      policy['default-src']?.join(' ') === "'self'",
      `${path}: default-src is ${JSON.stringify(policy['default-src'])}, expected ["'self'"]`
    );
    assert(policy['object-src']?.join(' ') === "'none'", `${path}: object-src is not 'none'`);
    assert(policy['base-uri']?.join(' ') === "'none'", `${path}: base-uri is not 'none'`);
    assert(
      policy['frame-ancestors']?.join(' ') === "'self'",
      `${path}: frame-ancestors does not match the edge's X-Frame-Options: SAMEORIGIN`
    );
  }
});

await check('G2 script-src is nonce-based and never allows unsafe-inline or eval', async (page) => {
  // The directive the whole policy is for. `'self'` stays beside the nonce deliberately:
  // a nonce does not propagate to a dynamic `import()`, and TipTap and Yjs arrive as
  // dynamically imported chunks.
  const policy = await policyOf(page, '/');
  const sources = policy['script-src'] ?? [];
  assert(sources.includes("'self'"), `script-src lacks 'self': ${sources.join(' ')}`);
  assert(
    sources.some((source) => source.startsWith("'nonce-")),
    `script-src carries no nonce: ${sources.join(' ')}`
  );
  for (const forbidden of ["'unsafe-inline'", "'unsafe-eval'", "'wasm-unsafe-eval'"]) {
    assert(!sources.includes(forbidden), `script-src allows ${forbidden}: ${sources.join(' ')}`);
  }
});

await check('G3 the browser enforces it: an injected inline script does not run', async (page) => {
  // Sent is not enforced. Without this check the header could be malformed in a way every
  // browser ignores and G1 and G2 would both still pass.
  await page.goto(BASE + '/', { waitUntil: 'networkidle' });
  const ran = await page.evaluate(() => {
    const script = document.createElement('script');
    script.textContent = 'window.__gwCspProbe = true;';
    document.head.appendChild(script);
    return window.__gwCspProbe === true;
  });
  assert(!ran, 'an inline <script> injected into the page executed — the policy is not enforced');
});

await check('G4 the nonce reaches app.html, so the pre-paint theme script still runs', async (page) => {
  // The cost of `script-src` having no `'unsafe-inline'`: app.html's theme script needs
  // `nonce="%sveltekit.nonce%"`. Drop that attribute and this is the check that notices —
  // the symptom otherwise is a flash of the wrong theme, which nothing else here can see.
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });
  await page.evaluate(() => localStorage.setItem('gw-theme', 'dark'));
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });
  const theme = await page.evaluate(() => document.documentElement.dataset.theme);
  assert(theme === 'dark', `expected the inline theme script to set data-theme="dark", got "${theme}"`);
});

await check('G5 the API serves its own HTML with a stricter, script-free policy', async (page) => {
  // `/auth/*` is routed to gw-api, not to SvelteKit, so SvelteKit's policy never reaches the
  // sign-in page — a password form on the public internet. `crates/gw-api/src/csp.rs` is
  // what covers it, and it can afford `default-src 'none'` because that page has no
  // JavaScript at all.
  const policy = await policyOf(page, '/auth/login');
  assert(
    policy['default-src']?.join(' ') === "'none'",
    `/auth/login: default-src is ${JSON.stringify(policy['default-src'])}, expected ["'none'"]`
  );
  assert(!('script-src' in policy), '/auth/login must not name script-src at all');
  assert(
    policy['form-action']?.join(' ') === "'self'",
    '/auth/login: the sign-in form must only be submittable to this origin'
  );
});

// ---------------------------------------------------------------------------------------
// Group H — the access panel says how far a grant reaches
// ---------------------------------------------------------------------------------------
//
// These live here rather than in AccessPanel.test.ts because the sentence that matters
// most is inside Ark's Portal, and a Portal mounts from an `$effect` — nothing in it is
// server-rendered, so `render()` from `svelte/server` cannot see it at all. Only a
// browser can answer "does the person making the grant actually read this?".
//
// The fixture is `/_behaviour/zugriff` (dev-only, static props). The real console needs
// the `admin` baseline, which this harness deliberately does not have.

/** The panel fixture, hydrated. `networkidle` for the same reason Group B needs it. */
async function loadAccessPanel(page, fall = 'geerbt') {
  const response = await page.goto(BASE + '/_behaviour/zugriff?fall=' + fall, {
    waitUntil: 'networkidle'
  });
  assert(
    response !== null && response.status() === 200,
    `expected 200 from the access-panel fixture, got ${response?.status()}`
  );
  return await response.text();
}

await check('H1 an inherited grant is named on the page as the reason, not the visibility', async (page) => {
  // The assumption this exists to correct: /handbuch/onboarding is `restricted` AND
  // reachable, because /handbuch carries a grant and `permits()` consults grants before
  // it ever looks at visibility. Asserted against the served HTML as well as the
  // rendered page, because "you can hover it and find out" is not saying it.
  const served = await loadAccessPanel(page);

  const title = 'Erreichbar über /handbuch, nicht über die Sichtbarkeit dieser Seite.';
  assert(served.includes(title), `the served HTML never says: ${title}`);

  const wegen = 'unabhängig davon, dass diese Seite als »Eingeschränkt« gekennzeichnet ist';
  assert(served.includes(wegen), `the served HTML never says: ${wegen}`);

  const eigener =
    'Ein eigener Eintrag auf /handbuch/onboarding ersetzt die geerbten Rechte vollständig.';
  assert(served.includes(eigener), `the served HTML never says: ${eigener}`);

  const notice = page.locator('.gw-adm-notice', { hasText: title });
  await notice.waitFor({ state: 'visible', timeout: 5_000 });
  assert(await notice.isVisible(), 'the explanation is in the markup but not visible');
});

await check('H2 the grant dialog states the reach of the grant before anything is granted', async (page) => {
  await loadAccessPanel(page);

  const trigger = page.getByRole('button', { name: 'Zugriff gewähren' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  // By accessible name, not by `.gw-dialog`. The panel mounts several dialogs now — grant,
  // revoke, visibility — and Ark renders each one's content into the DOM with the `hidden`
  // attribute while it is closed, so a class selector matches all of them and Playwright
  // refuses the ambiguity. The name is also what a screen-reader user is given.
  const dialog = page.getByRole('dialog', { name: 'Zugriff gewähren' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const text = await dialog.innerText();
  for (const sentence of [
    'Gilt auch für alle Seiten unter /handbuch/onboarding.',
    'Auch eine Seite mit der Sichtbarkeit »Eingeschränkt« wird dadurch erreichbar.',
    'Einen anderen Weg, einzelne Seiten auszunehmen, gibt es hier nicht.'
  ]) {
    assert(text.includes(sentence), `the open dialog never says: ${sentence}`);
  }
});

await check('H3 a path that carries its own grants is not described as inheriting them', async (page) => {
  // `Store::effective_grants` walks ancestors NEAREST FIRST and a path is its own first
  // ancestor, so `inherited_from` equals the path itself whenever that path has any
  // rows. Calling that "geerbt" sends somebody looking for a grant somewhere else.
  const served = await loadAccessPanel(page, 'eigen');
  assert(!served.includes('Geerbt von'), 'a grant written on this very path was called inherited');
  assert(
    served.includes('Entziehen'),
    'a grant written on this very path offered no revoke control'
  );
});

await check('H4 the panel names the ways in that no entry can show', async (page) => {
  // `permits()` widens `Restricted` for `Baseline::Admin` on reads, so anybody in a group
  // mapped to `admin` reads every restricted page in the corpus with no entry anywhere —
  // and no row in a table of entries will ever say so. The panel used to caption that
  // table "Wer /x erreicht" and stop there.
  const served = await loadAccessPanel(page, 'eigen');

  for (const sentence of [
    'Reichweite »Verwaltung«: liest jede Seite, ohne Eintrag.',
    'Sichtbarkeit »Eingeschränkt«: über die Sichtbarkeit kommt niemand herein.',
    'Zugriffseinträge, die auf /handbuch gelten'
  ]) {
    assert(served.includes(sentence), `the served HTML never says: ${sentence}`);
  }
  // The sentence this whole group exists to remove: true about entries, and read as
  // "nobody else gets in".
  assert(
    !served.includes('Es gilt allein die Sichtbarkeit'),
    'the panel still claims that with no entry only the visibility decides'
  );

  const summary = page.locator('.gw-adm-reach');
  await summary.waitFor({ state: 'visible', timeout: 5_000 });
  assert(await summary.isVisible(), 'the summary is in the markup but not visible');
});

await check('H5 an Anyone entry is marked as the open internet, not as another team', async (page) => {
  // `can()` answers an `Anyone` grant BEFORE it looks at whether the caller is signed in.
  // It is a public share link, and it rendered exactly like a team row.
  const served = await loadAccessPanel(page, 'freigabe');
  for (const sentence of [
    'Freigabelink: erreichbar aus dem offenen Internet.',
    'Offenes Internet'
  ]) {
    assert(served.includes(sentence), `the served HTML never says: ${sentence}`);
  }
});

await check('H6 revoking the last entry says the ancestor resumes across the subtree', async (page) => {
  // The dialog is inside Ark's Portal, so only a browser can see this at all. What it has
  // to say: removing the final row here does not close the page, it hands the page — and
  // every page below it that carries nothing of its own — back to /oberhalb.
  await loadAccessPanel(page, 'letzter');

  const trigger = page.locator('.gw-adm-trigger--danger [data-part="trigger"]');
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  const dialog = page.getByRole('dialog', { name: 'Zugriff entziehen?' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const text = await dialog.innerText();
  for (const sentence of [
    'Danach gelten hier wieder die Rechte von /oberhalb.',
    'Das ist der letzte Zugriffseintrag auf /oberhalb/unterseite.',
    'auf jeder Seite darunter, die selbst nichts eingetragen hat'
  ]) {
    assert(text.includes(sentence), `the open revoke dialog never says: ${sentence}`);
  }
});

await check('H7 the visibility dialog says what the change does before anything changes', async (page) => {
  // The control the badge always implied and never had, and the two things about it that
  // are the opposite of what people expect: it does NOT reach down the tree, and it does
  // NOT close anything an entry has opened.
  await loadAccessPanel(page, 'eigen');

  const trigger = page.getByRole('button', { name: 'Sichtbarkeit ändern' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  const dialog = page.getByRole('dialog', { name: 'Sichtbarkeit ändern' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const text = await dialog.innerText();
  for (const sentence of [
    'Gilt nur für /handbuch — und nimmt niemandem den Zugriff.',
    'Unterseiten behalten ihre eigene, anders als ein Zugriffseintrag wirkt sie nicht nach unten.',
    'sie hebt keinen Zugriffseintrag auf'
  ]) {
    assert(text.includes(sentence), `the open visibility dialog never says: ${sentence}`);
  }
});

// ---------------------------------------------------------------------------------------
// H8-H9 — I1: a picked-but-unconfirmed choice must not survive a page change
// ---------------------------------------------------------------------------------------
//
// `pickedVisibility` (and the grant dialog's `newSubject`/`newPermission`) were
// component-local `$state`, cleared only on a SUCCESSFUL change — and the admin console
// keeps AccessPanel mounted across a page change (`selectPath` calls `goto()`, not a
// fresh mount), with nothing keying the component on `path`. So a value picked on one
// page and left unconfirmed rode along onto the next. `AccessPanel.test.ts` cannot see
// this at all: it renders once, statically, via `svelte/server`, which never runs an
// `$effect` and cannot simulate a client-side navigation between two mounted states of
// the same instance — only a browser can drive that, which is why `../_behaviour/zugriff`
// now carries real `?fall=` links for these two checks to click.

await check('H8 a picked-but-unconfirmed visibility does not survive navigating to another page', async (page) => {
  await loadAccessPanel(page, 'eigen');

  let trigger = page.getByRole('button', { name: 'Sichtbarkeit ändern' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  let dialog = page.getByRole('dialog', { name: 'Sichtbarkeit ändern' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  await dialog.locator('.gw-adm-select-trigger').click();
  const option = page.locator('.gw-adm-option', { hasText: 'Öffentlich' });
  await option.waitFor({ state: 'visible', timeout: 5_000 });
  await option.click();

  const confirm = dialog.getByRole('button', { name: 'Sichtbarkeit setzen' });
  assert(await confirm.isEnabled(), 'picking "Öffentlich" never reached the option');

  // Cancel, not confirm — the choice must not survive BECAUSE it was never applied.
  await dialog.getByRole('button', { name: 'Abbrechen' }).click();
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});

  // A real link click — a client-side navigation that keeps the panel mounted, not
  // `page.goto()`, which would reload the page and could not exercise this bug even
  // with the fix reverted.
  await page.getByRole('link', { name: 'geerbt' }).click();
  await page.waitForURL(/fall=geerbt/);

  trigger = page.getByRole('button', { name: 'Sichtbarkeit ändern' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();
  dialog = page.getByRole('dialog', { name: 'Sichtbarkeit ändern' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const confirmAfter = dialog.getByRole('button', { name: 'Sichtbarkeit setzen' });
  assert(
    await confirmAfter.isDisabled(),
    'the visibility dialog on the new page opened with a stale, unconfirmed choice still live — ' +
      'the confirm button should be disabled for the unchanged value'
  );
});

await check('H9 a picked-but-ungranted subject does not survive navigating to another page', async (page) => {
  // `newSubject`/`newPermission` are the grant dialog's counterpart to `pickedVisibility`
  // — identical shape, identical missing reset.
  await loadAccessPanel(page, 'eigen');

  let trigger = page.getByRole('button', { name: 'Zugriff gewähren' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  let dialog = page.getByRole('dialog', { name: 'Zugriff gewähren' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  await dialog.locator('.gw-adm-combo-trigger').click();
  const option = page.locator('.gw-adm-option', { hasText: 'Sergej Maul' });
  await option.waitFor({ state: 'visible', timeout: 5_000 });
  await option.click();

  const confirm = dialog.getByRole('button', { name: 'Gewähren' });
  assert(await confirm.isEnabled(), 'picking a subject never reached the option');

  await dialog.getByRole('button', { name: 'Abbrechen' }).click();
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});

  await page.getByRole('link', { name: 'geerbt' }).click();
  await page.waitForURL(/fall=geerbt/);

  trigger = page.getByRole('button', { name: 'Zugriff gewähren' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();
  dialog = page.getByRole('dialog', { name: 'Zugriff gewähren' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const confirmAfter = dialog.getByRole('button', { name: 'Gewähren' });
  assert(
    await confirmAfter.isDisabled(),
    'the grant dialog on the new page opened with a stale, unconfirmed subject still chosen — ' +
      'the "Gewähren" button should be disabled with nobody picked'
  );
});

// ---------------------------------------------------------------------------------------
// H10-H11 — I3: a Select opened from inside a Dialog must be operable with the mouse
// ---------------------------------------------------------------------------------------
//
// Ark's Select/Combobox/Menu write an inline `z-index: var(--z-index)` on their
// positioner; `@zag-js/popper` populates `--z-index` from the positioner's own first
// child (`.gw-adm-listbox` / `.gw-adm-menu`), which carried no real `z-index` of its own
// — so it mirrored `auto`, and the dropdown rendered UNDER the dialog that contains it
// (root cause and fix live in admin.css, on `.gw-adm-popper`). Keyboard selection
// (ArrowDown/Enter) never touches hit-testing and would pass regardless of this bug —
// which is exactly how it shipped unnoticed. Only a real mouse click, with Playwright's
// actionability check left on (it refuses to click an element another element is
// currently painted over, and retries until it can or times out saying so), proves it.

await check('H10 the visibility select inside its dialog is operable with the mouse, not just the keyboard', async (page) => {
  await loadAccessPanel(page, 'eigen');

  const trigger = page.getByRole('button', { name: 'Sichtbarkeit ändern' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  const dialog = page.getByRole('dialog', { name: 'Sichtbarkeit ändern' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  await dialog.locator('.gw-adm-select-trigger').click();
  const option = page.locator('.gw-adm-option', { hasText: 'Öffentlich' });
  await option.waitFor({ state: 'visible', timeout: 5_000 });
  await option.click({ timeout: 5_000 });

  const confirm = dialog.getByRole('button', { name: 'Sichtbarkeit setzen' });
  assert(
    await confirm.isEnabled(),
    'clicking "Öffentlich" never reached the option — the confirm button is still disabled for the unchanged value'
  );
});

await check("H11 the grant dialog's permission select is likewise operable with the mouse", async (page) => {
  // The identical, pre-existing defect on a second Select that shares the same CSS
  // classes (`.gw-adm-popper`, `.gw-adm-listbox`) and the same nested-in-a-dialog shape.
  await loadAccessPanel(page, 'eigen');

  const trigger = page.getByRole('button', { name: 'Zugriff gewähren' });
  await trigger.waitFor({ state: 'visible' });
  await trigger.click();

  const dialog = page.getByRole('dialog', { name: 'Zugriff gewähren' });
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });

  const select = dialog.locator('.gw-adm-select-trigger');
  await select.click();
  const option = page.locator('.gw-adm-option', { hasText: 'Schreiben' });
  await option.waitFor({ state: 'visible', timeout: 5_000 });
  await option.click({ timeout: 5_000 });

  const chosen = (await select.innerText()).trim();
  assert(
    chosen.includes('Schreiben'),
    `clicking "Schreiben" never reached the option, the trigger still reads "${chosen}"`
  );
});

// ---------------------------------------------------------------------------------------
// Group I — topics: browsing by them, and saying what a page is about
//
// Against the same tour corpus, which carries four tagged pages and therefore a real
// hierarchy: `Rundgang` with `Rundgang/Tabellen` and `Rundgang/Umlaute` inside it, plus
// `Format` and `Verweise` on their own. The fixture's identity (`sergej:editors`, with write
// granted on `/rundgang`) is what makes I6 possible at all — without the grant the add would
// 403 and the check would prove nothing while still reporting a refusal correctly.
//
// What is NOT checked here is who may see which topic. ADR 0011 decides that and
// `Store::topics_for` implements it, mutation-tested; proving it end to end would need a
// tagged page this fixture's identity cannot read, and adding one means editing
// `content-example`, which several Rust tests assert against.
// ---------------------------------------------------------------------------------------

/**
 * The two tagged pages, by the addresses the seeder actually gives them — slugged from the
 * TITLE, not from the filename, so `content-example/rundgang/tabellen.md` lives here. Group C
 * names the first of these for the same reason.
 */
const TAGGED_PAGE = '/rundgang/tabellen-was-heute-passiert'; // Rundgang/Tabellen, Format
const UMLAUT_PAGE = '/rundgang/groesse-und-mass-deutsch-im-system'; // Rundgang/Umlaute

/** The nav a topic list renders into, out of a server response. */
function topicNav(html, label) {
  const opened = html.indexOf(`aria-label="${label}"`);
  if (opened === -1) return null;
  const from = html.lastIndexOf('<nav', opened);
  const to = html.indexOf('</nav>', opened);
  return from === -1 || to === -1 ? null : html.slice(from, to);
}

await check('I1 the topic index is linked from the header and nests in real markup', async (page) => {
  // `page.request` is a plain HTTP fetch: nothing renders, nothing hydrates. What comes back
  // is what a reader with JavaScript switched off receives, which is where the hierarchy has
  // to be — indentation is a fact about pixels, and a nested list is the only thing that says
  // "Tabellen is inside Rundgang" to somebody not looking at them.
  const home = await page.request.get(BASE + '/');
  assert(home.ok(), `expected 200 from /, got ${home.status()}`);
  const nav = (await home.text()).match(/<nav[^>]*aria-label="Hauptbereiche"[\s\S]*?<\/nav>/)?.[0];
  assert(nav !== undefined, 'no main navigation in the server-rendered HTML');
  assert(nav.includes('href="/themen"'), 'the main navigation does not link /themen');

  const response = await page.request.get(BASE + '/themen');
  assert(response.ok(), `expected 200 from /themen, got ${response.status()}`);
  const html = await response.text();

  const liste = topicNav(html, 'Alle Themen');
  assert(liste !== null, 'no topic list in the server-rendered /themen');
  assert(liste.includes('href="/themen/rundgang"'), 'the index does not link the Rundgang topic');
  assert(
    /<li\b[^>]*>[\s\S]*href="\/themen\/rundgang"[\s\S]*<ul[\s\S]*href="\/themen\/rundgang\/tabellen"/.test(
      liste
    ),
    'Rundgang/Tabellen is not rendered as a list INSIDE the Rundgang item — the hierarchy is only indentation'
  );
  // The one number a topic list may carry is the length of the list this reader would be
  // handed. Anything about what was left out is a fact about pages they may not read.
  assert(
    !/weitere|insgesamt|ausgeblendet|verborgen/i.test(liste),
    'the topic index hints at topics or pages it did not show'
  );
});

await check('I2 opening a topic shows what is filed under the topics inside it', async (page) => {
  // The store's decision, end to end: `/rundgang/tabellen` carries `Rundgang/Tabellen` and
  // NOT `Rundgang`, so its appearing here is the whole of "listing a topic means that topic
  // and everything inside it". Exact matching would show one page and hide two.
  const response = await page.request.get(BASE + '/themen/rundgang');
  assert(response.ok(), `expected 200 from /themen/rundgang, got ${response.status()}`);
  const html = await response.text();

  assert(
    html.includes(`href="${TAGGED_PAGE}"`),
    'a page filed under Rundgang/Tabellen is missing from the Rundgang topic'
  );
  assert(
    html.includes(`href="${UMLAUT_PAGE}"`),
    'a page filed under Rundgang/Umlaute is missing from the Rundgang topic'
  );
  assert(
    topicNav(html, 'Themen darin') !== null,
    'the topic page does not offer the topics inside it'
  );

  // A topic nobody typed and a topic you may see nothing of must answer the same way.
  const absent = await page.request.get(BASE + '/themen/gibt-es-nicht');
  assert(absent.status() === 404, `expected 404 for an unknown topic, got ${absent.status()}`);
  assert(
    !/dürfen|Berechtigung|gesperrt/i.test(await absent.text()),
    'the answer for an absent topic hints at a permission, which would tell absence and refusal apart'
  );
});

await check('I3 the sidebar switches to the topics without a single line of script', async (page) => {
  const plain = await page.request.get(BASE + '/rundgang');
  assert(plain.ok(), `expected 200 from /rundgang, got ${plain.status()}`);
  const before = await plain.text();
  assert(
    before.includes('href="/rundgang?seitenleiste=themen"'),
    'the sidebar offers no link to its topic half'
  );
  assert(
    topicNav(before, 'Themen') === null,
    'the sidebar shows the topics before they were asked for'
  );

  const asked = await page.request.get(BASE + '/rundgang?seitenleiste=themen');
  assert(asked.ok(), `expected 200 with the topics asked for, got ${asked.status()}`);
  const after = await asked.text();
  const liste = topicNav(after, 'Themen');
  assert(liste !== null, 'the sidebar does not render the topics when they are asked for');
  assert(
    liste.includes('href="/themen/rundgang?seitenleiste=themen"'),
    'a topic in the sidebar does not carry the sidebar‘s own choice, so the switcher would work exactly once'
  );
  // Which half you are on is stated, not left to a colour.
  assert(
    after.includes('aria-current="true"'),
    'the switcher does not mark which half is showing'
  );
});

await check('I4 a page says what it is about, under its title, in the first response', async (page) => {
  const response = await page.request.get(BASE + TAGGED_PAGE);
  assert(response.ok(), `expected 200 from ${TAGGED_PAGE}, got ${response.status()}`);
  const html = await response.text();

  const region = html.match(/<nav[^>]*aria-label="Themen dieser Seite"[\s\S]*?<\/nav>/)?.[0];
  assert(region !== undefined, 'no topic chips in the server-rendered HTML');
  assert(region.includes('href="/themen/format"'), 'a chip does not link its topic');
  assert(
    region.includes('Rundgang/Tabellen'),
    'a nested topic is not spelled in full on the chip, so it cannot be told from a top-level one'
  );
  // Under the title and above the document, which is the whole of the placement decision.
  const titel = html.indexOf('</h1>');
  const chips = html.indexOf('aria-label="Themen dieser Seite"');
  assert(titel !== -1 && chips > titel, 'the chips are not under the title');
  assert(chips < html.indexOf('<article'), 'the chips are not above the document');

  // A real form, not a button waiting for a bundle.
  assert(
    /<form[^>]*method="post"[^>]*action="\?\/themaHinzufuegen"/.test(region),
    'adding a topic is not a real form submission'
  );
  assert(
    /<form[^>]*action="\?\/themaEntfernen"/.test(region),
    'removing a topic is not a real form submission'
  );
});

await check('I5 the suggestions are the index, so they are filtered exactly as it is', async (page) => {
  // ADR 0011 names this as the surface that gets forgotten, because it feels like a UI
  // convenience. It cannot be forgotten here: the options ARE the array the sidebar and
  // /themen render, so this check compares the two lists rather than trusting a comment.
  const seite = await page.request.get(`${BASE}${TAGGED_PAGE}?seitenleiste=themen`);
  assert(seite.ok(), `expected 200 from ${TAGGED_PAGE}, got ${seite.status()}`);
  const html = await seite.text();

  const datalist = html.match(/<datalist[\s\S]*?<\/datalist>/)?.[0];
  assert(datalist !== undefined, 'the field offers no suggestions at all');
  const angeboten = [...datalist.matchAll(/value="([^"]*)"/g)].map((m) => m[1]);

  const liste = topicNav(html, 'Themen');
  assert(liste !== null, 'the sidebar did not render the topics to compare against');
  const gezeigt = [...liste.matchAll(/href="\/themen\/([^"?]*)/g)].map((m) => m[1]);

  assert(angeboten.length > 0, 'the suggestion list is empty on a wiki that has topics');
  assert(
    angeboten.length === gezeigt.length,
    `the suggestions and the index disagree: ${angeboten.length} offered, ${gezeigt.length} shown`
  );
  assert(
    angeboten.every((value) => !value.startsWith('/')),
    'a suggestion is spelled as a canonical path, which the API would refuse'
  );
});

await check('I6 a topic can be filed and un-filed from the page you are reading', async (page) => {
  // The fixture grants `group:editors` write on /rundgang, and the dev identity is in that
  // group — so this exercises the real write path rather than a refusal. It cleans up after
  // itself, and the topic is pruned once no page carries it.
  const NEU = 'Verhaltensprobe';
  await page.goto(BASE + '/rundgang', { waitUntil: 'domcontentloaded' });

  const feld = page.locator('#thema');
  await feld.waitFor({ state: 'visible', timeout: 10_000 });
  await feld.fill(NEU);
  await page.getByRole('button', { name: 'Hinzufügen' }).click();

  const chip = page.getByRole('link', { name: NEU, exact: true });
  await chip.waitFor({ state: 'visible', timeout: 10_000 });
  assert(
    page.url().endsWith('#gw-themen'),
    `a finished change should come back to the topics region, landed on ${page.url()}`
  );

  // The chip is a way in: following it reaches that topic's own page, which is the only way
  // a topic is reachable at all.
  await chip.click();
  await page.waitForURL(/\/themen\/verhaltensprobe/, { timeout: 10_000 });
  assert(
    (await page.locator('h1').innerText()).includes(NEU),
    'following a chip did not reach that topic'
  );

  await page.goto(BASE + '/rundgang', { waitUntil: 'domcontentloaded' });
  const weg = page.getByRole('button', { name: `Thema »${NEU}« entfernen` });
  await weg.waitFor({ state: 'visible', timeout: 10_000 });
  await weg.click();
  await page
    .getByRole('link', { name: NEU, exact: true })
    .waitFor({ state: 'detached', timeout: 10_000 });

  // Pruned, not merely unlinked: a topic no page carries stops existing (ADR 0011).
  const nachher = await page.request.get(BASE + '/themen/verhaltensprobe');
  assert(
    nachher.status() === 404,
    `a topic no page carries should be gone; /themen/verhaltensprobe answered ${nachher.status()}`
  );
});

// ---------------------------------------------------------------------------------------
// Group J — the Papierkorb: deleting where the page is, recovering where you can find it,
// and the one act in this system that loses data
//
// The fixture's identity (`sergej:editors`) holds **write** on `/rundgang` and
// `/verweisbeispiel`, and **admin** on exactly one page, `/verweisbeispiel/verweist-zurueck`.
// That split is what makes both halves of the purge checkable end to end: J5 watches the gate
// refuse where there is only write, and J6 watches it allow where there is admin. Without the
// admin grant J6 could not run at all and J5 would report "ok" for having correctly detected a
// refusal — proving nothing, which is precisely the failure `just behaviour-fixture`'s own
// comments were written about.
//
// **J6 really destroys a page**, which is why it is last. `behaviour-fixture` deletes and
// reseeds the database on every run, so the corpus is whole again next time; nothing after
// this line may depend on `/verweisbeispiel/verweist-zurueck` existing.
//
// What is NOT checked here is who may see which entry. `Store::trash_for` decides that, per
// document, and it is mutation-tested; proving it end to end would need a deleted page this
// identity cannot read, and putting one there means editing `content-example`, which several
// Rust tests assert against.
// ---------------------------------------------------------------------------------------

/** Write, and deliberately no admin: the page J3, J4 and J5 borrow and put back. */
const TRASH_PAGE = '/rundgang/was-schon-geht';
/** The one page this identity administers, and the one J6 destroys. */
const PURGE_PAGE = '/verweisbeispiel/verweist-zurueck';

/**
 * Submit a form by its own submit control and wait for the navigation it causes.
 *
 * The address is what is waited on, not a load state: these are native form submissions, so
 * the page is already `domcontentloaded` when the click happens and `waitForLoadState`
 * resolves instantly — the assertion after it then reads the URL of the page that has not
 * navigated yet. That failure looked exactly like a broken form, and was not one.
 */
async function submit(page, name, exact = false) {
  // `exact` because a file input is a BUTTON in the accessibility tree and takes its name from
  // its own label, so a substring match can resolve to two controls on one form. Group K found
  // that; nothing in Group J needs it, and the default keeps those calls unchanged.
  const button = page.getByRole('button', { name, exact });
  await button.waitFor({ state: 'visible', timeout: 10_000 });
  const before = page.url();
  await button.click();
  await page.waitForFunction((was) => location.href !== was, before, { timeout: 10_000 });
  await page.waitForLoadState('domcontentloaded');
}

/** Put a page back, so a check that borrowed one leaves the corpus as it found it. */
async function restore(page, path) {
  await page.goto(BASE + '/papierkorb', { waitUntil: 'domcontentloaded' });
  const row = page.locator(`tr[data-eintrag="${path}"]`);
  if ((await row.count()) === 0) return;
  await Promise.all([
    page.waitForLoadState('domcontentloaded'),
    row.getByRole('button').first().click()
  ]);
}

await check('J1 the Papierkorb is a place in the header, and counts nothing it did not show', async (page) => {
  const home = await page.request.get(BASE + '/');
  assert(home.ok(), `expected 200 from /, got ${home.status()}`);
  const nav = (await home.text()).match(/<nav[^>]*aria-label="Hauptbereiche"[\s\S]*?<\/nav>/)?.[0];
  assert(nav !== undefined, 'no main navigation in the server-rendered HTML');
  assert(nav.includes('href="/papierkorb"'), 'the main navigation does not link the Papierkorb');

  const response = await page.request.get(BASE + '/papierkorb');
  assert(response.ok(), `expected 200 from /papierkorb, got ${response.status()}`);
  const html = await response.text();
  assert(/<h1[^>]*>Papierkorb<\/h1>/.test(html), '/papierkorb does not name itself');
  // The listing is filtered per document. The one number it may carry is the size of an
  // entry this reader may see; anything about what was left out is a fact about pages they
  // may not read.
  assert(
    !/weitere|insgesamt|ausgeblendet|verborgen/i.test(html),
    'the Papierkorb hints at entries or pages it did not show'
  );
});

await check('J2 deleting is offered on the page, as a link that asks before it does anything', async (page) => {
  const response = await page.request.get(BASE + TRASH_PAGE);
  assert(response.ok(), `expected 200 from ${TRASH_PAGE}, got ${response.status()}`);
  const html = await response.text();

  const bar = html.match(/<p class="editbar[\s\S]*?<\/p>/)?.[0];
  assert(bar !== undefined, 'no control bar in the server-rendered HTML');
  assert(bar.includes('Bearbeiten') && bar.includes('Verlauf'), 'the control bar lost its own controls');
  assert(
    bar.includes(`href="${TRASH_PAGE}?loeschen=1#gw-loeschen"`),
    'deleting is not offered beside Bearbeiten and Verlauf, as a real link'
  );
  // Nothing on the reading page can delete anything by being clicked.
  assert(!html.includes('?/loeschen'), 'the reading page carries a control that deletes on click');

  const asked = await page.request.get(`${BASE}${TRASH_PAGE}?loeschen=1`);
  const question = await asked.text();
  assert(
    /<form[^>]*method="post"[^>]*action="\?\/loeschen"/.test(question),
    'the question before a delete is not a real form submission'
  );
  // The one thing nobody would guess, said before it happens rather than discovered after.
  assert(
    /darunter/.test(question),
    'the question does not say that the pages under this one go with it'
  );
  assert(question.includes('Abbrechen'), 'the question offers no way out');
});

await check('J3 a page really goes to the Papierkorb and really comes back', async (page) => {
  await page.goto(`${BASE}${TRASH_PAGE}?loeschen=1`, { waitUntil: 'domcontentloaded' });
  await submit(page, 'In den Papierkorb');

  assert(page.url().includes('/papierkorb'), `a finished delete should land in the Papierkorb, landed on ${page.url()}`);
  const row = page.locator(`tr[data-eintrag="${TRASH_PAGE}"]`);
  await row.waitFor({ state: 'visible', timeout: 10_000 });

  // The page is genuinely out of the wiki, not merely hidden from a list.
  const gone = await page.request.get(BASE + TRASH_PAGE);
  assert(gone.status() === 404, `a deleted page should be gone; ${TRASH_PAGE} answered ${gone.status()}`);
  const tree = await page.request.get(BASE + '/rundgang');
  assert(
    !(await tree.text()).includes(`href="${TRASH_PAGE}"`),
    'a deleted page is still linked from the navigation'
  );

  await Promise.all([page.waitForLoadState('domcontentloaded'), row.getByRole('button').first().click()]);
  await page.locator(`tr[data-eintrag="${TRASH_PAGE}"]`).waitFor({ state: 'detached', timeout: 10_000 });

  const back = await page.request.get(BASE + TRASH_PAGE);
  assert(back.ok(), `a restored page should answer again; ${TRASH_PAGE} answered ${back.status()}`);
});

await check('J4 a subtree that is not all yours is refused, and the refusal says what is in the way', async (page) => {
  // `/rundgang/nur-intern` carries its own grant for a group this identity is not in, so the
  // whole of `/rundgang` cannot go to the trash — a page goes with everything under it, and
  // that page is not this caller's to move.
  await page.goto(BASE + '/rundgang?loeschen=1', { waitUntil: 'domcontentloaded' });
  await submit(page, 'In den Papierkorb');

  const alarm = page.locator('[role="alert"]');
  await alarm.first().waitFor({ state: 'visible', timeout: 10_000 });
  const said = await alarm.first().innerText();
  assert(/Unterseite/.test(said), `the refusal must say what is in the way: ${said}`);
  assert(/nichts gelöscht/.test(said), `the refusal must promise that nothing moved: ${said}`);

  const still = await page.request.get(BASE + '/rundgang');
  assert(still.ok(), `nothing should have moved; /rundgang answered ${still.status()}`);
  const intern = await page.request.get(BASE + '/papierkorb');
  assert(
    !(await intern.text()).includes('data-eintrag="/rundgang"'),
    'a refused delete still put the page in the Papierkorb'
  );
});

await check('J5 no report, no control: the gate that refuses a purge refuses to describe one', async (page) => {
  // Write on this page, and no admin. ADR 0012 makes those different permissions on purpose.
  await page.goto(`${BASE}${TRASH_PAGE}?loeschen=1`, { waitUntil: 'domcontentloaded' });
  await submit(page, 'In den Papierkorb');

  const frage = page.locator(`tr[data-eintrag="${TRASH_PAGE}"] a`).first();
  await frage.waitFor({ state: 'visible', timeout: 10_000 });
  await Promise.all([page.waitForLoadState('domcontentloaded'), frage.click()]);

  const said = await page.locator('[role="alert"]').first().innerText();
  assert(/verwalt/i.test(said), `the refusal must say who may ask: ${said}`);
  // The whole point: the control that destroys is not drawn at all, rather than drawn and
  // refused on press.
  assert(
    (await page.getByRole('button', { name: 'Endgültig löschen' }).count()) === 0,
    'a purge control was offered to somebody the API had already refused'
  );

  await restore(page, TRASH_PAGE);
  const back = await page.request.get(BASE + TRASH_PAGE);
  assert(back.ok(), `the borrowed page should be back; ${TRASH_PAGE} answered ${back.status()}`);
});

await check('J6 a purge names every page it is about to destroy, and then destroys them', async (page) => {
  // LAST, and destructive: this really purges `/verweisbeispiel/verweist-zurueck`. The
  // fixture is rebuilt from `content-example` on every run, so nothing below may depend on
  // that page existing.
  const titel = 'Verweist zurück';
  await page.goto(`${BASE}${PURGE_PAGE}?loeschen=1`, { waitUntil: 'domcontentloaded' });
  await submit(page, 'In den Papierkorb');

  const frage = page.locator(`tr[data-eintrag="${PURGE_PAGE}"] a`).first();
  await frage.waitFor({ state: 'visible', timeout: 10_000 });
  const href = await frage.getAttribute('href');
  assert(
    href !== null && href.endsWith('#gw-endgueltig'),
    `the link to the question must carry the fragment that announces it, got ${href}`
  );

  // Followed as a reader would follow it, in a hydrated page — which is the case that was
  // wrong. The fragment moves focus only on a real page load, so the client-side router left
  // the question drawn and unannounced; `data-sveltekit-reload` on the link is what makes the
  // hydrated path behave as the scriptless one already did. This assertion is the only thing
  // that can tell the two apart, and it read the body before the attribute was there.
  await Promise.all([page.waitForURL(/entfernen=/, { timeout: 10_000 }), frage.click()]);
  await page.waitForFunction(() => document.readyState === 'complete', { timeout: 10_000 });
  const focused = await page.evaluate(() => document.activeElement?.id ?? '');
  assert(focused === 'gw-endgueltig', `the confirmation should take focus, got "${focused}"`);

  const confirm = page.locator('#gw-endgueltig');
  const text = await confirm.innerText();
  assert(/nicht rückgängig/i.test(text), `the confirmation must say it cannot be undone: ${text}`);
  // By NAME, from the report the API produced by running the purge and rolling it back —
  // never summarised into "diese Seite und N weitere".
  assert(text.includes(titel), 'the confirmation does not name the page it would destroy');
  assert(text.includes(PURGE_PAGE), 'the confirmation does not give the address of what goes');
  assert(!/und \d+ weitere/i.test(text), 'the confirmation summarised the names it was given');
  // And by count, every kind, including the kinds that are none.
  for (const was of ['Versionen', 'Karten', 'Projekte', 'Verweise', 'Themen']) {
    assert(text.includes(was), `the confirmation does not say what happens to ${was}`);
  }
  // At least the six kinds this interface has wording for. Not exactly six: the report is
  // walked rather than spelled out field by field, precisely so that a count the API grows
  // later appears here instead of being silently dropped, and pinning the number would turn
  // that safeguard into a failing check the day it did its job.
  const zahlen = await confirm.locator('dd').allInnerTexts();
  assert(zahlen.length >= 6, `expected at least six counted kinds, found ${zahlen.length}`);
  assert(
    zahlen.every((zahl) => /^\d+$/.test(zahl.trim())),
    `every counted kind must carry a number: ${zahlen.join(', ')}`
  );

  await submit(page, 'Endgültig löschen');

  const meldung = await page.locator('[role="status"]').first().innerText();
  assert(/endgültig/i.test(meldung), `the outcome must say what happened: ${meldung}`);
  assert(
    (await page.locator(`tr[data-eintrag="${PURGE_PAGE}"]`).count()) === 0,
    'a purged entry is still in the Papierkorb'
  );
  const weg = await page.request.get(BASE + PURGE_PAGE);
  assert(weg.status() === 404, `a purged page should be gone; it answered ${weg.status()}`);
});

// ---------------------------------------------------------------------------------------
// Group K — attachments: what a page carries besides its words, and how it gets there
//
// Against `/rundgang`, where the fixture's identity (`sergej:editors`) holds **write** — the
// same grant Group I's tagging check needs, and for the same reason: without it the upload
// would 403 and this group would prove nothing while still reporting "ok" for having correctly
// detected a refusal. `/start-hier` is the other half: no grant at all, so it is readable at
// the public baseline and not writable, which is what K5 checks the withheld control against.
//
// **These checks really attach files**, and nothing detaches them — there is no detach control
// in this interface yet (see the note in `PageAttachments.svelte`). `behaviour-fixture` deletes
// and reseeds the database on every run, so the rows go with it; the bytes stay on the media
// mount, which is exactly what ADR 0013 says happens and is bounded by the distinct files ever
// uploaded. Two small probes.
//
// **K6 and K7 are the inline placement** — D-15's other half, and they are deliberately the
// end of this group rather than a group of their own: they need a file to be attached, and K2
// is what attaches one. K6 places it through the editor, publishes, and reads the page back
// with no script running; K7 then detaches the file through the API and checks the prose is
// untouched, which is the consequence D-15 states and the one nothing else can prove end to
// end. K6 is also the only check anywhere that exercises the CRDT deletion path in a real
// browser: `extensions.test.ts` drives `@tiptap/y-tiptap` directly and is the finer
// instrument, but it cannot see a page that survives being edited, published and re-read.
//
// What is also not checked is who may see which file. `Store::attachments_for` decides that,
// per document, through the same body a page read ends in, and it is tested there — proving it
// end to end would need an attachment on a page this identity cannot read, and putting one
// there means a write this identity cannot make.
// ---------------------------------------------------------------------------------------

/** Write granted: the page Group K attaches to. */
const ATTACH_PAGE = '/rundgang';
/** No grant at all: readable at the public baseline, and not writable. */
const READONLY_PAGE = '/start-hier';

/**
 * A real 1x1 red PNG, 69 bytes: signature, IHDR, IDAT, IEND.
 *
 * `gw_store::blobs::sniff` compares byte prefixes and runs no parser, so eight bytes of
 * signature and some padding would be a PNG to the wiki — and that is what this was. It is a
 * decodable image now because K6 asserts the browser really DISPLAYS it, which needs the
 * bytes to survive the whole path and then be a picture at the end of it: an `<img>` whose
 * source the Content-Security-Policy refused, or whose address was wrong, looks exactly like
 * one whose bytes will not decode, and `naturalWidth` is the only thing that tells the three
 * apart from inside the page.
 */
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC',
  'base64'
);

/** Bytes that are no known format and are not text either — 0xFF is not valid UTF-8. */
const NONSENSE = Buffer.from([0x00, 0x01, 0x02, 0xff, 0xfe, 0x00, 0x03]);

/** Choose a file for the one file field on the page and send it. */
async function attach(page, name, mimeType, buffer) {
  const feld = page.locator('#gw-anhang-datei');
  await feld.waitFor({ state: 'visible', timeout: 10_000 });
  await feld.setInputFiles({ name, mimeType, buffer });
  await submit(page, 'Hochladen', true);
}

await check('K1 the Anhänge list and its upload are in the first response, before any script', async (page) => {
  // A plain HTTP fetch: nothing renders and nothing hydrates. What comes back is what a reader
  // with JavaScript switched off receives — which is where the list and the control have to
  // be, because a control that only works once a bundle arrives is one that looks live and
  // does nothing.
  const response = await page.request.get(BASE + ATTACH_PAGE);
  assert(response.ok(), `expected 200 from ${ATTACH_PAGE}, got ${response.status()}`);
  const html = await response.text();

  const region = html.match(/<section[^>]*id="gw-anhaenge"[\s\S]*?<\/section>/)?.[0];
  assert(region !== undefined, 'no Anhänge section in the server-rendered HTML');
  assert(/Anhänge/.test(region), 'the section does not name itself');
  // Below the document: the list is a fact ABOUT the page, wanted once you have read it. The
  // topics are the opposite case and sit under the title.
  assert(
    html.indexOf('id="gw-anhaenge"') > html.indexOf('<article'),
    'the Anhänge section is not below the document'
  );

  // A real multipart form submission, not a click handler.
  assert(
    /<form[^>]*method="post"[^>]*action="\?\/anhaengen"/.test(region),
    'attaching a file is not a real form submission'
  );
  assert(
    region.includes('enctype="multipart/form-data"'),
    'the upload form would not carry a file at all'
  );
  // Labelled, so the field can be reached and named rather than being a bare button.
  assert(
    /<label[^>]*for="gw-anhang-datei"/.test(region) &&
      /<input[^>]*id="gw-anhang-datei"[^>]*type="file"/.test(region),
    'the file field is not properly labelled'
  );
  // And it states no list of its own: `gw_store::blobs::sniff` owns the accepted set and it is
  // being widened. An `accept` attribute here would refuse a file the wiki would have taken,
  // before the request was made and with nothing in any log to say why.
  assert(
    !/accept=/.test(region),
    'the upload field carries its own list of file types, which is a second answer that goes stale'
  );
});

await check('K2 a file really attaches, and the page says what it is, how big, and who put it there', async (page) => {
  const NAME = 'probe-bild.png';
  await page.goto(BASE + ATTACH_PAGE, { waitUntil: 'domcontentloaded' });

  // The declared type is deliberately WRONG. `gw_store::blobs::sniff` reads the bytes, and
  // there is nowhere in the request for an uploader-chosen type to travel — so what the page
  // shows afterwards must be `image/png`, whatever this says.
  await attach(page, NAME, 'application/pdf', PNG);

  assert(
    page.url().endsWith('#gw-anhaenge'),
    `a finished upload should come back to the Anhänge section, landed on ${page.url()}`
  );
  // Announced, not merely drawn: the fragment moves focus to the section on a real navigation,
  // and a region that has just received focus is read out. Native form submission, no script.
  await page.waitForFunction(() => document.readyState === 'complete', { timeout: 10_000 });
  const focused = await page.evaluate(() => document.activeElement?.id ?? '');
  assert(focused === 'gw-anhaenge', `the Anhänge section should take focus, got "${focused}"`);

  const region = page.locator('#gw-anhaenge');
  const text = await region.innerText();
  assert(text.includes(NAME), `the list does not name the file that was just attached: ${text}`);
  assert(/ist jetzt angehängt/.test(text), `the arrival is not stated: ${text}`);
  // The type as the BYTES say it is, never as the upload claimed.
  assert(text.includes('image/png'), `the list does not say what the bytes are: ${text}`);
  assert(!text.includes('application/pdf'), `the list echoed the type the upload declared: ${text}`);
  // Size and type in words, not an icon: what somebody needs before fetching it.
  assert(/\bBild\b/.test(text), `the kind of file is not stated in German: ${text}`);
  assert(/\d+(,\d)?\s?(B|kB|MB|GB)\b/.test(text), `the size is not stated: ${text}`);
  assert(/Hochgeladen von/.test(text), `who attached it is not stated: ${text}`);

  const status = await page.locator('#gw-anhaenge [role="status"]').first().innerText();
  assert(status.includes(NAME), `the announcement does not name the file: ${status}`);

  // And it is really attached, not merely drawn once: a fresh, scriptless request says so.
  const wieder = await page.request.get(BASE + ATTACH_PAGE);
  assert(
    (await wieder.text()).includes(NAME),
    'the file is gone from the page it was just attached to'
  );
});

await check('K3 the download is a link that serves the bytes, addressed by page and never by hash', async (page) => {
  // Uses the file K2 attached. The address is the API's own — D-16 makes a download authorised
  // against the page it was reached through, which is only true while the page is in the
  // address and the bytes are not.
  const html = await (await page.request.get(BASE + ATTACH_PAGE)).text();
  const region = html.match(/<section[^>]*id="gw-anhaenge"[\s\S]*?<\/section>/)?.[0];
  assert(region !== undefined, 'no Anhänge section to take a download address from');
  const href = region.match(/<a[^>]*href="(\/api\/attachment\/[^"]+)"/)?.[1];
  assert(href !== undefined, 'the attachment is not offered as a link at all');

  // The page is in the address; the content address is nowhere, on the page or in the link.
  assert(href.includes(ATTACH_PAGE), `the download address does not name the page: ${href}`);
  assert(!/[0-9a-f]{40,}/.test(href), `the download address carries a digest: ${href}`);
  assert(
    !/[0-9a-f]{64}/.test(region) && !/sha256/i.test(region),
    'the rendered page carries a content address'
  );

  // A link, so a right-click saves it and hydration is irrelevant — and it really serves.
  const datei = await page.request.get(BASE + href);
  assert(datei.ok(), `the download should serve the file, got ${datei.status()}`);
  assert(
    datei.headers()['content-type'] === 'image/png',
    `the download should be typed by its bytes, got ${datei.headers()['content-type']}`
  );
  assert(
    datei.headers()['x-content-type-options'] === 'nosniff',
    'the download lets the browser decide the bytes are something else'
  );
  const bytes = await datei.body();
  assert(bytes.length === PNG.length, `expected ${PNG.length} bytes back, got ${bytes.length}`);
});

await check('K4 a file this wiki will not store is refused in the reader s own words', async (page) => {
  // Bytes that are no known format and are not text either. WHICH types are accepted is the
  // server's answer and is being widened, so this check asserts the SHAPE of the refusal —
  // German framing carrying the server's own sentence — and never the sentence itself.
  const NAME = 'probe-unbekannt.bin';
  await page.goto(BASE + ATTACH_PAGE, { waitUntil: 'domcontentloaded' });
  await attach(page, NAME, 'application/octet-stream', NONSENSE);

  const alarm = page.locator('#gw-anhaenge [role="alert"]').first();
  await alarm.waitFor({ state: 'visible', timeout: 10_000 });
  const said = await alarm.innerText();
  assert(/nichts angehängt/.test(said), `the refusal must promise that nothing was attached: ${said}`);
  assert(
    /Der Server nennt den Grund:/.test(said),
    `the refusal must carry what the server said rather than a bare status: ${said}`
  );
  assert(!/^Fehler/.test(said.trim()), `a bare status code is not an explanation: ${said}`);

  // The refusal removed nothing and attached nothing.
  const html = await (await page.request.get(BASE + ATTACH_PAGE)).text();
  assert(!html.includes(NAME), 'a refused upload was attached anyway');
  assert(html.includes('probe-bild.png'), 'a refused upload took the existing list with it');
});

/** The description K6 types into the editor's prompt, and reads back off the page. */
const PLACED_ALT = 'Röntgenbild, seitlich';

await check('K5 a page this reader may not write offers no way to attach anything', async (page) => {
  // No grant on this path, so `group:editors` reaches it at the public baseline: readable, not
  // writable. `may_write` comes off the same authorisation that produced the list (ADR 0010),
  // so a control withheld on it is one the API would have refused — never a guess.
  const response = await page.request.get(BASE + READONLY_PAGE);
  assert(response.ok(), `expected 200 from ${READONLY_PAGE}, got ${response.status()}`);
  const html = await response.text();

  assert(!html.includes('?/anhaengen'), 'an upload control was offered on a page this reader may not write');
  assert(
    !/id="gw-anhang-datei"/.test(html),
    'a file field was drawn on a page this reader may not write'
  );
});

await check('K6 a file placed in the editor survives the CRDT, the publish and the reload', async (page) => {
  // The end-to-end proof of D-15's other half, and of the mirror that destroys data. It uses
  // the file K2 attached, places it with the editor's own control, publishes, and then reads
  // the page back with `page.request` — a plain fetch, nothing hydrated — so what is asserted
  // is what a reader with JavaScript switched off receives.
  //
  // Every silent failure this feature can have is on this path. A block kind the editor's
  // schema does not name is DELETED from the Y.Doc by `createNodeFromYElement`'s catch and the
  // deletion is published; an attribute the schema does not declare is removed by
  // `updateYFragment` on the first edit that touches the node. Neither throws and neither is
  // logged: the page simply comes back one picture shorter, or with a picture that no longer
  // says which file it is.
  const NAME = 'probe-bild.png';
  await page.goto(BASE + ATTACH_PAGE + '?edit=1', { waitUntil: 'networkidle' });
  const region = page.locator('section[aria-label="Seite bearbeiten"]');
  await region.waitFor({ state: 'visible', timeout: 10_000 });

  const head = region.locator('.gw-ed-status-head');
  const settled = await until(
    async () => {
      const saw = (await head.textContent())?.trim() ?? '';
      return { ok: saw.length > 0 && !saw.includes('wird geöffnet'), saw };
    },
    'the editing session never settled into an answer',
    10_000
  );
  // A check that cannot run must fail rather than exit quietly — Group E's own comment
  // records what months of that cost.
  assert(
    settled.saw.includes('Verbunden'),
    `K6 needs a live editing session on ${ATTACH_PAGE} and there isn't one — headline says ` +
      `"${settled.saw}". Is the behaviour fixture's write grant missing?`
  );

  // The control IS the list: placing a file means choosing one that is attached, never typing
  // a name — so a reference to a file that is not there cannot be written by accident.
  const button = region.getByRole('button', { name: NAME });
  await button.waitFor({ state: 'visible', timeout: 10_000 });

  // The description is asked for with a prompt, exactly as the Link control asks for an
  // address. Playwright dismisses a dialog nobody handles, so this has to be armed first.
  page.once('dialog', (dialog) => dialog.accept(PLACED_ALT));

  // The document is emptied first, so the placement is the whole of it and the assertions
  // below cannot pass on prose that happened to be there already. `Mod-a` and `Backspace` are
  // both ProseMirror commands acting on `state.selection` inside one synchronous keydown —
  // see E7 for why a selection built by moving the native caret is not safe here.
  const surface = region.locator('[contenteditable="true"]');
  await surface.click();
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Backspace');
  await button.click();

  // In the editor's own DOM first: if the schema could not build the node, it is already gone
  // here, before anything is published.
  const placed = region.locator('figure[data-attachment]');
  await placed.waitFor({ state: 'visible', timeout: 10_000 });
  assert(
    (await placed.getAttribute('data-filename')) === NAME,
    'the placed block does not name the file it was made from'
  );

  const publish = region.getByRole('button', { name: 'Veröffentlichen' });
  await publish.click();
  await until(
    async () => {
      const saw = (await region.locator('.gw-ed-note').first().textContent())?.trim() ?? '';
      return { ok: /gespeichert|veröffentlicht/i.test(saw), saw };
    },
    'the editor never confirmed the publish',
    15_000
  );

  // And now the reader's side, with nothing hydrated: the picture is in the first response, at
  // the address the API built, and the description that was typed is its alt text.
  const html = await (await page.request.get(BASE + ATTACH_PAGE)).text();
  const article = html.match(/<article[^>]*class="prose[\s\S]*?<\/article>/)?.[0] ?? '';
  assert(article !== '', 'the page came back without a document at all');
  assert(
    article.includes(`src="/api/attachment/${NAME}${ATTACH_PAGE}"`),
    `the picture is not in the document, or not at the API's own address: ${article}`
  );
  assert(article.includes(`alt="${PLACED_ALT}"`), `the description did not survive: ${article}`);
  // The address names the page and never the bytes (D-16), on the reading path as much as in
  // the `Anhänge` list K3 checks.
  assert(!/[0-9a-f]{40,}/.test(article), `the document carries a content address: ${article}`);
  // An SVG would have to render this way too, so nothing here may ever become a mechanism that
  // executes what it renders. `BlockView.test.ts` asserts that on an SVG specifically; this is
  // the same rule stated where a real browser can see it.
  for (const forbidden of ['<object', '<embed', '<iframe']) {
    assert(
      !article.toLowerCase().includes(forbidden),
      `the document renders a file through ${forbidden}`
    );
  }

  // And it is really ON SCREEN, which the markup alone cannot say. Three different failures
  // produce an `<img>` that renders nothing and none of them appears in the HTML: the
  // Content-Security-Policy refusing the address (`img-src` is `'self' data:` and this is a
  // same-origin `/api/…` path — Group G is where the policy itself is checked, and this is
  // the one place anything asserts that a picture survives it), a download the API refuses,
  // and bytes that are not a picture. `naturalWidth` is non-zero only when the browser
  // fetched it AND decoded it, so it answers all three at once.
  await page.goto(BASE + ATTACH_PAGE, { waitUntil: 'networkidle' });
  const bild = page.locator(`article.prose img[src="/api/attachment/${NAME}${ATTACH_PAGE}"]`);
  await bild.waitFor({ state: 'visible', timeout: 10_000 });
  const width = await bild.evaluate((el) => el.naturalWidth);
  assert(
    width > 0,
    'the picture is in the markup and the browser did not display it — the policy refused ' +
      'the address, the download failed, or the bytes are not an image'
  );
});

await check('K7 detaching a file leaves the prose exactly as it was, and the page says so', async (page) => {
  // D-15's consequence, and the half that is easy to get backwards: the LIST is the authority
  // on what is attached, and a block in the body is a reference to it. So taking the file away
  // must not touch the document — and the page must then say the file is missing rather than
  // drawing a broken picture, which reads as a network fault and sends whoever investigates to
  // the wrong place.
  //
  // Through the API, because there is no detach control in this interface yet. That is the one
  // thing in this group that reaches past the browser's own controls, and it is what lets the
  // check exist at all.
  const NAME = 'probe-bild.png';
  const before = await (await page.request.get(BASE + ATTACH_PAGE)).text();
  assert(
    before.includes(`src="/api/attachment/${NAME}${ATTACH_PAGE}"`),
    'K7 needs the placement K6 published, and the page does not have one'
  );

  const gone = await page.request.delete(`${BASE}/api/attachment/${NAME}${ATTACH_PAGE}`);
  assert(gone.ok(), `the detach should have worked, got ${gone.status()}`);

  const html = await (await page.request.get(BASE + ATTACH_PAGE)).text();
  const article = html.match(/<article[^>]*class="prose[\s\S]*?<\/article>/)?.[0] ?? '';
  // The block is still there — it is still the page's own prose — and it now names the file it
  // cannot find instead of pointing at an address that would answer 404.
  assert(article.includes(NAME), `the reference to the file was destroyed with it: ${article}`);
  assert(
    !article.includes(`src="/api/attachment/${NAME}`),
    `the page still offers an address for a file it no longer carries: ${article}`
  );
  assert(
    /entfernt|hochgeladen/.test(article),
    `the page does not say why the file is missing: ${article}`
  );
  // And the list agrees, because the list is what was actually changed.
  const region = html.match(/<section[^>]*id="gw-anhaenge"[\s\S]*?<\/section>/)?.[0] ?? '';
  assert(!region.includes(NAME), 'the Anhänge list still shows a file that was detached');
});

// ---------------------------------------------------------------------------------------
// Group L — a diagram is a picture, and it is the one the reader's theme calls for
//
// The step this group exists for could not be checked anywhere else. Mermaid needs the DOM to
// measure text, so it never runs on the server and no `svelte/server` render in `vitest` can
// see a drawn diagram at all — every one of those tests can only assert the fence's own source.
// So this is the only place that answers whether the thing actually draws.
//
// Against `/rundgang/was-schon-geht`, which `content-example` seeds with one ```mermaid fence.
// It needs no grant: these checks only read.
//
// ONE HONEST LIMITATION, because it is the shape of failure this feature is most likely to
// have. This harness drives `npm run dev`, and SvelteKit adds `'unsafe-inline'` to `style-src`
// in development (see `web/src/lib/csp.ts`), so the CSS-injection barrier ADR 0018 relies on is
// NOT in force here. A green run says the diagram draws and that no console error was raised;
// it does not say the production policy admits it. That is verified against a production build
// by hand, and ADR 0018 says so.
const DIAGRAM_PAGE = '/rundgang/was-schon-geht';

await check('L1 a diagram is text in the first response and a picture after it', async (page) => {
  // The half a reader with JavaScript switched off keeps. It also pins the other direction:
  // the server must NOT have drawn anything, because mermaid cannot run there — a data URI in
  // the first response would mean the library had reached the server bundle, which is the
  // failure `web/scripts/check-server-bundle.sh` exists to catch at build time.
  const first = await (await page.request.get(BASE + DIAGRAM_PAGE)).text();
  // The PROSE, not the whole document: `app.html` carries a `data:image/svg+xml` favicon, and
  // asserting against the page as a whole made this fail for a reason that has nothing to do
  // with diagrams — which is the same trap K7 avoids by cutting the article out first.
  const artikel = first.match(/<article[^>]*class="prose[\s\S]*?<\/article>/)?.[0] ?? '';
  assert(artikel.includes('graph TD;'), 'the diagram source is not in the first response');
  assert(
    !artikel.includes('data:image/svg+xml'),
    'the server drew a diagram — mermaid must never be on the server rendering path'
  );

  const klagen = [];
  page.on('console', (message) => {
    if (message.type() === 'error') klagen.push(message.text());
  });
  page.on('pageerror', (err) => klagen.push(String(err)));

  await page.goto(BASE + DIAGRAM_PAGE, { waitUntil: 'networkidle' });
  const bilder = page.locator('article.prose img[src^="data:image/svg+xml"]');
  // Generous: mermaid is fetched only when a diagram is on screen, and in DEV that is a great
  // many separate module requests.
  await bilder.first().waitFor({ state: 'visible', timeout: 60_000 });

  const anzahl = await bilder.count();
  assert(anzahl === 2, `D-24 draws once per theme, so there should be two images, not ${anzahl}`);

  // `naturalWidth` is non-zero only when the browser accepted the address AND decoded the
  // bytes, so it answers the policy question and the "is it really an SVG" question at once.
  const breite = await bilder.first().evaluate((el) => el.naturalWidth);
  assert(breite > 0, 'the picture is in the markup and the browser did not display it');

  // The two really are two renders. Identical sources would mean one theme was drawn twice,
  // which is the failure D-24 exists to prevent and which looks fine until somebody switches
  // the site to dark.
  const beide = await bilder.evaluateAll((els) => els.map((el) => el.getAttribute('src')));
  assert(beide[0] !== beide[1], 'both images are the same drawing — the theme did not change');

  // And the source is the description, since the text inside a picture is not text any more.
  const alt = await bilder.first().getAttribute('alt');
  assert(alt?.includes('graph TD;'), `the image has no description of the diagram: ${alt}`);

  // One expected complaint is filtered, by name, and it is worth knowing about. In PRODUCTION
  // every diagram logs `Refused to apply inline style … style-src 'self' 'nonce-…'`: mermaid
  // inserts a `<style>` element while it measures text, and refusing an injected style element
  // is precisely what that directive is for. Verified by hand against a production build — the
  // drawing is unaffected, because that `<style>` is serialised into the returned SVG and is
  // the image's own business once it is inside the `<img>`. It does not appear HERE, because
  // SvelteKit adds `'unsafe-inline'` to `style-src` in development; the exclusion is written
  // down so that a green run in dev is not read as a promise about production.
  const echte = klagen.filter((zeile) => !/Refused to apply inline style/.test(zeile));
  assert(echte.length === 0, `the browser complained while drawing: ${echte.join(' | ')}`);
});

await check('L2 the picture shown is the one drawn for the theme in force', async (page) => {
  // The whole of what the two images buy. An `<img>` cannot follow `prefers-color-scheme` or
  // the site's own control, so the stylesheet has to choose — and a wrong rule here is
  // invisible in the light theme, which is the one everything else is checked in.
  await page.goto(BASE + DIAGRAM_PAGE, { waitUntil: 'networkidle' });
  const hell = page.locator('article.prose img.hell');
  const dunkel = page.locator('article.prose img.dunkel');
  await hell.waitFor({ state: 'visible', timeout: 60_000 });
  assert(await dunkel.isHidden(), 'the dark drawing is shown on a light page');

  await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
  await dunkel.waitFor({ state: 'visible', timeout: 5_000 });
  assert(await hell.isHidden(), 'the light drawing is still shown after switching to dark');
});

await check('L3 a label with the documented line break is a picture, not a broken image', async (page) => {
  // The bug this check exists for, and the reason it is HERE rather than in `vitest`: it is
  // the pure case of something that works only because no browser was involved.
  //
  // Mermaid renders an `htmlLabels` label as HTML inside a `<foreignObject>` and serialises
  // the finished SVG with `innerHTML` — the HTML serialiser — so `A[Erste Zeile<br>Zweite
  // Zeile]`, the documented and canonical Mermaid line break, comes back with a `<br>` that
  // has no closing tag. That is well-formed HTML and it is NOT well-formed XML, and
  // `data:image/svg+xml` is parsed as strict XML: the address does not decode, and the reader
  // gets a broken-image glyph in place of a diagram whose source was perfectly good. Chromium,
  // Firefox and WebKit all agreed; `DOMParser` on the decoded source answered *"Opening and
  // ending tag mismatch: br line 1 and p"*. `<br/>` behaved identically, because the HTML
  // serialiser normalises it back.
  //
  // Nothing in the unit suites could see it — none of them decodes an image — and the seeded
  // diagram had no `<br>` in it. So the corpus now has one, `mermaidConfig` sets
  // `htmlLabels: false` so that the break is drawn as `<tspan>`s instead, and every address is
  // put through the browser's own decoder before it reaches the page. This asserts both
  // halves: that the example still holds the case, and that it draws.
  const first = await (await page.request.get(BASE + DIAGRAM_PAGE)).text();
  // `&lt;br>`, not `&lt;br&gt;`: Svelte's text escaping replaces `&` and `<` and leaves `>`
  // alone, which is correct and is the kind of detail that makes an assertion about escaped
  // markup fail for the wrong reason.
  assert(
    first.includes('&lt;br>'),
    'the seeded diagram no longer contains a <br> label — the case this check exists for is gone'
  );

  await page.goto(BASE + DIAGRAM_PAGE, { waitUntil: 'networkidle' });
  const bilder = page.locator('article.prose img[src^="data:image/svg+xml"]');
  await bilder.first().waitFor({ state: 'visible', timeout: 60_000 });

  // EVERY drawing, not merely the first: the two are separate renders and a serialisation
  // fault would take both, but a theme-specific one would take only one.
  const breiten = await bilder.evaluateAll((els) => els.map((el) => el.naturalWidth));
  assert(breiten.length === 2, `expected two drawings, got ${breiten.length}`);
  assert(
    breiten.every((breite) => breite > 0),
    `a drawing did not decode — the <br> case is back: ${breiten.join(', ')}`
  );

  // And the break really is drawn rather than dropped: with `htmlLabels: false` mermaid puts
  // each line in its own `<tspan>`, so the decoded source carries two of them where the label
  // was one string. Read out of the address rather than out of the DOM — the picture is an
  // `<img>` and its markup is deliberately unreachable from this page.
  const quelle = decodeURIComponent(
    (await bilder.first().getAttribute('src'))?.replace(/^data:image\/svg\+xml;charset=utf-8,/, '') ??
      ''
  );
  assert(!quelle.includes('<br>'), 'the drawing still carries an unclosed <br>');
  // Word by word rather than as one string: mermaid lays a text label out one word per
  // `<tspan>`, so `Seite im` is not a contiguous run in the markup even though it is one line
  // on the screen. What matters is that BOTH halves of the broken label were drawn — a break
  // that had been mishandled by truncation would lose the second one silently.
  const fehlend = ['Seite', 'Browser'].filter((wort) => !quelle.includes(wort));
  assert(
    fehlend.length === 0,
    `the label lost ${fehlend.join(', ')} — decoded drawing begins: ${quelle.slice(0, 300)}`
  );
});

await browser.close();

// ---------------------------------------------------------------------------------------
const passed = results.filter((r) => r.ok).length;
const failed = results.length - passed;
console.log(`\n${passed}/${results.length} checks passed${failed ? `, ${failed} FAILED` : ''}`);

process.exit(failed === 0 ? 0 : 1);
