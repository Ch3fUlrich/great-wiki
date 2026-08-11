//! `GET /auth/invite/{token}` and `POST /auth/invite/{token}/accept`.
//!
//! The other end of D-M2-3: the recipient of a link sets their own password, so no
//! credential ever passes through chat or email. What passes instead is the link, and the
//! link is itself a credential — which is what shapes everything below.
//!
//! # Why the API renders this and not SvelteKit
//!
//! The same reason `/auth/login` is rendered here (see [`super::page`]): `/auth/*` is
//! proxied to this application by both Vite and Caddy, because the OIDC callback is a
//! browser navigation that has to reach the code holding the flow cookies. Splitting the
//! prefix across two servers so that one path went to SvelteKit would be a routing rule
//! that is wrong in exactly one direction and silently. This page also loads nothing from
//! anywhere — no script, no font, no stylesheet — and renders identically with JavaScript
//! off, which for a page that creates an account is the difference between failing visibly
//! and failing silently.
//!
//! # The four rules this file exists to keep
//!
//! - **One answer for four states.** Unknown, expired, revoked and already-spent all
//!   produce [`refused`], byte for byte, on both the GET and the POST. Anything else turns
//!   the endpoint into a way to ask which tokens exist.
//! - **The token is checked first.** Before the double-submit token, before the display
//!   name, before the password. Two reasons: no later check can then produce a different
//!   answer for one of the four states, and an attacker walking the token space never
//!   provokes an argon2id hash, which is the expensive thing they would otherwise get for
//!   free.
//! - **Constant-time comparison.** The lookup is by digest, so the index is on a value
//!   nobody can reverse; the confirmation is [`constant_time_eq`], so no code path here
//!   compares a secret with `==`.
//! - **The password goes through `accept_new_password`.** Length floor *and* breach corpus,
//!   with the degraded mode audited (D-M2-16). Calling `hash_password` directly is how the
//!   admin API once ended up with a policy that looked implemented and checked nothing.

use crate::error::ApiError;
use crate::routes::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use axum_extra::extract::CookieJar;
use gw_auth::{PasswordError, Permission};
use gw_store::{AcceptOutcome, InviteOffer};
use serde::Deserialize;
use subtle::ConstantTimeEq;

use super::oidc::random_secret;
use super::page::CSRF_COOKIE;
use super::password_policy::accept_new_password;
use super::session::{cleared_cookie, flow_cookie, hash_token, new_session_token, session_cookie};

/// How long the double-submit token issued with this page stays usable. The same hour the
/// sign-in page uses, and the same cookie: it is a proof of same-origin cookie access, not
/// a fact about which form it was minted on.
const CSRF_TTL_SECONDS: i64 = 60 * 60;

/// What the form submits.
///
/// Every field defaults, so a truncated or hand-made body lands on the ordinary refusal
/// path rather than on axum's 422 — a response shape only malformed requests would ever
/// see, and therefore something to probe with.
#[derive(Debug, Deserialize)]
pub struct Acceptance {
    #[serde(default)]
    display_name: String,
    /// Never logged, never echoed, never stored except as an argon2id hash.
    #[serde(default)]
    password: String,
    #[serde(default)]
    csrf: String,
}

/// Constant-time comparison. `==` on strings returns at the first differing byte and so
/// times how much of a guess was right.
///
/// A sibling of the one in [`super::local`], kept separate rather than shared because that
/// one is private to the module it defends and neither should acquire a caller by
/// accident.
fn constant_time_eq(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// The invite this token names, if it is redeemable.
///
/// `None` for unknown, expired, revoked and already-spent alike — the store returns one
/// answer for all four, and this function is careful not to invent a second one. A store
/// error is also `None`: a database that cannot say whether this invitation is live has
/// not established that it is.
async fn offer(state: &AppState, token: &str) -> Option<InviteOffer> {
    let presented = hash_token(token);
    match state.store.invite_offer(&presented).await {
        Ok(Some(offer)) if constant_time_eq(&offer.token_hash, &presented) => Some(offer),
        Ok(_) => None,
        Err(error) => {
            tracing::error!(%error, "could not resolve an invite token");
            None
        }
    }
}

/// What, if anything, to tell the person about their last attempt.
///
/// None of these says anything about a token: by the time one is shown, the token has
/// already been established as good. They are about the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The double-submit token was missing or wrong — in practice a page left open past
    /// the cookie's hour, or cookies blocked.
    Stale,
    NameRequired,
    TooShort,
    Breached,
    /// Somebody took the username while the invitation was outstanding.
    UsernameTaken,
}

impl Notice {
    fn message(self) -> &'static str {
        match self {
            Notice::Stale => {
                "Das Formular ist abgelaufen. Bitte laden Sie die Seite neu und versuchen Sie es \
                 erneut."
            }
            Notice::NameRequired => "Bitte geben Sie einen Anzeigenamen an.",
            Notice::TooShort => "Das Passwort muss mindestens 12 Zeichen lang sein.",
            Notice::Breached => {
                "Dieses Passwort steht in einem bekannten Datenleck. Bitte wählen Sie ein anderes."
            }
            Notice::UsernameTaken => {
                "Dieser Benutzername ist inzwischen vergeben. Bitte wenden Sie sich an die Person, \
                 die Sie eingeladen hat."
            }
        }
    }
}

impl From<PasswordError> for Notice {
    fn from(error: PasswordError) -> Self {
        match error {
            PasswordError::TooShort => Notice::TooShort,
            PasswordError::Breached { .. } => Notice::Breached,
        }
    }
}

/// HTML escaping for text and attribute content.
///
/// Load-bearing here, unlike on the sign-in page. This page interpolates a display name, a
/// team name, a username and a path — every one of them chosen by somebody else, and the
/// display name by whoever holds an account that can invite. Without this, an inviter
/// could put script into the page their invitee is asked to type a password into.
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

/// What a permission means to somebody who has never seen this wiki.
fn permission_word(permission: Permission) -> &'static str {
    match permission {
        Permission::Read => "Lesezugriff",
        Permission::Comment => "Kommentarzugriff",
        Permission::Write => "Schreibzugriff",
        Permission::Admin => "Verwaltungszugriff",
    }
}

/// The colours and the shell, shared with [`super::page`] so a person who has just been
/// invited does not land on something that looks like a different site. Restated rather
/// than imported for the reason given there: a page that must never fetch anything cannot
/// reach across to the front end for a stylesheet.
const STYLE: &str = r#"
:root {
  color-scheme: light dark;
  --bg: #fdfdfc; --bg-raised: #ffffff; --bg-sunken: #f4f4f2;
  --border: #e3e2df; --border-strong: #c9c8c3;
  --ink: #1a1a18; --ink-muted: #5c5b56; --ink-faint: #8a8983;
  --accent: #2f5fd0; --accent-ink: #ffffff; --focus: #2f5fd0; --danger: #a32a2a;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #14161a; --bg-raised: #1b1e24; --bg-sunken: #101216;
    --border: #2b2f38; --border-strong: #3d424d;
    --ink: #e8e9ec; --ink-muted: #a5a9b3; --ink-faint: #757a85;
    --accent: #8ab4ff; --accent-ink: #101216; --focus: #8ab4ff; --danger: #f08a86;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0; padding: 2rem 1rem; min-height: 100vh;
  display: flex; align-items: center; justify-content: center;
  background: var(--bg); color: var(--ink);
  font: 1.0625rem/1.65 ui-sans-serif, system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
}
main {
  inline-size: 100%; max-inline-size: 26rem;
  padding: 2rem; border: 1px solid var(--border); border-radius: 8px;
  background: var(--bg-raised);
}
h1 { margin: 0 0 0.25rem; font-size: 1.424rem; letter-spacing: -0.01em; }
h2 { margin: 1.5rem 0 0.75rem; font-size: 1.0625rem; }
p { margin: 0 0 1rem; }
ul { margin: 0 0 1rem; padding-inline-start: 1.25rem; }
li { margin-block-end: 0.25rem; }
.hint { color: var(--ink-muted); font-size: 0.889rem; }
.fehler {
  padding: 0.75rem 1rem; margin-block-end: 1.5rem;
  border: 1px solid var(--danger); border-radius: 4px;
  color: var(--danger); font-size: 0.889rem;
}
label { display: block; margin-block-end: 0.25rem; font-size: 0.889rem; font-weight: 650; }
input {
  inline-size: 100%; padding: 0.5rem 0.75rem; margin-block-end: 1rem;
  border: 1px solid var(--border-strong); border-radius: 4px;
  background: var(--bg); color: var(--ink); font: inherit;
}
button {
  inline-size: 100%; padding: 0.75rem 1rem;
  border: 1px solid transparent; border-radius: 4px;
  background: var(--accent); color: var(--accent-ink);
  font: inherit; font-weight: 650; cursor: pointer;
}
button:hover { filter: brightness(1.08); }
:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.zurueck { display: block; margin-block-start: 1.5rem; color: var(--ink-muted); font-size: 0.889rem; }
"#;

fn shell(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>{title} — great-wiki</title>
<style>{STYLE}</style>
</head>
<body>
  <main>
{body}    <a class="zurueck" href="/">Zurück zum Wiki</a>
  </main>
</body>
</html>
"#
    )
}

/// The page a live invitation shows.
pub fn render(offer: &InviteOffer, token: &str, csrf: &str, notice: Option<Notice>) -> String {
    let inviter = match &offer.invited_by_name {
        Some(name) => format!(
            "<strong>{}</strong> hat Sie zu great-wiki eingeladen.",
            escape(name)
        ),
        // The invite outlives whoever made it. Saying "somebody" is better than inventing
        // a name, and better than an empty space that reads like a rendering bug.
        None => "Sie wurden zu great-wiki eingeladen.".to_string(),
    };

    let mut carries = String::new();
    if let (Some(path), Some(permission)) = (&offer.path, offer.permission) {
        carries.push_str(&format!(
            "      <li>{} auf <code>{}</code></li>\n",
            permission_word(permission),
            escape(path)
        ));
    }
    if let Some(team) = &offer.team_name {
        carries.push_str(&format!(
            "      <li>Mitgliedschaft im Team „{}“</li>\n",
            escape(team)
        ));
    }

    let notice_block = match notice {
        // `role="alert"` so a screen reader announces the refusal rather than leaving
        // somebody wondering whether the button worked.
        Some(notice) => format!(
            "    <p class=\"fehler\" role=\"alert\">{}</p>\n",
            escape(notice.message())
        ),
        None => String::new(),
    };

    // The window matters to the person reading: it is why the link in their inbox may stop
    // working. The date alone — the stored value's time component is noise here.
    let expires = offer
        .expires_at
        .split_whitespace()
        .next()
        .unwrap_or(&offer.expires_at);

    let body = format!(
        r#"    <h1>Einladung zu great-wiki</h1>
    <p>{inviter}</p>
{notice_block}    <p>Damit erhalten Sie:</p>
    <ul>
{carries}    </ul>
    <p class="hint">Benutzername: <strong>{username}</strong>. Diese Einladung gilt einmalig und
      läuft am {expires} ab.</p>
    <h2>Konto anlegen</h2>
    <form method="post" action="/auth/invite/{token}/accept">
      <input type="hidden" name="csrf" value="{csrf}">
      <label for="display_name">Anzeigename</label>
      <input id="display_name" name="display_name" type="text" autocomplete="name"
             maxlength="120" required>
      <label for="password">Passwort</label>
      <input id="password" name="password" type="password" autocomplete="new-password"
             minlength="12" required>
      <p class="hint">Mindestens 12 Zeichen. Keine Vorschriften zu Groß- und Kleinschreibung
        oder Sonderzeichen — eine lange Passphrase ist besser als ein kurzes Kunstwort.</p>
      <button type="submit">Konto erstellen und anmelden</button>
    </form>
"#,
        username = escape(&offer.username),
        token = escape(token),
        csrf = escape(csrf),
    );

    shell("Einladung", &body)
}

/// The page for a token that offers nothing.
///
/// One page for unknown, expired, revoked and spent, and it names all four possibilities
/// precisely so that it reveals which one applies to none of them.
pub fn render_refusal() -> String {
    shell(
        "Einladung",
        "    <h1>Einladung ungültig</h1>\n    <p>Diese Einladung ist nicht (mehr) gültig. Sie \
         wurde bereits benutzt, zurückgezogen oder ist abgelaufen — oder der Link ist \
         unvollständig. Bitte fragen Sie nach einer neuen.</p>\n",
    )
}

/// The one refusal, used by both the GET and the POST.
///
/// Sets no cookie and carries no per-request value, so two refusals are identical bytes
/// however they were reached. That is the whole point: the four states this stands in for
/// must not be tellable apart.
fn refused() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        render_refusal(),
    )
        .into_response()
}

/// The invitation page, plus the double-submit cookie when one had to be minted.
///
/// The token is REUSED when the browser already holds one rather than rotated on every
/// render, exactly as the sign-in page does: rotating would break two tabs on one form and
/// would make two refusals from one browser differ byte for byte.
fn respond(
    jar: CookieJar,
    offer: &InviteOffer,
    token: &str,
    notice: Option<Notice>,
    status: StatusCode,
) -> Response {
    let existing = jar
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_string())
        .filter(|value| !value.trim().is_empty());

    let (csrf, jar) = match existing {
        Some(csrf) => (csrf, jar),
        None => {
            let csrf = random_secret();
            let jar = jar.add(flow_cookie(CSRF_COOKIE, csrf.clone(), CSRF_TTL_SECONDS));
            (csrf, jar)
        }
    };

    (
        status,
        jar,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            // Never a cached artefact: it carries a per-browser token and names an account
            // that is about to exist. A shared cache holding one would hand both to the
            // next person through.
            (header::CACHE_CONTROL, "no-store"),
        ],
        render(offer, token, &csrf, notice),
    )
        .into_response()
}

/// `GET /auth/invite/{token}`.
pub async fn show(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(token): Path<String>,
) -> Response {
    match offer(&state, &token).await {
        Some(offer) => respond(jar, &offer, &token, None, StatusCode::OK),
        None => refused(),
    }
}

/// `POST /auth/invite/{token}/accept`.
///
/// Returns a `Response` rather than a `Result` because every refusal is a rendered page and
/// the token refusal in particular has to be one exact page: routing it through `ApiError`
/// would give it a JSON body and a status that varied with the reason, which is the
/// distinction this handler exists to avoid.
pub async fn accept(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(token): Path<String>,
    Form(form): Form<Acceptance>,
) -> Response {
    // THE TOKEN FIRST. Nothing below may answer before this does, or one of the four
    // states would produce a different response from the others — and an attacker walking
    // the token space would provoke an argon2id hash per guess.
    let Some(offer) = offer(&state, &token).await else {
        return refused();
    };

    // The token this browser was issued, presented back in the form. `SameSite=Lax` already
    // withholds the cookie from a cross-site POST; this is the second of two independent
    // defences, and neither is sufficient alone.
    let expected = jar
        .get(CSRF_COOKIE)
        .map(|cookie| cookie.value().to_string())
        .unwrap_or_default();
    if expected.trim().is_empty() || !constant_time_eq(&expected, &form.csrf) {
        tracing::warn!("invite acceptance refused: the form carried no valid CSRF token");
        return respond(
            jar,
            &offer,
            &token,
            Some(Notice::Stale),
            StatusCode::BAD_REQUEST,
        );
    }

    let display_name = form.display_name.trim().to_string();
    if display_name.is_empty() {
        return respond(
            jar,
            &offer,
            &token,
            Some(Notice::NameRequired),
            StatusCode::BAD_REQUEST,
        );
    }

    // The FULL policy (D-M2-16): the length floor, the breach corpus, and an audit row
    // when the corpus could not be reached. `hash_password` is deliberately not called
    // here — it is a hash with no policy in front of it, which is exactly the defect the
    // admin API shipped with.
    //
    // It runs BEFORE the transaction below, so a refused password consumes no invite and
    // creates no account — and so that the audit row it may write is not competing for the
    // connection the transaction holds.
    //
    // The actor is `None`: nobody is signed in, and the person setting this password has
    // no principal yet. Recording the inviter would name somebody who did not choose it.
    let password_hash = match accept_new_password(
        &state.store,
        None,
        &offer.username,
        &form.password,
        state.corpus.as_ref(),
    )
    .await
    {
        Ok(hash) => hash,
        Err(error) => {
            return respond(
                jar,
                &offer,
                &token,
                Some(Notice::from(error)),
                StatusCode::BAD_REQUEST,
            )
        }
    };

    // The same generator, the same digest and the same cookie the sign-in form issues. A
    // second kind of session would be a second place to get expiry, revocation and the
    // `__Host-` attributes wrong.
    let session = new_session_token();
    match state
        .store
        .accept_invite_audited(
            &hash_token(&token),
            &display_name,
            &password_hash,
            &hash_token(&session),
            gw_store::SESSION_TTL_SECONDS,
        )
        .await
    {
        Ok(AcceptOutcome::Accepted(principal)) => {
            tracing::info!(username = %principal.username, "invite accepted");
            // The double-submit cookie has done its job and names nothing else; clearing it
            // keeps a spent token out of the browser.
            let jar = jar
                .add(session_cookie(session))
                .remove(cleared_cookie(CSRF_COOKIE));
            // 303 rather than 302: a POST answered with 302 is re-POSTed by some clients on
            // refresh, which would submit the password again.
            (
                jar,
                (StatusCode::SEE_OTHER, [(header::LOCATION, "/".to_string())]),
            )
                .into_response()
        }
        // Spent, revoked or expired between the page being rendered and the form being
        // submitted. The same page an unknown token gets — the state changed under them,
        // and which state it changed to is not something to publish.
        Ok(AcceptOutcome::Gone) => refused(),
        Ok(AcceptOutcome::UsernameTaken) => respond(
            jar,
            &offer,
            &token,
            Some(Notice::UsernameTaken),
            StatusCode::CONFLICT,
        ),
        Err(error) => ApiError::Internal(error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, escape, permission_word, render, render_refusal, Notice};
    use gw_auth::{PasswordError, Permission};
    use gw_store::InviteOffer;

    fn offer() -> InviteOffer {
        InviteOffer {
            token_hash: "digest".into(),
            username: "gast".into(),
            invited_by_name: Some("Lektor".into()),
            path: Some("/raum".into()),
            permission: Some(Permission::Read),
            team_name: None,
            expires_at: "2026-09-10 12:00:00".into(),
        }
    }

    #[test]
    fn the_page_says_who_invited_them_and_what_they_will_get() {
        let page = render(&offer(), "abc", "csrf-token", None);
        assert!(page.contains("Lektor"), "{page}");
        assert!(page.contains("Lesezugriff"), "{page}");
        assert!(page.contains("/raum"), "{page}");
        assert!(page.contains("gast"), "{page}");
        assert!(page.contains("2026-09-10"), "{page}");
        assert!(
            page.contains(r#"action="/auth/invite/abc/accept""#),
            "{page}"
        );
        assert!(page.contains(r#"name="csrf" value="csrf-token""#), "{page}");
    }

    #[test]
    fn an_invite_whose_inviter_is_gone_says_somebody_rather_than_nobody() {
        let page = render(
            &InviteOffer {
                invited_by_name: None,
                ..offer()
            },
            "abc",
            "t",
            None,
        );
        assert!(
            page.contains("Sie wurden zu great-wiki eingeladen"),
            "{page}"
        );
    }

    #[test]
    fn a_team_invite_names_the_team() {
        let page = render(
            &InviteOffer {
                path: None,
                permission: None,
                team_name: Some("Redaktion".into()),
                ..offer()
            },
            "abc",
            "t",
            None,
        );
        assert!(page.contains("Redaktion"), "{page}");
        assert!(!page.contains("Lesezugriff"), "{page}");
    }

    #[test]
    fn markup_in_a_name_a_team_or_a_token_cannot_escape_its_attribute() {
        // Every one of these is chosen by somebody else. The display name in particular is
        // chosen by whoever holds an account that can invite.
        let page = render(
            &InviteOffer {
                invited_by_name: Some(r#"<script>alert(1)</script>"#.into()),
                team_name: Some(r#""><img src=x onerror=alert(1)>"#.into()),
                username: "<b>gast</b>".into(),
                ..offer()
            },
            r#"" onload="alert(1)"#,
            "t",
            None,
        );
        // The assertion has to be about a TAG being formed, not about the payload text
        // appearing: `onerror=alert(1)` contains nothing HTML-special, so it survives
        // escaping as literal characters and is entirely harmless there. What must not
        // survive is the `<` in front of it, and the `"` that would close an attribute.
        assert!(!page.contains("<script"), "{page}");
        assert!(!page.contains("<img"), "{page}");
        assert!(!page.contains("<b>gast</b>"), "{page}");
        assert!(!page.contains(r#"onload="alert"#), "{page}");
        // …and they are all still THERE, escaped, rather than the test passing because
        // nothing was rendered at all.
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{page}"
        );
        assert!(page.contains("&quot;&gt;&lt;img"), "{page}");
        assert_eq!(escape(r#"<a href="x">"#), "&lt;a href=&quot;x&quot;&gt;");
    }

    #[test]
    fn the_refusal_page_names_every_possibility_so_it_names_none_of_them() {
        let page = render_refusal();
        for word in ["benutzt", "zurückgezogen", "abgelaufen", "unvollständig"] {
            assert!(
                page.contains(word),
                "the refusal must not narrow it down: {page}"
            );
        }
        // It carries nothing per-request, which is what makes two refusals identical.
        assert!(!page.contains("csrf"), "{page}");
        assert!(!page.contains("<form"), "{page}");
    }

    #[test]
    fn the_page_loads_nothing_from_anywhere() {
        // A page somebody types a new password into must not be one a third party can
        // change, or one that tells a third party it was opened.
        let page = render(&offer(), "abc", "t", None);
        assert!(!page.contains("http://"), "{page}");
        assert!(!page.contains("https://"), "{page}");
        assert!(!page.contains("<script"), "{page}");
    }

    #[test]
    fn a_password_error_becomes_the_notice_that_says_what_to_do_about_it() {
        assert_eq!(Notice::from(PasswordError::TooShort), Notice::TooShort);
        assert_eq!(
            Notice::from(PasswordError::Breached { times: 5 }),
            Notice::Breached
        );
        assert!(Notice::TooShort.message().contains("12 Zeichen"));
        assert!(Notice::Breached.message().contains("Datenleck"));
        // And no notice ever quotes the password or names a token state.
        for notice in [
            Notice::Stale,
            Notice::NameRequired,
            Notice::TooShort,
            Notice::Breached,
            Notice::UsernameTaken,
        ] {
            assert!(!notice.message().contains("Token"));
        }
    }

    #[test]
    fn every_permission_has_a_word_somebody_outside_this_project_would_understand() {
        assert_eq!(permission_word(Permission::Read), "Lesezugriff");
        assert_eq!(permission_word(Permission::Comment), "Kommentarzugriff");
        assert_eq!(permission_word(Permission::Write), "Schreibzugriff");
        assert_eq!(permission_word(Permission::Admin), "Verwaltungszugriff");
    }

    #[test]
    fn the_token_comparison_is_exact() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("abc", ""));
        assert!(!constant_time_eq("", "abc"));
    }
}
