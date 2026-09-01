//! How many queries `Store::graph_for` issues, counted rather than assumed.
//!
//! A review of Task 7 flagged the shape: `Store::backlinks_for` asks `document_for` per
//! candidate, which is three queries, and that is perfectly fine for one page's backlinks.
//! `graph_for` runs the same shape over the WHOLE corpus, so asking per *edge* would be N+1
//! across everything — a page at both ends of forty edges authorised eighty times, on the
//! one screen that touches every document at once.
//!
//! **Its own test binary, and that is load-bearing.** sqlx reports each executed statement
//! as a `tracing` event, and the SQLite driver executes on a per-connection worker thread —
//! so the event does NOT arrive on the thread that awaited the query, and a thread-local
//! counter reads zero (measured: 145 statements counted in a shared atomic against 0 in a
//! thread-local, on the same run). Counting therefore has to be process-wide, and a
//! process-wide counter can only be trusted where nothing else in the process is querying.
//! Cargo gives each integration-test file its own binary; this one holds exactly one test.
//!
//! Two more things this file was taught the hard way, both of which made the count zero
//! while every assertion passed:
//!
//! - `tracing::subscriber::set_default` is thread-scoped, and the events arrive on a
//!   thread it was never installed on. The subscriber must be the GLOBAL default.
//! - a counter that counts nothing satisfies every upper bound, so the first assertion
//!   below is that the count is not zero.

use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, BlockKind, DocumentType, Mark, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing_subscriber::layer::SubscriberExt;

/// Statements executed anywhere in this process since the last reset.
static QUERIES: AtomicUsize = AtomicUsize::new(0);

/// Counts the `sqlx::query` events. sqlx emits exactly one per executed statement
/// (`sqlx-core`'s `QueryLogger`), so this is a query count and not a proxy for one.
struct CountQueries;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CountQueries {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target().starts_with("sqlx::query") {
            QUERIES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn empty_body() -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

/// One paragraph per `href`, each carrying a link mark.
fn body_linking_to(hrefs: &[String]) -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: hrefs
            .iter()
            .map(|href| Block {
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                content: vec![Block {
                    kind: BlockKind::Text,
                    attrs: Default::default(),
                    content: Vec::new(),
                    text: Some("siehe dort".into()),
                    marks: vec![Mark::link_to_url(href)],
                }],
                text: None,
                marks: Vec::new(),
            })
            .collect(),
        text: None,
        marks: Vec::new(),
    }
}

#[tokio::test]
async fn the_permission_question_is_asked_once_per_document_not_once_per_edge() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(CountQueries))
        .expect("nothing else in this binary installs a subscriber");

    // Eight pages, every ordered pair a link: 8 documents against 56 edges, which is far
    // enough apart that per-document and per-edge cannot be confused for one another.
    const PAGES: usize = 8;
    let store = Store::open("sqlite::memory:").await.unwrap();
    let chef = Principal::test("chef", &[], &[]);

    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for i in 0..PAGES {
        let path = format!("/n{i}");
        ids.push(
            store
                .create_document(
                    Author::Import,
                    &NewDocument {
                        parent_path: None,
                        doc_type: DocumentType::Page,
                        title: format!("N{i}"),
                        slug: Some(format!("n{i}")),
                        language: "de".into(),
                        visibility: Visibility::Public,
                        body: empty_body(),
                        sort_key: 0,
                        topics: Vec::new(),
                    },
                    None,
                )
                .await
                .unwrap(),
        );
        store
            .add_grant(
                &path,
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        paths.push(path);
    }
    // Published rather than inserted straight into `links`, so the rows under test are the
    // ones a publish actually writes.
    for (i, id) in ids.iter().enumerate() {
        let targets: Vec<String> = paths
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, p)| p.clone())
            .collect();
        store
            .publish_revision(&chef, id, &body_linking_to(&targets), None)
            .await
            .unwrap()
            .expect("the publish was refused");
    }

    let leser = Principal::test("leser", &[], &[]);
    // After the fixture, so its statements are not counted.
    QUERIES.store(0, Ordering::Relaxed);
    let graph = store.graph_for(&leser, None).await.unwrap();
    let queries = QUERIES.load(Ordering::Relaxed);

    // Correct first. A cheap wrong answer is not the property under test.
    assert_eq!(graph.nodes.len(), PAGES, "{:?}", graph.nodes);
    assert_eq!(
        graph.edges.len(),
        PAGES * (PAGES - 1),
        "{}",
        graph.edges.len()
    );

    assert!(
        queries > 0,
        "the query counter counted nothing, so the bounds below prove nothing"
    );
    assert!(
        queries < graph.edges.len(),
        "{queries} queries for {} edges — that is per-edge work",
        graph.edges.len()
    );
    // The shape it should have: one query for the candidate join, one for the baseline
    // hoisted out of the walk, and `document_for`'s own two per document. The slack is for
    // an ancestor walk deeper than this fixture's, not for another query per document.
    assert!(
        queries <= 3 * graph.nodes.len() + 5,
        "{queries} queries for {} documents — the per-document cost has grown",
        graph.nodes.len()
    );
}
