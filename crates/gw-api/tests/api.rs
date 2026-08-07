use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

async fn seed() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    // Annotated because `body` is otherwise only constrained by `Serialize`, which is
    // ambiguous. It is round-tripped below to give each document its own owned copy.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap();

    for (title, vis) in [
        ("Öffentlich", Visibility::Public),
        ("Geheim", Visibility::Restricted),
    ] {
        store
            .insert_document(&NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: None,
                language: "de".into(),
                visibility: vis,
                body: serde_json::from_value(serde_json::to_value(&body).unwrap()).unwrap(),
                sort_key: 0,
            })
            .await
            .unwrap();
    }
    Arc::new(store)
}

fn app(store: Arc<Store>, dev: Option<gw_api::Identity>) -> axum::Router {
    gw_api::build_router(gw_api::AppState::for_test(store, dev))
}

async fn get(app: axum::Router, uri: &str) -> StatusCode {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn health_is_ok() {
    assert_eq!(
        get(app(seed().await, None), "/api/health").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn public_document_is_readable_anonymously() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn restricted_document_is_forbidden_anonymously() {
    // 403 not 404: the proxy already knows the path exists, and a misleading 404 makes
    // debugging an authentication problem needlessly hard.
    assert_eq!(
        get(app(seed().await, None), "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn restricted_document_is_readable_when_authenticated() {
    let identity = gw_api::Identity::dev("sergej", &["admins"]);
    assert_eq!(
        get(app(seed().await, Some(identity)), "/api/documents/geheim").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn absent_document_is_not_found() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/gibt-es-nicht").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn anonymous_tree_omits_restricted_documents() {
    let response = app(seed().await, None)
        .oneshot(
            Request::builder()
                .uri("/api/tree")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("Öffentlich"));
    // A restricted title leaking into the navigation is a disclosure even without the body.
    assert!(
        !text.contains("Geheim"),
        "restricted titles must not appear in the tree"
    );
}
