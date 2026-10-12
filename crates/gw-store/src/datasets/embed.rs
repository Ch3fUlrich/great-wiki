//! The read a `dataset-view` block makes (ADR 0029 §8, spec §6): which dataset page and
//! which saved view a block on some page names, answered for one caller.
//!
//! **The permission is the dataset's own, never the embedding page's.** A block in a page
//! the whole wiki can read says only that somebody put an address there; the data behind it
//! belongs to the dataset's ACL. So this function is handed the two ids and nothing else —
//! not the embedding page, not its path, not its grants — and the one question it asks is
//! [`Store::document_for_id`] with `Action::Read`, the same accessor every other dataset
//! read ends in.
//!
//! What is not here matters as much: no rows and no count. The web loads rows through the
//! row endpoint under the caller's own identity, so this answer cannot become a second,
//! differently-filtered way onto the data. A view's config names field keys, and the ones
//! hidden from this caller come back stripped by the same `strip_hidden` the view list uses,
//! so the two cannot drift.
//!
//! Every refusal is one answer (ADR 0022): an absent id, a page that is not a dataset, a
//! page in the trash, a page the caller may not read, and a string that is not shaped like
//! an id at all are indistinguishable, so probing the endpoint says nothing about which of
//! them a wiki holds. A distinguishable `ViewMissing` is reached only behind the dataset's
//! own read gate.

use super::views::strip_hidden;
use super::DatasetView;
use crate::Store;
use anyhow::Result;
use gw_auth::Action;
use gw_auth::Principal;
use gw_core::DocumentType;
use serde::Serialize;

/// How an embed read ended. `Unavailable` is the uniform refusal and carries nothing at
/// all, so its serialisation is byte-identical for every reason behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EmbedOutcome {
    /// The caller may read this dataset, and the view is one of its saved views.
    Ok {
        path: String,
        title: String,
        view: DatasetView,
    },
    /// The caller may read this dataset, but this view is not one of its views.
    ViewMissing,
    /// Absent, not a dataset, trashed, unreadable, or not an id. One answer, no detail.
    Unavailable,
}

impl Store {
    /// What the `dataset-view` block naming `dataset_doc` and `view` may show `principal`.
    ///
    /// The embedding page is not a parameter, because its permission decides nothing here.
    pub async fn dataset_embed(
        &self,
        principal: &Principal,
        dataset_doc: &str,
        view: &str,
    ) -> Result<EmbedOutcome> {
        // Shaped like an id at all, before anything is asked of the database: a string the
        // format could not write back is not a probe about a page, it is a bad request, and
        // it must not be answerable as anything but the uniform refusal. One rule, borrowed
        // from the syntax that mints these blocks, rather than a second restatement of it.
        if gw_core::markdown::dataset_view_destination(dataset_doc, view).is_none() {
            return Ok(EmbedOutcome::Unavailable);
        }
        // The dataset's own read gate — the single line that decides everything below, and
        // the one `mutate.sh` breaks. Absent, not a dataset, thrown away and unreadable are
        // one `None` here, which is why the refusals cannot be told apart.
        let found = self
            .document_for_id(principal, dataset_doc, Action::Read)
            .await?;
        let Some(doc) = found else {
            return Ok(EmbedOutcome::Unavailable);
        };
        if doc.doc_type != DocumentType::Dataset.as_str() {
            return Ok(EmbedOutcome::Unavailable);
        }
        // Confined to this dataset's views, so a view id of some other page is not reachable
        // by asking for it here. Telling a reader that their view does not exist is safe;
        // telling anybody else is not, and they are above, refused already.
        let Some(mut v) = self
            .views_of(&doc.id)
            .await?
            .into_iter()
            .find(|v| v.id == view)
        else {
            return Ok(EmbedOutcome::ViewMissing);
        };
        let visible = self.visible_field_keys(principal, &doc.id).await?;
        strip_hidden(&v.kind, &mut v.config, &visible);
        Ok(EmbedOutcome::Ok {
            path: doc.path,
            title: doc.title,
            view: v,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::{NewField, SchemaOutcome};
    use crate::documents::NewDocument;
    use crate::revisions::Author;
    use crate::{CreateOutcome, CreateRequest};
    use gw_auth::{Permission, Subject};
    use gw_core::{Block, BlockKind, FieldKind, Visibility};
    use serde_json::json;

    /// Everything a test asks about, by id.
    struct Fixture {
        store: Store,
        anna: Principal,
        bea: Principal,
        cleo: Principal,
        /// `/raum/tabelle` — read by anna and bea, since bea reads `/raum`.
        tabelle: String,
        /// Its saved view, naming `name`, `alter` and the `mit` relation.
        view: String,
        /// `/geheim/ziel` — anna only.
        ziel: String,
        /// A view of `ziel`, so the embedding page can point at a dataset bea may not read.
        ziel_view: String,
        /// `/raum/seite` — a page, not a dataset.
        plain: String,
        /// `/raum/mull` — trashed below.
        mull: String,
    }

    async fn raw_page(store: &Store, parent: Option<&str>, slug: &str, body: Block) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: slug.into(),
                    slug: Some(slug.into()),
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    fn empty_body() -> Block {
        serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap()
    }

    fn view_block(dataset_doc: &str, view: &str, label: &str) -> serde_json::Value {
        json!({
            "kind": "dataset-view",
            "attrs": {"dataset_doc": dataset_doc, "view": view, "label": label}
        })
    }

    async fn dataset(
        store: &Store,
        anna: &Principal,
        parent: &str,
        title: &str,
    ) -> (String, String) {
        let req = CreateRequest {
            parent: Some(parent.into()),
            title: title.into(),
            slug: None,
            template: None,
        };
        match store
            .create_dataset_for(anna, &req, false, "d")
            .await
            .unwrap()
        {
            CreateOutcome::Created { path, id } => (path, id),
            other => panic!("{other:?}"),
        }
    }

    async fn field(store: &Store, anna: &Principal, path: &str, f: NewField) {
        let out = store.add_dataset_field(anna, path, &f).await.unwrap();
        assert!(matches!(out, SchemaOutcome::Done(_)), "{}: {out:?}", f.key);
    }

    async fn saved_view(
        store: &Store,
        anna: &Principal,
        path: &str,
        config: serde_json::Value,
    ) -> String {
        match store
            .create_dataset_view(anna, path, "Alle", "table", &config)
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v.id,
            other => panic!("{other:?}"),
        }
    }

    /// `/raum` — write `anna`, read `bea`, nothing for `cleo`. `/geheim` — `anna` only.
    /// Under `/raum`: the dataset `Tabelle` (columns `name`, `alter`, `status`, and a `mit`
    /// relation to `/geheim/ziel`, which is therefore hidden from `bea`), a plain page, a
    /// page in the trash, and `einbettung` whose body holds two `dataset-view` blocks — one
    /// at `Tabelle`, one at the dataset `bea` may not read.
    async fn fixture() -> Fixture {
        let store = Store::open("sqlite::memory:").await.unwrap();
        for slug in ["raum", "geheim"] {
            raw_page(&store, None, slug, empty_body()).await;
        }
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        let cleo = Principal::test("cleo", &[], &[]);
        for (path, who, perm) in [
            ("/raum", &anna, Permission::Write),
            ("/geheim", &anna, Permission::Write),
            ("/raum", &bea, Permission::Read),
        ] {
            store
                .add_grant(path, Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let (tabelle_path, tabelle) = dataset(&store, &anna, "/raum", "Tabelle").await;
        let (ziel_path, ziel) = dataset(&store, &anna, "/geheim", "Ziel").await;
        for (key, kind) in [
            ("name", FieldKind::Text),
            ("alter", FieldKind::Number),
            ("status", FieldKind::Select),
        ] {
            field(
                &store,
                &anna,
                &tabelle_path,
                NewField {
                    key: key.into(),
                    label: key.into(),
                    kind,
                    config: if key == "status" {
                        json!({"options": [{"id": "offen"}, {"id": "fertig"}]})
                    } else {
                        json!({})
                    },
                },
            )
            .await;
        }
        field(
            &store,
            &anna,
            &tabelle_path,
            NewField {
                key: "mit".into(),
                label: "Mit".into(),
                kind: FieldKind::Relation,
                config: json!({ "target": ziel_path, "multiple": true }),
            },
        )
        .await;
        field(
            &store,
            &anna,
            &ziel_path,
            NewField {
                key: "zweck".into(),
                label: "Zweck".into(),
                kind: FieldKind::Text,
                config: json!({}),
            },
        )
        .await;
        let view = saved_view(
            &store,
            &anna,
            &tabelle_path,
            json!({"fields": ["name", "mit", "alter"], "sort": {"key": "alter"}}),
        )
        .await;
        let ziel_view = saved_view(&store, &anna, &ziel_path, json!({"fields": ["zweck"]})).await;
        let plain = raw_page(&store, Some("/raum"), "seite", empty_body()).await;
        let mull = raw_page(&store, Some("/raum"), "mull", empty_body()).await;
        // The embedding page, holding both addresses. The ids a reader finds here are the
        // ids that reader asks about, and nothing else.
        let body: Block = serde_json::from_value(json!({"kind": "doc", "content": [
            view_block(&tabelle, &view, "Laborwerte"),
            view_block(&ziel, &ziel_view, "Geheim"),
        ]}))
        .unwrap();
        assert_eq!(body.content[0].kind, BlockKind::DatasetView);
        assert_eq!(body.content[1].kind, BlockKind::DatasetView);
        raw_page(&store, Some("/raum"), "einbettung", body).await;
        Fixture {
            store,
            anna,
            bea,
            cleo,
            tabelle,
            view,
            ziel,
            ziel_view,
            plain,
            mull,
        }
    }

    /// The two halves of the block a page's body carries, as its reader reads them.
    async fn block_on_the_page(store: &Store, who: &Principal, which: usize) -> (String, String) {
        let page = store
            .document_for(who, "/raum/einbettung", Action::Read)
            .await
            .unwrap()
            .expect("bea reads the page that holds the blocks");
        let body: Block = serde_json::from_str(&page.body).unwrap();
        let b = &body.content[which];
        assert_eq!(b.kind, BlockKind::DatasetView);
        (
            b.attrs["dataset_doc"].as_str().unwrap().to_string(),
            b.attrs["view"].as_str().unwrap().to_string(),
        )
    }

    fn json_of(out: &EmbedOutcome) -> String {
        serde_json::to_string(out).unwrap()
    }

    #[tokio::test]
    async fn a_reader_of_the_dataset_gets_the_saved_view_and_the_page_it_belonged_to() {
        let f = fixture().await;
        let out = f
            .store
            .dataset_embed(&f.anna, &f.tabelle, &f.view)
            .await
            .unwrap();
        let EmbedOutcome::Ok {
            path,
            title,
            view: v,
        } = &out
        else {
            panic!("the owner reads her own dataset: {out:?}")
        };
        assert_eq!(
            (path.as_str(), title.as_str()),
            ("/raum/tabelle", "Tabelle")
        );
        assert_eq!(v.id, f.view);
        assert_eq!(v.name, "Alle");
        assert_eq!(v.kind, "table");
        assert_eq!(v.config["fields"], json!(["name", "mit", "alter"]));
        // Config and nothing else: the answer carries no rows and no count.
        let text = json_of(&out);
        assert!(!text.contains("rows") && !text.contains("count"), "{text}");
    }

    /// The load-bearing one: `bea` may read the page that holds the block, and may not read
    /// the dataset its second block points at. The embedding page's permission grants
    /// nothing — not a title, not a path, not the knowledge that the ids point at anything.
    #[tokio::test]
    async fn a_reader_of_the_embedding_page_without_dataset_access_sees_no_title_and_no_view() {
        let f = fixture().await;
        let (asked_doc, asked_view) = block_on_the_page(&f.store, &f.bea, 1).await;
        assert_eq!(asked_doc, f.ziel);
        assert_eq!(asked_view, f.ziel_view);
        assert!(f
            .store
            .document_for(&f.bea, "/raum/einbettung", Action::Read)
            .await
            .unwrap()
            .is_some());
        let out = f
            .store
            .dataset_embed(&f.bea, &asked_doc, &asked_view)
            .await
            .unwrap();
        assert_eq!(out, EmbedOutcome::Unavailable);
        assert_eq!(json_of(&out), r#"{"state":"unavailable"}"#);
        // anna, who owns both pages, gets an answer about the same two ids.
        assert!(matches!(
            f.store
                .dataset_embed(&f.anna, &asked_doc, &asked_view)
                .await
                .unwrap(),
            EmbedOutcome::Ok { .. }
        ));
    }

    /// Every refusal is one answer: value-equal here, byte-identical at the HTTP layer.
    #[tokio::test]
    async fn absent_not_a_dataset_unreadable_trashed_and_malformed_are_one_answer() {
        let f = fixture().await;
        f.store.trash_document(&f.anna, "/raum/mull").await.unwrap();
        let absent = uuid::Uuid::now_v7().to_string();
        let cases = [
            ("absent id", absent.clone(), f.view.clone()),
            ("a plain page", f.plain.clone(), f.view.clone()),
            ("a trashed page", f.mull.clone(), f.view.clone()),
            (
                "a readable dataset, no such view",
                f.tabelle.clone(),
                absent.clone(),
            ),
            (
                "a malformed dataset id",
                "x) ![evil](https://e.example/a".into(),
                f.view.clone(),
            ),
            (
                "a malformed view id",
                f.tabelle.clone(),
                "nicht-ein-uuid".into(),
            ),
            ("empty ids", String::new(), String::new()),
            (
                "the dataset itself, for a stranger",
                f.tabelle.clone(),
                f.view.clone(),
            ),
        ];
        for (what, asked_doc, asked_view) in cases {
            let out = f
                .store
                .dataset_embed(&f.cleo, &asked_doc, &asked_view)
                .await
                .unwrap();
            assert_eq!(
                out,
                EmbedOutcome::Unavailable,
                "{what}: a stranger must not learn which of these it was"
            );
            assert_eq!(json_of(&out), r#"{"state":"unavailable"}"#, "{what}");
        }
        // A trashed dataset answers `Unavailable` even to its owner, and a page id never
        // becomes a dataset id by asking nicely.
        assert_eq!(
            f.store
                .dataset_embed(&f.anna, &f.mull, &f.view)
                .await
                .unwrap(),
            EmbedOutcome::Unavailable
        );
        assert_eq!(
            f.store
                .dataset_embed(&f.anna, &f.plain, &f.view)
                .await
                .unwrap(),
            EmbedOutcome::Unavailable
        );
    }

    /// `ViewMissing` is safe only behind the dataset's own read gate: a caller who may read
    /// the dataset is told the view is not one of its views, and a caller who may not is
    /// refused in the same bytes as an absent page.
    #[tokio::test]
    async fn a_view_that_is_not_one_of_the_datasets_is_view_missing_for_a_reader_only() {
        let f = fixture().await;
        let absent = uuid::Uuid::now_v7().to_string();
        for (what, asked) in [
            ("an unknown id", absent.clone()),
            ("a view of another dataset", f.ziel_view.clone()),
        ] {
            for who in [&f.anna, &f.bea] {
                assert_eq!(
                    f.store
                        .dataset_embed(who, &f.tabelle, &asked)
                        .await
                        .unwrap(),
                    EmbedOutcome::ViewMissing,
                    "{what}: a reader of the dataset may be told this"
                );
            }
            let out = f
                .store
                .dataset_embed(&f.cleo, &f.tabelle, &asked)
                .await
                .unwrap();
            assert_eq!(
                out,
                EmbedOutcome::Unavailable,
                "{what}: not even this for a stranger"
            );
            assert_eq!(json_of(&out), r#"{"state":"unavailable"}"#);
        }
        // The foreign view, asked of the dataset that owns it, is an answer.
        assert!(matches!(
            f.store
                .dataset_embed(&f.anna, &f.ziel, &f.ziel_view)
                .await
                .unwrap(),
            EmbedOutcome::Ok { .. }
        ));
    }

    /// The strip is the same seam the view list uses: a field hidden from this caller is
    /// gone from the returned config, and the view itself still comes back.
    #[tokio::test]
    async fn a_field_hidden_from_the_caller_is_stripped_from_the_embedded_view() {
        let f = fixture().await;
        let out = f
            .store
            .dataset_embed(&f.bea, &f.tabelle, &f.view)
            .await
            .unwrap();
        let EmbedOutcome::Ok { view: v, .. } = out else {
            panic!("bea reads /raum, so she reads the dataset: {out:?}")
        };
        assert_eq!(
            v.config["fields"],
            json!(["name", "alter"]),
            "the relation to a dataset bea may not read is gone: {}",
            v.config
        );
        assert_eq!(
            v.config["sort"],
            json!({"key": "alter"}),
            "a sort on a visible field stays"
        );
        // The owner still sees all three columns.
        let SchemaOutcome::Done(list) = f
            .store
            .dataset_views(&f.anna, "/raum/tabelle")
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(list[0].config["fields"], json!(["name", "mit", "alter"]));
    }

    /// What a refusal may never carry: a title, a path, a count, a name — the body is one
    /// constant.
    #[tokio::test]
    async fn no_refusal_names_a_page_a_count_or_a_view() {
        let f = fixture().await;
        for (who, doc, view) in [
            (&f.cleo, f.tabelle.clone(), f.view.clone()),
            (&f.bea, f.ziel.clone(), f.ziel_view.clone()),
            (&f.anna, f.mull.clone(), f.view.clone()),
            (&f.anna, String::new(), String::new()),
        ] {
            let out = f.store.dataset_embed(who, &doc, &view).await.unwrap();
            assert_eq!(json_of(&out), r#"{"state":"unavailable"}"#);
        }
        assert_eq!(
            json_of(&EmbedOutcome::ViewMissing),
            r#"{"state":"view_missing"}"#
        );
    }
}
