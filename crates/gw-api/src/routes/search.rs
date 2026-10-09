//! Search over HTTP: pages, topics and tasks matching some words.
//!
//! # This module makes no permission decision, and that is the whole design
//!
//! `gw_store::Store::search_for` decides every one of them: each page candidate the index
//! yields is put through the permission-checked accessor before it can be a hit, and topics
//! and tasks come from the same filtered views the topic index and the boards are (ADR 0024,
//! architecture rule 2). The handler turns a request into a principal and a string, and the
//! answer into JSON. A filter applied here as well would be a second place for the property
//! to be wrong, and the handler's is the one that always is.
//!
//! # A search box is an existence oracle unless it is built not to be
//!
//! Three ways it could become one, and what rules each out:
//!
//! * **By counting.** A total, "12 results" over a list of 9, "3 more you may not see" — each
//!   says that pages exist. The response has no such key
//!   (`the_response_has_no_key_that_could_count_what_it_hid`, and the store's twin test), and
//!   the store stops at the limit of *visible* hits rather than measuring.
//! * **By telling queries apart.** A query that matches only a page the caller may not read
//!   is answered byte for byte as one that matches nothing — status, headers and body — and
//!   so are a blank query, an over-long one and one made of nothing but punctuation. All are
//!   `200` with three empty lists, never `400`: a `400` for "unsearchable" would tell a
//!   prober which of its queries the server parsed as something.
//!   `a_query_matching_only_a_withheld_page_is_answered_exactly_as_one_matching_nothing`
//!   asserts it on the bytes.
//! * **By echoing.** The query is never put in the response, so there is nothing for a
//!   header or a body to differ by.
//!
//! The query is read from the raw query string rather than through a typed extractor: a
//! malformed one (a bad percent-escape, a repeated `q`, no `q`) must be the empty answer,
//! not an extractor's `400`.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{RawQuery, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_store::SearchResults;

/// Hits per group. A search box shows the first few; paging is not offered, so there is no
/// offset to walk a result set with.
const HITS_PER_GROUP: usize = 20;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/search", get(search))
}

/// The value of the first `q` parameter, lossily decoded, or empty.
fn query_of(raw: Option<&str>) -> String {
    raw.into_iter()
        .flat_map(|raw| url::form_urlencoded::parse(raw.as_bytes()))
        .find(|(key, _)| key == "q")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

/// Pages, topics and tasks the caller may see that match `?q=`.
///
/// Anonymous callers are allowed: the accessor answers for them what it answers for anyone
/// else, which is the public pages and nothing more.
pub async fn search(
    State(state): State<AppState>,
    jar: CookieJar,
    RawQuery(raw): RawQuery,
) -> Result<Json<SearchResults>, ApiError> {
    let principal = state.principal(&jar).await;
    let results = state
        .store
        .search_for(&principal, &query_of(raw.as_deref()), HITS_PER_GROUP)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(results))
}

#[cfg(test)]
mod tests {
    use super::query_of;

    #[test]
    fn the_first_q_is_used_and_garbage_is_empty() {
        assert_eq!(query_of(Some("q=Darm+Polypen")), "Darm Polypen");
        assert_eq!(query_of(Some("x=1&q=a%20b&q=c")), "a b");
        assert_eq!(query_of(Some("q=%FF%FE")), "\u{FFFD}\u{FFFD}");
        assert_eq!(query_of(Some("q=%")), "%");
        assert_eq!(query_of(Some("other=1")), "");
        assert_eq!(query_of(None), "");
    }
}
