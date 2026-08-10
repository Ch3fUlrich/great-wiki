//! great-wiki's own sign-in page.
//!
//! D-M2-11: there is one *Anmelden* control, and it opens this page rather than going
//! straight to Authelia. Which mechanism somebody uses is great-wiki's business, not
//! something a reader has to understand before clicking.
//!
//! # Why the API renders it and not SvelteKit
//!
//! `/auth/*` already belongs to the application, not to the web front end: the Vite dev
//! server proxies the whole prefix to port 8092 and Caddy routes it the same way, because
//! the OIDC callback is a browser navigation that has to reach the code holding the flow
//! cookies. Putting the page in SvelteKit would mean splitting one prefix across two
//! servers so that `/auth/login` went one way and `/auth/callback` the other — a routing
//! rule that would be wrong in exactly one direction and silently.
//!
//! So this is hand-written HTML with its own styles inlined. It loads nothing from
//! anywhere: no script, no font, no stylesheet, not even from this origin. A sign-in page
//! that fetches something is a sign-in page that a third party can change, and this one
//! renders identically with JavaScript switched off — which for a login is the difference
//! between failing visibly and failing silently.
//!
//! The colours are the same tokens `web/src/lib/styles/tokens.css` declares, restated
//! here because there is no way to reach that file from a response this server writes.
//! They are duplicated deliberately and the duplication is small: a theme drifting means
//! the login page looks slightly older than the wiki, which is a cosmetic bug, whereas
//! reaching across to the front end for a stylesheet would be an external dependency on
//! the one page that must never have one.

use crate::routes::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;

use super::oidc::random_secret;
use super::session::flow_cookie;

/// The double-submit token for the guest form.
///
/// `SameSite=Lax` already withholds cookies from a cross-site POST, so this is the second
/// of two independent defences rather than the only one. Both are cheap and neither is
/// sufficient on its own: `Lax` is a browser behaviour that older clients and some
/// embedded webviews get wrong, and a token with no `SameSite` behind it is only as good
/// as the cookie's confidentiality.
pub const CSRF_COOKIE: &str = "__Host-gw_login_csrf";

/// How long a token issued with the page stays usable. Long enough to read the page,
/// find the password and type it; short enough that a token left in a browser overnight
/// is not still current in the morning.
const CSRF_TTL_SECONDS: i64 = 60 * 60;

/// What, if anything, to tell the person about their last attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// Wrong password, unknown username, deactivated account, missing token — every
    /// refusal reads the same. Anything more specific is a list of who has an account.
    Failed,
    /// The throttle refused before any password was looked at. This one is allowed to
    /// differ from `Failed`, because it says nothing about whether the account exists:
    /// the counter is keyed on the submitted string and rises for imaginary names too.
    Throttled,
}

impl Notice {
    fn message(self) -> &'static str {
        match self {
            Notice::Failed => "Anmeldung fehlgeschlagen. Bitte erneut versuchen.",
            Notice::Throttled => {
                "Zu viele fehlgeschlagene Versuche. Bitte in einigen Minuten erneut versuchen."
            }
        }
    }
}

/// Minimal HTML escaping for attribute and text content.
///
/// Only the CSRF token is interpolated today and it is base64url, so nothing here can
/// currently be reached. It is applied anyway: the next person to add a field to this
/// page should find escaping already in the path rather than have to think of it.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Render the page.
///
/// `homelab` is false when no identity provider is configured. The button is then absent
/// rather than present-and-broken — the same choice `/api/me`'s `login_available` already
/// made, for the same reason: sending somebody to a control that answers 503 is worse
/// than not offering it.
pub fn render(homelab: bool, csrf: &str, notice: Option<Notice>) -> String {
    let homelab_block = if homelab {
        r#"    <a class="homelab" href="/auth/oidc" rel="nofollow">Mit Homelab-Konto anmelden</a>
    <p class="hint">Für alle, die ein Konto im Homelab haben. Die Anmeldung läuft über
      Authelia, einschließlich zweitem Faktor.</p>
    <div class="trenner"><span>oder</span></div>
"#
    } else {
        // No provider configured. Local accounts still work, which is a legitimate
        // deployment, so the guest form below stays.
        ""
    };

    let notice_block = match notice {
        Some(notice) => format!(
            // `role="alert"` so a screen reader announces the refusal instead of leaving
            // somebody wondering whether the button worked.
            "    <p class=\"fehler\" role=\"alert\">{}</p>\n",
            escape(notice.message())
        ),
        None => String::new(),
    };

    format!(
        r##"<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>Anmelden — great-wiki</title>
<style>
:root {{
  color-scheme: light dark;
  --bg: #fdfdfc; --bg-raised: #ffffff; --bg-sunken: #f4f4f2;
  --border: #e3e2df; --border-strong: #c9c8c3;
  --ink: #1a1a18; --ink-muted: #5c5b56; --ink-faint: #8a8983;
  --accent: #2f5fd0; --accent-ink: #ffffff; --focus: #2f5fd0; --danger: #a32a2a;
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --bg: #14161a; --bg-raised: #1b1e24; --bg-sunken: #101216;
    --border: #2b2f38; --border-strong: #3d424d;
    --ink: #e8e9ec; --ink-muted: #a5a9b3; --ink-faint: #757a85;
    --accent: #8ab4ff; --accent-ink: #101216; --focus: #8ab4ff; --danger: #f08a86;
  }}
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0; padding: 2rem 1rem; min-height: 100vh;
  display: flex; align-items: center; justify-content: center;
  background: var(--bg); color: var(--ink);
  font: 1.0625rem/1.65 ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
}}
main {{
  inline-size: 100%; max-inline-size: 26rem;
  padding: 2rem; border: 1px solid var(--border); border-radius: 8px;
  background: var(--bg-raised);
}}
h1 {{ margin: 0 0 0.25rem; font-size: 1.424rem; letter-spacing: -0.01em; }}
h2 {{ margin: 0 0 0.75rem; font-size: 1.0625rem; }}
p {{ margin: 0 0 1rem; }}
.hint {{ color: var(--ink-muted); font-size: 0.889rem; }}
.fehler {{
  padding: 0.75rem 1rem; margin-block-end: 1.5rem;
  border: 1px solid var(--danger); border-radius: 4px;
  color: var(--danger); font-size: 0.889rem;
}}
.homelab {{
  display: block; padding: 0.75rem 1rem; margin-block-end: 0.75rem;
  border: 1px solid transparent; border-radius: 4px;
  background: var(--accent); color: var(--accent-ink);
  font-weight: 650; text-align: center; text-decoration: none;
}}
.homelab:hover {{ filter: brightness(1.08); }}
.trenner {{
  display: flex; align-items: center; gap: 0.75rem;
  margin: 1.5rem 0; color: var(--ink-faint); font-size: 0.79rem;
}}
.trenner::before, .trenner::after {{
  content: ''; flex: 1; border-block-start: 1px solid var(--border);
}}
label {{ display: block; margin-block-end: 0.25rem; font-size: 0.889rem; font-weight: 650; }}
input {{
  inline-size: 100%; padding: 0.5rem 0.75rem; margin-block-end: 1rem;
  border: 1px solid var(--border-strong); border-radius: 4px;
  background: var(--bg); color: var(--ink); font: inherit;
}}
button {{
  inline-size: 100%; padding: 0.75rem 1rem;
  border: 1px solid var(--border-strong); border-radius: 4px;
  background: var(--bg-sunken); color: var(--ink);
  font: inherit; font-weight: 650; cursor: pointer;
}}
button:hover {{ border-color: var(--ink-faint); }}
:focus-visible {{ outline: 2px solid var(--focus); outline-offset: 2px; }}
.zurueck {{ display: block; margin-block-start: 1.5rem; color: var(--ink-muted); font-size: 0.889rem; }}
</style>
</head>
<body>
  <main>
    <h1>Bei great-wiki anmelden</h1>
    <p class="hint">Öffentliche Seiten sind ohne Anmeldung lesbar.</p>
{notice_block}{homelab_block}    <h2>Gastzugang</h2>
    <form method="post" action="/auth/local">
      <input type="hidden" name="csrf" value="{csrf}">
      <label for="username">Benutzername</label>
      <input id="username" name="username" type="text" autocomplete="username"
             autocapitalize="none" autocorrect="off" spellcheck="false" required>
      <label for="password">Passwort</label>
      <input id="password" name="password" type="password" autocomplete="current-password"
             required>
      <button type="submit">Als Gast anmelden</button>
    </form>
    <a class="zurueck" href="/">Zurück zum Wiki</a>
  </main>
</body>
</html>
"##,
        csrf = escape(csrf),
    )
}

/// A `200 text/html` carrying the page, plus the CSRF cookie when one had to be minted.
///
/// The token is REUSED when the browser already holds one rather than rotated on every
/// render. Rotating would break the ordinary case of two tabs open on the same form, and
/// it would make two refusals from one browser differ byte for byte — which is what
/// `a_wrong_password_and_an_unknown_username_are_indistinguishable` compares.
pub fn respond(
    jar: CookieJar,
    homelab: bool,
    notice: Option<Notice>,
    status: StatusCode,
) -> Response {
    let existing = jar
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_string())
        .filter(|value| !value.trim().is_empty());

    let (token, jar) = match existing {
        Some(token) => (token, jar),
        None => {
            let token = random_secret();
            let jar = jar.add(flow_cookie(CSRF_COOKIE, token.clone(), CSRF_TTL_SECONDS));
            (token, jar)
        }
    };

    (
        status,
        jar,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            // This page is never a cached artefact: it carries a per-browser token, and a
            // shared cache holding one would hand it to the next person through.
            (header::CACHE_CONTROL, "no-store"),
        ],
        render(homelab, &token, notice),
    )
        .into_response()
}

/// `GET /auth/login` — the page D-M2-11 put in front of both mechanisms.
pub async fn show(State(state): State<AppState>, jar: CookieJar) -> Response {
    respond(jar, state.oidc.is_some(), None, StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::{escape, render, Notice};

    #[test]
    fn the_homelab_button_appears_only_when_a_provider_is_configured() {
        assert!(render(true, "t", None).contains("/auth/oidc"));
        let without = render(false, "t", None);
        assert!(!without.contains("/auth/oidc"), "{without}");
        assert!(
            without.contains("/auth/local"),
            "the guest form must survive a deployment with no provider"
        );
    }

    #[test]
    fn the_token_is_carried_in_the_form() {
        assert!(render(true, "abc123", None).contains(r#"name="csrf" value="abc123""#));
    }

    #[test]
    fn markup_in_a_token_cannot_escape_the_attribute() {
        // Unreachable today — tokens are base64url — and asserted so that it stays that
        // way if anything else is ever interpolated here.
        let page = render(true, r#"" onload="alert(1)"#, None);
        assert!(!page.contains(r#"onload="alert"#), "{page}");
        assert_eq!(escape(r#"<a href="x">"#), "&lt;a href=&quot;x&quot;&gt;");
    }

    #[test]
    fn the_two_notices_say_different_things_and_neither_names_an_account() {
        let failed = render(true, "t", Some(Notice::Failed));
        let throttled = render(true, "t", Some(Notice::Throttled));
        assert!(failed.contains("Anmeldung fehlgeschlagen"), "{failed}");
        assert!(throttled.contains("Zu viele"), "{throttled}");
        assert_ne!(failed, throttled);
        assert!(render(true, "t", None).contains("Bei great-wiki anmelden"));
    }
}
