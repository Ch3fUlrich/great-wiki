use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::Json;
use axum_extra::extract::CookieJar;
use gw_auth::Action;
use gw_store::StoredDocument;
use serde::Serialize;

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
    /// Asserted only about a page the caller may already read: a refused read is a 403 and
    /// carries no body at all, so this discloses nothing about pages they cannot see.
    pub may_write: bool,
}

/// One document, if the caller may read it.
///
/// The handler makes no authorisation decision of its own. It asks the store for the
/// document *as this principal*, and the store consults the permission engine; there is no
/// unfiltered variant it could reach for by mistake.
pub async fn get_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<DocumentView>, ApiError> {
    let principal = state.principal(&jar).await;
    // Paths are stored with a leading slash; the route captures without one.
    let full = format!("/{}", path.trim_start_matches('/'));

    // Existence is checked first, so an absent path is 404 and a forbidden one is 403.
    // `document_for` returns `None` for both, deliberately — it is this layer that decides
    // which to reveal. Collapsing both to 404 would hide configuration mistakes behind a
    // status code that says "you spelled it wrong"; collapsing both to 403 would confirm
    // the existence of every path somebody guesses. `document_exists` answers exactly the
    // one bit needed to choose, and nothing about the document itself.
    if !state
        .store
        .document_exists(&full)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }

    // `document_access` rather than `document_for`: same accessor, same decision, one field
    // further. The read this handler already performs is what produces the write verdict, so
    // there is no second query and no second answer.
    state
        .store
        .document_access(&principal, &full, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .map(|access| {
            Json(DocumentView {
                document: access.document,
                may_write: access.may_write,
            })
        })
        .ok_or(ApiError::Forbidden)
}
