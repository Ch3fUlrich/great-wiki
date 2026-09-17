//! Reading one page, and **the rule every path-keyed endpoint in this API refuses by**.
//!
//! [`withheld_or_absent`] lives here because this is the module whose comment used to argue
//! the opposite, and the argument is worth keeping beside the thing that replaced it.
//!
//! **What this used to do, and why it was right once.** Existence was checked before
//! permission, so a path that was not there answered 404 and a page the caller may not read
//! answered 403. The reasoning, written out at the time: *collapsing both to 404 would hide
//! configuration mistakes behind a status code that says "you spelled it wrong"; collapsing
//! both to 403 would confirm the existence of every path somebody guesses.* Every derived
//! endpoint — backlinks, history, attachments, the per-page task list — copied it by name.
//!
//! **Why it stopped being right.** That was decided when this wiki had exactly one person in
//! it, and the only caller who could meet a 403 was the person who could also fix it. The
//! invitation flow put a second person in — see `docs/operations/invite-walkthrough-2026-09-17.md`,
//! which walked it and found the consequence: the pair of status codes is an existence
//! oracle, and a signed-in relative could enumerate which pages exist and are being kept from
//! them by guessing addresses. Addresses are guessable words. The **titles** are what the
//! grants are hiding, and a title is one grant away from a path that is known to exist.
//!
//! **What it does now.** A refusal may only tell itself apart from "absent" for somebody who
//! may already read the page. The configuration mistake stays diagnosable, and better than a
//! status code ever made it: whoever administers the path can read what is at it, so they are
//! shown the page rather than a refusal. See ADR 0022, and `crates/gw-api/tests/withheld.rs`
//! for the fence.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::Json;
use axum_extra::extract::CookieJar;
use gw_auth::{Action, Principal};
use gw_core::Block;
use gw_store::{Embed, Reference, StoredDocument};
use serde::Serialize;
use std::collections::BTreeMap;

/// Which refusal a **path-keyed** request has earned, once the operation on `path` has
/// already been refused: 403 if the caller may read the page, 404 otherwise — and 404 for a
/// path where there is nothing at all.
///
/// **One rule, in one place.** Every path-keyed handler in this crate ends its failing branch
/// here rather than deciding for itself; a copy per handler is how the four endpoints the
/// walkthrough found came to disagree with `/api/topics/tagged/`, which had answered 404 for
/// both all along. The module comment above has the history.
///
/// **Why "may read" is the whole test.** The decision (roadmap, 2026-09-17) is *"404 for both
/// to anyone who is not an admin on the path; an admin still sees 403"*. Administering a path
/// implies being able to read what is at it — `Permission::Admin` satisfies `Action::Read` in
/// `gw_auth::can`, and `Baseline::Admin` widens every restricted read in
/// `gw_store::acl::permits` — so `super::admin::path_admin` would answer `Ok` for nobody this
/// function has not already answered `Forbidden` for. Asking it as well would be a second
/// authorisation decision that can never change an outcome, which is the one kind of code
/// this crate refuses to keep: it cannot be tested, so it cannot be known to be right.
///
/// Reading is also the *sufficient* condition, not merely the necessary one, and that is what
/// keeps the endpoints needing **write** honest. Somebody who may read a page but not change
/// it meets a refusal legitimately, and answering "there is no such page" to a reader who is
/// looking at it would be a lie the interface then has to relay.
///
/// **There is no existence check here**, deliberately. `document_for` answers `None` for an
/// absent path and for a withheld one alike, and both leave through the same
/// [`ApiError::NotFound`] — so the two cannot come out of this function differing by a header,
/// a length or a byte, whatever a caller does downstream. A store failure is reported as
/// itself rather than swallowed into a 404, because "the database is down" is not a statement
/// about which pages exist.
pub(crate) async fn withheld_or_absent(
    state: &AppState,
    principal: &Principal,
    path: &str,
) -> ApiError {
    match state
        .store
        .document_for(principal, path, Action::Read)
        .await
    {
        Ok(Some(_)) => ApiError::Forbidden,
        Ok(None) => ApiError::NotFound,
        Err(error) => ApiError::Internal(error),
    }
}

/// One page, as somebody reading it is given it: the stored document, and the one thing
/// about the *caller's own* rights that an interface has to know before it offers a control.
///
/// The document's own fields are flattened rather than nested, so this is `StoredDocument`
/// plus one key and every client that already reads a page keeps working. `may_write` is
/// declared here, on this crate's wire type, rather than on the store's row type: a column
/// added to `documents` must not appear on the API by itself, which is the division
/// [`super::tasks::ProjectView`] makes for the same reason.
#[derive(Debug, Serialize)]
pub struct DocumentView {
    #[serde(flatten)]
    pub document: StoredDocument,
    /// Whether the caller may **write** this page.
    ///
    /// Not computed here. It is [`gw_store::DocumentAccess::may_write`], produced by the
    /// very authorisation that let this response exist — the same `permits()` verdict a
    /// write to this page goes through — so the answer an interface offers a control on and
    /// the answer that refuses it afterwards are one answer. A check written in this handler
    /// would be the second one, and the second one is always the one that gets it wrong.
    ///
    /// **What it licenses**: opening the editor and saving what is typed, making the page a
    /// project's home, and changing or throwing away a card the page governs. **Filing a
    /// revision needs one thing more** — a signed-in, active account, because a revision
    /// records an author — so a control that publishes or restores composes this with
    /// `authenticated` from `/api/me`. See [`gw_store::DocumentAccess::may_write`] and
    /// ADR 0010.
    ///
    /// Asserted only about a page the caller may already read: a refused read is a 404 that
    /// carries the same body a page which does not exist carries, so this discloses nothing
    /// about pages they cannot see — not the bit, and not that there is a page to hold it.
    pub may_write: bool,
    /// Where every `dok:` reference in this body points **for this caller**, keyed by the
    /// document id the body names — and nothing about the ones it does not point at.
    ///
    /// D-5 stores a link's target as an id precisely so that the path and the title can be
    /// resolved when the page is *read* rather than baked in when it is written; this is
    /// that resolution arriving with the page that needs it, so a reader gets the target's
    /// address and name in the first response and with JavaScript switched off.
    ///
    /// **Not computed here.** [`gw_store::Store::references_for`] is the whole rule — which
    /// ids resolve, which silently do not, and what a missing entry means — for the reason
    /// `may_write` is not computed here either: a second answer to a permission question is
    /// always the one that gets it wrong. An id absent from this map is one this caller may
    /// not read, one that is in the Papierkorb, one that was purged, one that never existed,
    /// or one past the per-page cap — deliberately indistinguishable, and rendered as the
    /// reference's own text with no address and no title. ADR 0019.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, Reference>,
    /// What every embed in this body may show **this caller**, keyed by the block's own
    /// target-and-section key (`gw_core::Block::embed_key`).
    ///
    /// An embed stores which page and which section and nothing else (D-27), so the words
    /// inside the frame are resolved here, at read time, against the person reading — which
    /// is what keeps a frame from going on showing somebody a page they lost access to.
    ///
    /// **Not computed here.** [`gw_store::Store::embeds_for`] is the whole rule, for
    /// `references`' reason. An embed absent from this map names a page this caller may not
    /// read, one in the Papierkorb, one that was purged, one that never existed, or one past
    /// the per-page cap — deliberately indistinguishable, and rendered as the author's own
    /// label. An entry whose `body` is absent is D-29's orphan: the anchored heading is gone,
    /// and the frame says so and links the source. ADR 0020.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub embeds: BTreeMap<String, Embed>,
}

/// One document, if the caller may read it.
///
/// The handler makes no authorisation decision of its own. It asks the store for the
/// document *as this principal*, and the store consults the permission engine; there is no
/// unfiltered variant it could reach for by mistake.
///
/// **This endpoint cannot answer 403 any more, and that is the point.** The action it needs
/// is `Read`, so [`withheld_or_absent`]'s one condition is exactly the one that just failed:
/// a refused read is 404, indistinguishable from a page that was never there. Whoever
/// administers the path reads the page and is given it.
pub async fn get_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<DocumentView>, ApiError> {
    let principal = state.principal(&jar).await;
    // Paths are stored with a leading slash; the route captures without one.
    let full = format!("/{}", path.trim_start_matches('/'));

    // `document_access` rather than `document_for`: same accessor, same decision, one field
    // further. The read this handler already performs is what produces the write verdict, so
    // there is no second query and no second answer.
    //
    // No existence check precedes it. `document_access` answers `None` for an absent path and
    // for a withheld one alike, and both leave as the same 404 — see [`withheld_or_absent`].
    let Some(access) = state
        .store
        .document_access(&principal, &full, Action::Read)
        .await
        .map_err(ApiError::Internal)?
    else {
        return Err(withheld_or_absent(&state, &principal, &full).await);
    };

    // Resolved against the CALLER, never against the author who wrote the reference. The
    // body is parsed rather than scanned as text because a `doc` mark is a structure, and
    // because `references_for` takes the tree the reader will be given. A body that will not
    // parse is not this handler's to repair: it answers with no references, so the page
    // still renders and its references fall through to plain text, which is the same state
    // an unreadable target is in.
    let (references, embeds) = match serde_json::from_str::<Block>(&access.document.body) {
        Ok(body) => (
            state
                .store
                .references_for(&principal, &body)
                .await
                .map_err(ApiError::Internal)?,
            // The host document's own id, because an embed of the page it is written on —
            // and any ring of embeds leading back to it — is a cycle, which is stopped at
            // render and named rather than drawn.
            state
                .store
                .embeds_for(&principal, &access.document.id, &body)
                .await
                .map_err(ApiError::Internal)?,
        ),
        Err(error) => {
            tracing::warn!(
                target: "gw_api::docs",
                path = %full,
                %error,
                "a stored body would not parse; its references are not resolved"
            );
            (BTreeMap::new(), BTreeMap::new())
        }
    };

    Ok(Json(DocumentView {
        document: access.document,
        may_write: access.may_write,
        references,
        embeds,
    }))
}
