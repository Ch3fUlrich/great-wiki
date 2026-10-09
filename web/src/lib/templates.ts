/**
 * Page templates and the create form, on the wire and in words (ADR 0028).
 *
 * Pure, like `$lib/trash`: imported by the route's server file and by its component, so it
 * may not touch `$env/dynamic/private`. Nothing here decides who may create what — the API
 * answers that, and this module only turns the answer into a German sentence.
 */

export const CREATE_PATH = "/neu";
export const TEMPLATES_ENDPOINT = "/api/templates";
export const PAGES_ENDPOINT = "/api/pages";

/** `GET /api/templates` entries. Mirrors `gw_store::TemplateEntry`. */
export interface TemplateEntry {
  path: string;
  title: string;
}

/** The address of the create form, optionally under a page. */
export function createHref(parent?: string): string {
  return parent && parent !== "/"
    ? `${CREATE_PATH}?unter=${encodeURIComponent(parent)}`
    : CREATE_PATH;
}

/** A refusal in words. The API's own sentence is kept — it names the way out. */
export function describeCreate(status: number, message: string | null): string {
  const rest = message ? ` (${message})` : "";
  if (status === 0)
    return "Der Server ist nicht erreichbar. Es wurde keine Seite angelegt.";
  if (status === 401)
    return "Bitte melden Sie sich an, um eine Seite anzulegen.";
  if (status === 409 || status === 400) {
    return `Die Seite wurde nicht angelegt${rest}.`;
  }
  return `Die Seite konnte nicht angelegt werden (Fehler ${status}).`;
}
