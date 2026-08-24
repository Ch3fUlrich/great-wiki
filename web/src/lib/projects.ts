/**
 * Projects, on the wire and in words.
 *
 * Pure on purpose: this module is imported from `+page.server.ts` **and** from the page
 * component, so it must stay free of `$env/dynamic/private` for the same reason
 * `$lib/adminApi` does — importing a server-only module from a component poisons the client
 * bundle. The server half (the actual calls) lives in `$lib/api`.
 *
 * Nothing here decides who may see or do anything. `/api/projects` answers only the projects
 * whose home page the caller may read, per document (D-3, D-13), and that filtering belongs
 * to `Store::projects_for` where it is mutation-tested. What this file owns is the wording of
 * a refusal, which is a smaller job and a real one: a refusal that arrives as "Fehler" is a
 * refusal nobody can act on, and the two the API actually returns — "that page is already the
 * home of a project" and "you may not write that page" — each have a different way out.
 */

/**
 * One project. Mirrors `gw_api::routes::tasks::ProjectView`, which drops `home_doc` for the
 * same reason a backlink drops the linking document's id: an internal identifier has no
 * reason to leave the API. The path and the title are what an interface links to and shows.
 */
export interface Project {
  id: string;
  home_path: string;
  home_title: string;
  /** The tag that pulls documents in from elsewhere (D-3), or `null` for none. */
  tag_id: string | null;
  created_at: string;
}

/** `GET /api/projects`. Already filtered — see the module comment. */
export interface ProjectsResponse {
  projects: Project[];
}

/**
 * What somebody typed, turned into the path the API expects — or `null` when they typed
 * nothing that could name a page.
 *
 * Three spellings of one page reach this form in practice and all three mean it: with and
 * without the leading slash, with a trailing one, and as a whole address pasted out of the
 * browser's bar. The last is worth handling rather than 404-ing, because copying a page's
 * URL is how people say "this page" — `CHANGELOG.md` records the same courtesy being added
 * for links, for the same reason.
 *
 * The origin of a pasted address is deliberately NOT checked. This function cannot know what
 * this deployment is called (the store cannot either — see the absolute-link entry in the
 * changelog), and guessing from a request header is exactly the thing that must not be done.
 * A path taken from somebody else's address is simply a path that does not exist here, and
 * the refusal names it.
 */
export function homePath(typed: string): string | null {
  let text = typed.trim();
  if (!text) return null;

  // `new URL` throws for anything without a scheme, which is every ordinary path.
  try {
    text = new URL(text).pathname;
  } catch {
    // Not an address. Take it as written.
  }

  if (!text.startsWith('/')) text = `/${text}`;
  if (text.length > 1 && text.endsWith('/')) text = text.slice(0, -1);
  return text === '/' ? null : text;
}

/** The sentence for a status nothing else has wording for. Nothing is swallowed. */
function generic(status: number, clause: string, server: string | null): string {
  const detail = server ? ` Der Server meldet: ${server}` : '';
  if (status === 0) return `${clause}: Die Anwendung antwortet nicht.`;
  return `${clause} (Fehler ${status}).${detail}`;
}

/** Why the list is not there. Never conflated with "there are none" — see the page. */
export function describeList(status: number): string {
  return generic(status, 'Die Projekte konnten nicht geladen werden', null);
}

/**
 * Why a project was not created, in the reader's own language and with the way out.
 *
 * The API's messages are English sentences written for a client, not for a person reading a
 * German page, so the two refusals it actually returns are spelled out here instead. They
 * carry the same facts — including the 409's way out, which is the half that makes it
 * actionable — and `server` is appended only where there is no wording, so an unexpected
 * status still arrives with whatever the server said about it.
 */
export function describeCreate(status: number, path: string, server: string | null = null): string {
  if (status === 0) {
    return 'Die Anwendung antwortet nicht. Es wurde nichts angelegt.';
  }
  if (status === 401) {
    return 'Nicht angemeldet — bitte erneut anmelden. Es wurde nichts angelegt.';
  }
  if (status === 403) {
    return (
      `Für »${path}« fehlt das Schreibrecht. Ein Projekt entsteht auf einer Seite, ` +
      'die man selbst bearbeiten darf.'
    );
  }
  if (status === 404) {
    return `Die Seite »${path}« gibt es nicht. Bitte zuerst anlegen oder die Schreibweise prüfen.`;
  }
  if (status === 409) {
    // The API's own way out, said in German: the project that already owns this page is one
    // the caller may read — `create_project` finds the conflict in the caller's OWN filtered
    // list — so it is on this very page, and pointing at it discloses nothing new.
    return (
      `»${path}« ist bereits die Startseite eines Projekts. Dieses Projekt steht unten in ` +
      'der Liste; für ein neues Projekt bitte eine andere Seite wählen.'
    );
  }
  return generic(status, 'Das Projekt konnte nicht angelegt werden', server);
}

/**
 * Why a project was not deleted — and, in every branch, that nothing was.
 *
 * The promise is the point. A deletion that half happened is the thing somebody would go and
 * check for, and the API takes the decision before it writes anything, so it is a promise
 * this interface may actually make.
 */
export function describeDelete(status: number, server: string | null = null): string {
  if (status === 0) {
    return 'Die Anwendung antwortet nicht. Es wurde nichts gelöscht.';
  }
  if (status === 403) {
    return (
      'Dafür fehlt das Schreibrecht auf der Startseite des Projekts. ' +
      'Es wurde nichts gelöscht.'
    );
  }
  if (status === 404) {
    return 'Dieses Projekt gibt es nicht (mehr). Es wurde nichts gelöscht.';
  }
  return `${generic(status, 'Das Projekt konnte nicht gelöscht werden', server)} Es wurde nichts gelöscht.`;
}
