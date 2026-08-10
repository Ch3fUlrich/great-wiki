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

await browser.close();

// ---------------------------------------------------------------------------------------
const passed = results.filter((r) => r.ok).length;
const failed = results.length - passed;
console.log(`\n${passed}/${results.length} checks passed${failed ? `, ${failed} FAILED` : ''}`);

process.exit(failed === 0 ? 0 : 1);
