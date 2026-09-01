//! How many queries `Store::board_for` issues, counted rather than assumed.
//!
//! A board is the widest aggregate this system has: every card the caller may see, across
//! every project and the ones filed under none. It already asks its permission question once
//! per *page* rather than once per card — `Store::board_for` memoises what the accessor
//! answered — and `Task::may_write` is carried out of that same answer. This file is what
//! stops that becoming untrue: the tempting way to add a write verdict to a card is to ask
//! "may I write this one?" per row, which reads like one line and is N+1 across the whole
//! corpus.
//!
//! **Its own test binary, and that is load-bearing** — for the reason `graph_query_count.rs`
//! records in full: sqlx reports each statement as a `tracing` event on the SQLite driver's
//! own worker thread, so the counter has to be process-wide and globally installed, and a
//! process-wide counter can only be trusted where nothing else in the process is querying.
//! Cargo gives each integration-test file its own binary; this one holds exactly one test.
//!
//! And the same two traps, both of which made the count zero while every assertion passed:
//! the subscriber must be the GLOBAL default, and the first assertion has to be that the
//! count is not zero — a counter that counts nothing satisfies every upper bound.

use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, BlockKind, DocumentType, Visibility};
use gw_store::{Author, NewDocument, NewTask, Store, TaskHome, TaskOutcome, TaskStatus};
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

async fn page(store: &Store, parent: Option<&str>, title: &str, slug: &str) -> String {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(str::to_string),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: Visibility::Restricted,
                body: empty_body(),
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn the_write_verdict_is_asked_once_per_page_not_once_per_card() {
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(CountQueries))
        .expect("nothing else in this binary installs a subscriber");

    // Four pages carrying twelve cards each: 4 documents against 48 cards, which is far
    // enough apart that per-document and per-card cannot be confused for one another.
    const PAGES: usize = 4;
    const CARDS_PER_PAGE: usize = 12;

    let store = Store::open("sqlite::memory:").await.unwrap();
    let chef = store
        .create_local_principal("chef", "Chef", None, "irrelevanter-hash")
        .await
        .unwrap();

    page(&store, None, "Projekt", "projekt").await;
    store
        .add_grant(
            "/projekt",
            Subject::Principal(chef.id.clone()),
            Permission::Write,
        )
        .await
        .unwrap();
    store
        .create_project(&chef, "/projekt", None)
        .await
        .unwrap()
        .expect("the fixture's project was refused");

    for p in 0..PAGES {
        let doc = page(&store, Some("/projekt"), &format!("S{p}"), &format!("s{p}")).await;
        for c in 0..CARDS_PER_PAGE {
            let outcome = store
                .create_task(
                    &chef,
                    &NewTask {
                        home: TaskHome::Anchored {
                            doc_id: doc.clone(),
                            block_id: None,
                        },
                        title: format!("K{p}-{c}"),
                        status: TaskStatus::Offen,
                        assignee: None,
                        due_at: None,
                        position: c as i64,
                    },
                )
                .await
                .unwrap();
            assert!(
                matches!(outcome, TaskOutcome::Done(_)),
                "the fixture's own card was refused: {outcome:?}"
            );
        }
    }

    // After the fixture, so its statements are not counted.
    QUERIES.store(0, Ordering::Relaxed);
    let board = store.board_for(&chef, None).await.unwrap();
    let queries = QUERIES.load(Ordering::Relaxed);

    // Correct first. A cheap wrong answer is not the property under test — and the bit has
    // to be on every card, or "it costs nothing" would be true of nothing.
    assert_eq!(board.len(), PAGES * CARDS_PER_PAGE, "{}", board.len());
    assert!(
        board.iter().all(|task| task.may_write),
        "a card that its own author may move came back read-only"
    );

    assert!(
        queries > 0,
        "the query counter counted nothing, so the bounds below prove nothing"
    );
    // The shape it should have: one for the baseline, one for the candidate join, one row
    // read per card — that much is per-card already and is not what this test is about — and
    // the authorisation once per page. So anything beyond one query per card has to fit in a
    // small multiple of the PAGE count, and a verdict asked per card would not.
    assert!(
        queries <= board.len() + 4 * PAGES + 6,
        "{queries} queries for {} cards over {PAGES} pages — the per-card cost has grown \
         beyond reading the row",
        board.len()
    );
    // Anonymous is refused every page, so nothing is emitted — and the cost of saying so is
    // still per page rather than per card, because the accessor is what omits a card.
    QUERIES.store(0, Ordering::Relaxed);
    let nothing = store
        .board_for(&Principal::anonymous(), None)
        .await
        .unwrap();
    let refusals = QUERIES.load(Ordering::Relaxed);
    assert!(nothing.is_empty(), "{nothing:#?}");
    assert!(
        refusals > 0 && refusals <= 4 * PAGES + 6,
        "{refusals} queries to refuse {} cards over {PAGES} pages",
        PAGES * CARDS_PER_PAGE
    );
}
