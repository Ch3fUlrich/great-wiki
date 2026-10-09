import { fail, redirect } from "@sveltejs/kit";
import { apiGet, apiSend } from "$lib/api";
import {
  describeCreate,
  PAGES_ENDPOINT,
  TEMPLATES_ENDPOINT,
  type TemplateEntry,
} from "$lib/templates";
import type { Actions, PageServerLoad } from "./$types";

/**
 * The create form (ADR 0028): a title, an optional parent, an optional template.
 *
 * **A real form action, no script needed** — the browser's own submission carries the cookie
 * and SvelteKit's origin check is the CSRF defence, as for every other action here. The
 * template list is `GET /api/templates`, which is already filtered to what the caller may
 * read; nothing here filters, and a failed list is an empty picker, not a failed page.
 *
 * **Nothing here decides whether the caller may create.** `POST /api/pages` does, and its
 * refusal is shown in its own words. A parent or template the caller cannot read arrives
 * worded as a missing one (ADR 0022), so the form cannot be used to probe for pages.
 */
export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get("cookie");
  let vorlagen: TemplateEntry[] = [];
  try {
    const answer = await apiGet<TemplateEntry[]>(
      fetch,
      TEMPLATES_ENDPOINT,
      cookie,
    );
    vorlagen = answer.data ?? [];
  } catch {
    vorlagen = [];
  }
  return { vorlagen, unter: url.searchParams.get("unter") ?? "" };
};

export const actions: Actions = {
  anlegen: async ({ request, fetch }) => {
    const form = await request.formData();
    const titel = String(form.get("titel") ?? "").trim();
    const unter = String(form.get("unter") ?? "").trim();
    const vorlage = String(form.get("vorlage") ?? "").trim();
    if (!titel) {
      return fail(400, {
        fehler:
          "Bitte geben Sie einen Titel an. Es wurde keine Seite angelegt.",
        titel,
        unter,
        vorlage,
      });
    }
    const { status, data, failure } = await apiSend<{ path: string }>(
      fetch,
      "POST",
      PAGES_ENDPOINT,
      request.headers.get("cookie"),
      { parent: unter || null, title: titel, template: vorlage || null },
    );
    if (failure || !data) {
      return fail(status === 0 ? 503 : status || 500, {
        fehler: describeCreate(status, failure?.message ?? null),
        titel,
        unter,
        vorlage,
      });
    }
    redirect(303, `${data.path}?edit=1`);
  },
};
