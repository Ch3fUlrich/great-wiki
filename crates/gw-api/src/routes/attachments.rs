//! Attachments over HTTP: the `Anhänge` list, the upload, and the download.
//!
//! # Every address goes through a page, and none of them carries a digest (D-16)
//!
//! ```text
//! GET    /api/attachments/{page}                 the list
//! GET    /api/attachment/{filename}/{page}       the bytes
//! POST   /api/attachment/{filename}/{page}       attach
//! DELETE /api/attachment/{filename}/{page}       detach
//! ```
//!
//! **There is no `/blob/{sha}` and there must never be one.** D-16 makes a download
//! authorised against the page it was reached through, and that is only true while the page
//! is part of the address: an address that named the bytes would be reachable without a page,
//! so the permission check would be something a caller could route around rather than
//! something they have to pass. The digest exists on
//! [`gw_store::AttachmentSource`], which never leaves this process — no handler here
//! serialises one, and `no_response_anywhere_carries_the_content_address` in
//! `tests/attachments.rs` is what keeps it that way when somebody adds a field.
//!
//! **The filename comes before the page path**, which reads backwards and is not a choice: a
//! `{*path}` catch-all must be the last segment of a route, so anything else in the address
//! has to precede it.
//!
//! **Singular and plural are two different prefixes on purpose.** `/api/attachments/{*path}`
//! and `/api/attachment/{filename}/{*path}` cannot be one prefix with a literal segment
//! inside it — `/api/attachments/file/{name}/{*path}` would be chosen over the catch-all for
//! a page whose first segment is `file`, which is the shadowing [`super::trash`] and
//! [`super::collab`] both record.
//!
//! # One authorisation, in the store
//!
//! No handler here asks a permission question. `attachments_for`, `attachment_for`, `attach`
//! and `detach` each end in [`gw_store::Store::document_access`] — the same accessor a page
//! read ends in — and the handlers only choose a status code afterwards. That is the rule
//! [`super::topics`] and [`super::trash`] state and it matters more here than anywhere else
//! in the API: a leak on this path does not reveal that a page exists, it hands over the
//! contents.
//!
//! # What a download says about itself
//!
//! The `Content-Type` is what [`gw_store::blobs::sniff`] made of the leading bytes, never
//! what the upload declared — there is no code path that could echo a declared type, because
//! nothing records one. Beside it go three headers that all exist to protect the browser
//! rather than the wiki: `X-Content-Type-Options: nosniff`, so a browser cannot decide the
//! bytes are HTML after all; `Content-Security-Policy: default-src 'none'; sandbox`, so a
//! format that can carry script (a PDF can) renders in an opaque origin with nothing reachable
//! from it; and `Content-Disposition: attachment` for everything that is not an image or a
//! PDF, so an unexpected type is saved rather than rendered.
//!
//! **`image/svg+xml` is `attachment` too, and that is a rule rather than a consequence.** An
//! SVG is an image by media type and a program by capability, and [`content_disposition`]
//! names it before it asks whether anything is an image. The whole of the reasoning, and the
//! constraint it leaves for whoever renders an attachment in the interface, is on that
//! function and in
//! `docs/decisions/0014-what-a-file-has-to-be-to-be-attached.md`.

use super::AppState;
use crate::error::ApiError;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use futures_util::StreamExt;
use gw_auth::Action;
use gw_store::{AttachOutcome, Attachment, DetachOutcome, MAX_ATTACHMENT_BYTES};
use serde::Serialize;
use tower_http::limit::RequestBodyLimitLayer;

/// One file on a page, as a reader is given it.
///
/// Declared here rather than serialising [`gw_store::Attachment`], for the reason
/// [`super::docs::DocumentView`] and [`super::trash::TrashSummaryView`] are: a column added
/// to the store's own row type must not appear on the wire by itself. Here that division is
/// load-bearing rather than tidy — the field that must never appear is the content address,
/// and this type is where "must never" is enforced.
#[derive(Debug, Serialize)]
pub struct AttachmentView {
    pub filename: String,
    /// What the bytes **are**. Never what an upload claimed they were.
    pub media_type: String,
    pub byte_size: i64,
    pub uploaded_at: String,
    pub uploaded_by_name: String,
    /// Where to fetch it: through the page, always.
    ///
    /// Built here rather than by the client, so there is one shape of address and no client
    /// is ever in a position to assemble a different one. It is also the plainest statement
    /// of D-16 that a reader of the API can see — the thing they are handed to fetch a file
    /// contains the page, and does not contain the file.
    pub href: String,
}

impl AttachmentView {
    fn of(attachment: Attachment, page: &str) -> Self {
        let href = format!(
            "/api/attachment/{}{}",
            percent_encode_segment(&attachment.filename),
            page
        );
        Self {
            filename: attachment.filename,
            media_type: attachment.media_type,
            byte_size: attachment.byte_size,
            uploaded_at: attachment.uploaded_at,
            uploaded_by_name: attachment.uploaded_by_name,
            href,
        }
    }
}

/// A page's `Anhänge` list.
#[derive(Debug, Serialize)]
pub struct AttachmentsResponse {
    pub attachments: Vec<AttachmentView>,
    /// Whether this caller may attach and detach here. The store's own write verdict,
    /// carried rather than recomputed (ADR 0010), so the control an interface offers and the
    /// answer that refuses it afterwards are one answer.
    pub may_write: bool,
}

/// Every attachment route, carrying its **own** request-body limit.
///
/// Merged into [`super::build_router`] *after* the ordinary limit and never inside it: layers
/// wrap, so a 2 MB cap applied above this one would refuse every attachment at 2 MB no matter
/// what this said. That is not hypothetical — the limit used to live in `main.rs`, outside
/// the router entirely, where nothing in this crate could have carved an exception out of it
/// and no test would have noticed.
///
/// The limit is [`MAX_ATTACHMENT_BYTES`] itself, so a body that *declares* itself larger is
/// refused before a byte is read. [`gw_store::BlobWriter`] counts what actually arrives as
/// well, which is what catches an upload that declares nothing — and what still refuses if
/// this layer is ever removed.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/attachments/{*path}", get(list))
        .route(
            "/api/attachment/{filename}/{*path}",
            get(download).post(upload).delete(detach),
        )
        .layer(RequestBodyLimitLayer::new(MAX_ATTACHMENT_BYTES as usize))
}

/// Paths are stored with a leading slash; a route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Percent-encode one path segment, so a filename holding a space, a `?`, a `#` or a
/// non-ASCII character survives being put in [`AttachmentView::href`].
///
/// Deliberately narrow: [`gw_store::attachments::canonical_filename`] has already refused
/// `/`, `\`, `"` and every control character, so what is left to escape is what would
/// otherwise end the path or start a query. Everything in the RFC 3986 unreserved set plus
/// the sub-delims that are safe inside a segment is left alone, because a reader sees this
/// address and `Befund%202024.pdf` is easier to recognise than a fully escaped one.
fn percent_encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b'$'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b','
            | b'@'
            | b'+'
            | b'='
            | b':' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Tell 404 from 403 for a page-addressed attachment request.
///
/// The same two questions [`super::docs::get_document`] asks, in the same order and for the
/// same reason: collapsing both to 404 hides configuration mistakes behind a status code that
/// says "you spelled it wrong", and collapsing both to 403 confirms the existence of every
/// path somebody guesses. Reached only after the operation has already been refused, so it
/// costs nothing on the path that succeeds.
async fn absent_or_forbidden(state: &AppState, path: &str) -> ApiError {
    match state.store.document_exists(path).await {
        Ok(false) => ApiError::NotFound,
        Ok(true) => ApiError::Forbidden,
        Err(error) => ApiError::Internal(error),
    }
}

/// The `Anhänge` list of a page. Needs **read** on the page.
pub async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<AttachmentsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    // One call and one authorisation. `may_write` comes back from the same read that
    // permitted the list rather than from a second question this handler asks (ADR 0010).
    let Some(list) = state
        .store
        .attachments_for(&principal, &path)
        .await
        .map_err(ApiError::Internal)?
    else {
        return Err(absent_or_forbidden(&state, &path).await);
    };

    Ok(Json(AttachmentsResponse {
        attachments: list
            .attachments
            .into_iter()
            .map(|attachment| AttachmentView::of(attachment, &path))
            .collect(),
        may_write: list.may_write,
    }))
}

/// The bytes of one attachment. Needs **read on the page it was reached through** (D-16).
///
/// The page is resolved and authorised before the mount is touched, so a caller who may not
/// read the page cannot learn whether the file exists, cannot learn whether the mount is
/// healthy, and cannot make this handler perform any I/O at all.
pub async fn download(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((filename, captured)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    let Some(source) = state
        .store
        .attachment_for(&principal, &path, &filename)
        .await
        .map_err(ApiError::Internal)?
    else {
        // Three ways to get here and they are different answers: no such page (404), a page
        // this caller may not read (403), and a readable page carrying no such file (404).
        // The store conflates the first two, so the page read is asked again — only on the
        // failing path — to tell them apart.
        if state
            .store
            .document_for(&principal, &path, Action::Read)
            .await
            .map_err(ApiError::Internal)?
            .is_some()
        {
            return Err(ApiError::NotFound);
        }
        return Err(absent_or_forbidden(&state, &path).await);
    };

    // Opened before a single header is sent, so "the row is here and the bytes are not" is a
    // status code rather than a truncated body. `open_read` answers `None` for a file that
    // was never there and for one the mount will not hand over — a stale NFS handle, which
    // `/mnt/cloud` really does produce inside a container while the host is fine — and both
    // are 503: the wiki knows this file exists and is failing to serve it, which is not the
    // statement 404 makes.
    let Some(file) = state
        .blobs
        .open_read(&source.sha256)
        .await
        .map_err(ApiError::Internal)?
    else {
        tracing::error!(
            path = %path,
            filename = %source.filename,
            "an attachment's row is in the database and its bytes are not on the mount"
        );
        return Err(ApiError::Unavailable);
    };

    // The one thing that can be checked cheaply before the headers go out: the file is as
    // long as the database says it is. `Content-Length` is sent from the stored size, so a
    // file the mount has truncated would otherwise become a response that simply stops —
    // indistinguishable from a dropped connection, and impossible to notice. Refusing is the
    // closed answer, and it is the same 503 as bytes that are missing entirely, because it is
    // the same statement: the wiki knows about this file and cannot serve it.
    let on_disk = file
        .metadata()
        .await
        .map_err(|error| ApiError::Internal(error.into()))?
        .len();
    if on_disk != source.byte_size as u64 {
        tracing::error!(
            path = %path,
            filename = %source.filename,
            expected = source.byte_size,
            found = on_disk,
            "an attachment's bytes are not the length the database recorded"
        );
        return Err(ApiError::Unavailable);
    }

    // Streamed in chunks rather than read whole: D-17 allows 250 MB per file, and buffering
    // that per concurrent reader is how a wiki with four users falls over.
    let stream = futures_util::stream::unfold(file, |mut file| async move {
        use tokio::io::AsyncReadExt;
        let mut buffer = vec![0u8; 64 * 1024];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(read) => {
                buffer.truncate(read);
                Some((Ok(axum::body::Bytes::from(buffer)), file))
            }
            // The headers have already gone by this point, so there is no status code left to
            // send: the stream ends in an error and the connection is broken, which is what
            // tells the client the body is incomplete.
            Err(error) => Some((Err(error), file)),
        }
    });

    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&source.media_type).map_err(|_| {
            ApiError::Internal(anyhow::anyhow!(
                "stored media type `{}` is not a header value",
                source.media_type
            ))
        })?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&source.byte_size.to_string())
            .expect("a decimal integer is a header value"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        content_disposition(&source.media_type, &source.filename),
    );
    // The browser must not be allowed to decide these bytes are something other than what
    // they were sniffed as — which is the entire value of sniffing them.
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // A PDF can carry script and is served inline. `sandbox` puts it in an opaque origin, so
    // what it can reach is nothing: not this wiki's cookies, not its DOM, not its API.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    // These bytes were authorised against one page for one caller. A shared cache holding
    // them would serve them to the next request without that check, which is exactly the
    // thing D-16 is about.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

/// Whether the browser may render this, and under what name it saves it.
///
/// `inline` for images and PDFs, because D-15 wants a picture beside the paragraph explaining
/// it. `attachment` for everything else — a ZIP, a video, plain text, anything added to the
/// allowlist later — so a type nobody thought about is saved rather than rendered.
///
/// **`image/svg+xml` is the exception, and it is checked before the image rule rather than
/// carved out of it.** An SVG is XML that can carry `<script>`, event handlers and external
/// references: the one image format that is also a program. It is stored exactly as
/// uploaded — `gw_store::blobs` says why nothing sanitises it — so *not being rendered where
/// it was reached* is the defence, and a defence that depends on somebody remembering that
/// SVG is an image would not survive the next type being added. Written as a match whose
/// first arm names it, so the image branch is not even reached.
///
/// The constraint that leaves this file with it: **anything that later renders an attachment
/// inline must not render an SVG through a mechanism that executes it.** `<img src>` and a
/// CSS `background-image` are safe — no browser runs script in either — while `<object>`,
/// `<embed>`, `<iframe>` and inlining the markup into this wiki's own DOM all execute it,
/// and the last of those would execute it *in this origin*, with the session cookie in
/// reach. `attachment` also makes an `<iframe>` pointing here download rather than render,
/// so the disposition is doing work for a page that has not been written yet.
///
/// Both spellings of the name are sent: a quoted ASCII fallback for a client that does not
/// implement RFC 5987, and `filename*=UTF-8''…` for the ones that do, so `Röntgen.png` keeps
/// its umlaut. The fallback replaces every non-ASCII byte rather than dropping it, because a
/// name that collapses to nothing is worse than one that is partly underscores.
///
/// It cannot inject a header: `canonical_filename` has already refused `"`, `\` and every
/// control character, and the encoder below emits nothing else.
fn content_disposition(media_type: &str, filename: &str) -> HeaderValue {
    let inline = match media_type {
        // Never, whatever else is true of it. See the doc comment above.
        "image/svg+xml" => false,
        other => other.starts_with("image/") || other == "application/pdf",
    };
    let disposition = if inline { "inline" } else { "attachment" };
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let value = format!(
        "{disposition}; filename=\"{ascii}\"; filename*=UTF-8''{}",
        percent_encode_segment(filename)
    );
    HeaderValue::from_str(&value).unwrap_or_else(|_| {
        // Unreachable given the refusals above, and closed rather than clever if it is not:
        // a name that will not go in a header is not a reason to serve the file unnamed under
        // a disposition that lets the browser render it.
        HeaderValue::from_static("attachment")
    })
}

/// Attach a file to a page. Needs **write** on the page, and a signed-in, active account.
///
/// The body is the file and nothing else — no multipart, no JSON envelope, no declared type.
/// That is not minimalism: a `Content-Type` in the request is a type the uploader chose, and
/// the only way to be sure it is never echoed back is for there to be nowhere to put it.
/// The name comes from the address, so it is visible in the log line and in the audit row
/// without anything having to parse a body to find it.
///
/// The bytes are read into `tmp/` first and published only if [`gw_store::Store::attach`]
/// says yes — see [`gw_store::PendingBlob`] for why the check cannot come first, and why a
/// refused upload therefore leaves nothing on the mount.
pub async fn upload(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((filename, captured)): Path<(String, String)>,
    body: Body,
) -> Result<(StatusCode, Json<AttachmentView>), ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    let mut writer = state.blobs.writer().map_err(ApiError::Internal)?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        // A body that stops early is either the limit layer cutting it off or a client that
        // hung up. There is no way to tell them apart from here — and no need to: a client
        // that hung up is not reading this response, so the only case anybody observes is the
        // limit, and 413 is what that case means.
        let chunk = chunk.map_err(|error| {
            tracing::debug!(%error, "an upload body ended early");
            ApiError::TooLarge(format!(
                "an attachment may be at most {} MB",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            ))
        })?;
        writer.push(&chunk).await.map_err(ApiError::Internal)?;
    }

    let pending = match writer.finish().await.map_err(ApiError::Internal)? {
        gw_store::BlobOutcome::Accepted(pending) => pending,
        gw_store::BlobOutcome::TooLarge => {
            return Err(ApiError::TooLarge(format!(
                "an attachment may be at most {} MB",
                MAX_ATTACHMENT_BYTES / (1024 * 1024)
            )))
        }
        gw_store::BlobOutcome::UnknownType => {
            return Err(ApiError::Unsupported(
                "this wiki stores images, PDFs, ZIP archives, MP4, WebM or Ogg media, and \
                 UTF-8 text — which is how plain text, Markdown, CSV and SVG get in. The \
                 type is read from the file itself, so renaming it does not help: what is \
                 refused is a file that is neither a format this recognises nor text at all."
                    .into(),
            ))
        }
        gw_store::BlobOutcome::Empty => {
            return Err(ApiError::Invalid("the upload was empty".into()))
        }
    };

    match state
        .store
        .attach(&principal, &path, &filename, pending)
        .await
        .map_err(ApiError::Internal)?
    {
        AttachOutcome::Done(attachment) => Ok((
            StatusCode::CREATED,
            Json(AttachmentView::of(attachment, &path)),
        )),
        AttachOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        AttachOutcome::Refused => Err(absent_or_forbidden(&state, &path).await),
    }
}

/// What one detach removed.
#[derive(Debug, Serialize)]
pub struct DetachedView {
    pub filename: String,
}

/// Take a file off a page. Needs **write** on the page, and a signed-in, active account.
///
/// It removes the row and leaves the bytes. That is D-15 pointed the other way — the list is
/// the authority on what is attached, so letting go of an entry is an edit and not a
/// destruction — and ADR 0013 is where the bytes go, and when.
pub async fn detach(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((filename, captured)): Path<(String, String)>,
) -> Result<Json<DetachedView>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    match state
        .store
        .detach(&principal, &path, &filename)
        .await
        .map_err(ApiError::Internal)?
    {
        DetachOutcome::Done(attachment) => Ok(Json(DetachedView {
            filename: attachment.filename,
        })),
        // Told only to somebody who may already write the page, so it confirms nothing they
        // could not have listed.
        DetachOutcome::NoSuchFile => Err(ApiError::NotFound),
        DetachOutcome::Refused => Err(absent_or_forbidden(&state, &path).await),
    }
}

#[cfg(test)]
mod tests {
    use super::{content_disposition, percent_encode_segment};

    #[test]
    fn a_name_survives_being_put_in_an_address() {
        assert_eq!(percent_encode_segment("befund.png"), "befund.png");
        assert_eq!(
            percent_encode_segment("Befund 2024.pdf"),
            "Befund%202024.pdf"
        );
        assert_eq!(percent_encode_segment("a?b#c"), "a%3Fb%23c");
        // Two bytes in UTF-8, and both of them escaped.
        assert_eq!(percent_encode_segment("Röntgen"), "R%C3%B6ntgen");
    }

    #[test]
    fn only_pictures_and_pdfs_are_offered_inline() {
        for (media_type, expected) in [
            ("image/png", "inline"),
            ("image/webp", "inline"),
            ("application/pdf", "inline"),
            ("application/zip", "attachment"),
            ("video/mp4", "attachment"),
            ("audio/ogg", "attachment"),
            // The exception, and the only one: an image type that is never rendered.
            ("image/svg+xml", "attachment"),
            ("text/plain; charset=utf-8", "attachment"),
        ] {
            let header = content_disposition(media_type, "x.bin");
            assert!(
                header.to_str().unwrap().starts_with(expected),
                "{media_type}: {header:?}"
            );
        }
    }

    #[test]
    fn a_name_reaches_the_header_in_both_spellings() {
        let header = content_disposition("image/png", "Röntgen – links.png");
        let value = header.to_str().unwrap();
        assert!(
            value.contains(r#"filename="R_ntgen _ links.png""#),
            "an ASCII fallback that is still recognisable: {value}"
        );
        assert!(
            value.contains("filename*=UTF-8''R%C3%B6ntgen%20%E2%80%93%20links.png"),
            "and the real one: {value}"
        );
    }
}
