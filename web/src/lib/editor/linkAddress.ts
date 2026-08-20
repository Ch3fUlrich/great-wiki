/**
 * What the Link control in {@link EditorToolbar} does with what a person typed into the
 * `window.prompt`, before it ever reaches {@link import('$lib/blocks/render').safeHref}.
 *
 * # The bug this exists to close
 *
 * The prompt used to read, verbatim, "Adresse des Links (https://…):" — it TOLD the person
 * to paste an absolute URL. `safeHref` accepts one, the rendered `<a>` works, and the reader
 * clicks through without noticing anything is wrong. But `gw_store::links::wiki_path` only
 * turns a scheme-less, authority-less reference into a graph edge (`crates/gw-store/src/
 * links.rs`'s module doc explains why: it has no idea which origin is its own, and guessing
 * would mean inventing a hostname and drawing edges from it). An absolute `https://…` URL was
 * therefore always read as external, even the ones that point straight back at this wiki —
 * so the dominant, PROMPTED authoring flow was the one flow that recorded no edge at all.
 * The backlinks panel and the graph both look broken, silently, for every link anyone
 * actually pasted the way they were told to.
 *
 * # What this function does about it
 *
 * Normalises what was typed at the one moment this code still knows two things `gw-store`
 * deliberately does not: the browser's own `location.origin`, and the path of the page the
 * link is being written on. Neither is passed as a global read here — `location` does not
 * exist in this project's test environment, and a function that reaches for it directly
 * cannot be unit-tested — so both arrive as arguments, exactly as `collabTarget` in
 * `./session.ts` takes `origin` rather than reading it.
 *
 * - A same-origin absolute URL (what pasting the address bar produces) becomes its path,
 *   query and fragment — the same shape `wiki_path` already resolves.
 * - A foreign absolute URL is returned unchanged: it addresses somewhere this wiki cannot
 *   and must not claim an edge to, same as `wiki_path` treats it.
 * - A protocol-relative address (`//host/path`) is judged by the host it names, not by the
 *   scheme it omits: pointing at this origin it becomes a path like any other same-origin
 *   address, pointing anywhere else it is returned unchanged like any other foreign one.
 * - Anything else is a relative reference and is resolved against the CURRENT page, root-
 *   anchored by construction (`URL.pathname` always starts with `/`) — which is what fixes
 *   the companion bug in `wiki_path` for links written here specifically: Task 7 shipped
 *   root-anchoring a bare relative reference instead of resolving it against its own page,
 *   and everything typed through this control now arrives already resolved, so `wiki_path`
 *   never has to guess for it.
 *
 * # What this does NOT cover
 *
 * This function still never sees imported markdown — it runs at EDITOR INSERT TIME, not on
 * every body a revision ever holds, so an absolute self-link baked into a file before import
 * (`[siehe](https://wiki.example.org/ziel)`) never passes through it. That part of the
 * account above is still exactly true.
 *
 * What changed is that the gap this used to leave is closed by a different mechanism, not
 * by this one: `gw_store::links::replace_links` (`crates/gw-store/src/links.rs`) now accepts
 * an absolute self-link too, when the deployment's `public_origin` is configured and the
 * link's origin matches it exactly — and every write path threads that same configuration
 * through, including `Store::create_document`, which is what the importer calls. So an
 * absolute self-link in imported markdown now becomes a graph edge, same as one typed
 * through this control, PROVIDED `public_origin` is configured. With it unset, `gw-store`
 * still cannot resolve any absolute URL on its own — the deployment host is configuration,
 * not something either it or the importer is handed — and every absolute link, imported or
 * typed, is external, same as always.
 *
 * Two limitations remain regardless of configuration, both in `gw-store` rather than here: a
 * protocol-relative address (`//wiki.example.org/ziel`, no scheme) is always external to it,
 * unconditionally — which is why this function resolves the same-origin case to a path before
 * `gw-store` ever sees it; and a page imported or published before `public_origin` was
 * configured keeps the edges that import produced until something re-publishes it — there is
 * no backfill.
 */
export function normalizeLinkAddress(origin: string, currentPath: string, typed: string): string {
  const trimmed = typed.trim();
  if (trimmed === '') return trimmed;

  try {
    // Parses only when `trimmed` carries its own scheme and is therefore an ABSOLUTE
    // reference — `new URL` with no base throws on anything relative, which is how the two
    // cases below are told apart.
    const asTyped = new URL(trimmed);
    if (asTyped.origin === origin) {
      return asTyped.pathname + asTyped.search + asTyped.hash;
    }
    // A foreign origin, or a non-special scheme (`mailto:`, `javascript:`, …) for which the
    // WHATWG parser reports `origin` as the literal string `"null"` — never equal to a real
    // `location.origin`, so it falls through here unchanged. `safeHref` is what refuses a
    // dangerous scheme; this function's job ends at "did not touch a foreign address".
    return trimmed;
  } catch {
    // Not parseable on its own, so it is relative OR protocol-relative. Resolved against the
    // page it was typed on — exactly what a browser would do with it left in the body
    // unresolved — which is the whole fix: this used to reach `wiki_path` un-resolved and get
    // root-anchored instead, naming a page the link did not go to.
    const resolved = new URL(trimmed, origin + currentPath);
    // `//host/path` lands here too: it carries an AUTHORITY but no scheme, so `new URL` with
    // no base throws on it exactly as a relative reference does. Resolving it borrows the
    // page's scheme and REPLACES the host, so the resolved origin is the typed one, not this
    // wiki's — and returning `.pathname` off that would throw the host away and silently turn
    // `//evil.example/phish` into a link to this wiki's own `/phish`. Comparing origins is
    // what tells the two cases apart; a genuinely relative reference can never change origin,
    // so this is inert for every one of those.
    if (resolved.origin !== origin) return trimmed;
    return resolved.pathname + resolved.search + resolved.hash;
  }
}
