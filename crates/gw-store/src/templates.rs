//! Page templates, and creating a page — with or without one — as a person (ADR 0028).
//!
//! A template is an ordinary page under [`TEMPLATE_ROOT`]. It therefore has history,
//! permissions, search and links without a storage type of its own, and *which templates a
//! caller is offered* is simply which of those pages the caller may read.
//!
//! **Creating a page over HTTP did not exist before this module.** [`Store::create_document`]
//! deliberately decides no authorisation (its only caller was the importer), so the
//! permission-checked door lives here: write on the parent, administration for the top
//! level, the same rules [`Store::move_document`] applies to a destination.

use crate::acl::Baseline;
use crate::{Author, NewDocument, Store, TreeNode};
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::{slugify, title_problem, Block, Visibility};
use serde::Serialize;

/// The reserved subtree whose pages are templates.
pub const TEMPLATE_ROOT: &str = "/vorlagen";

/// Top-level addresses the web app serves itself (`web/src/routes`, plus the create form).
/// A page there would be unreachable, or would shadow the app.
pub const RESERVED_TOP_LEVEL: [&str; 11] = [
    "admin",
    "api",
    "aufgaben",
    "benachrichtigungen",
    "graph",
    "neu",
    "papierkorb",
    "projekte",
    "suche",
    "themen",
    "history",
];

/// Longest title a new page may have, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// A template as the picker lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateEntry {
    pub path: String,
    pub title: String,
}

/// What a new page is called and where it goes.
#[derive(Debug, Clone)]
pub struct CreateRequest {
    /// `None` is the top level.
    pub parent: Option<String>,
    pub title: String,
    /// `None` or blank derives the address from the title.
    pub slug: Option<String>,
    /// A template's path; `None` makes an empty page.
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Created {
        path: String,
        id: String,
    },
    /// Nothing was created; the reason is safe to show this caller.
    Blocked(String),
    /// Not a signed-in, active account.
    Refused,
}

/// Fill `{{titel}}` and `{{datum}}` in one pass. Nothing else is evaluated, and what is
/// inserted is never scanned again, so a title that spells `{{datum}}` stays as typed.
pub fn fill_placeholders(text: &str, title: &str, date: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(r) = tail.strip_prefix("{{titel}}") {
            out.push_str(title);
            rest = r;
        } else if let Some(r) = tail.strip_prefix("{{datum}}") {
            out.push_str(date);
            rest = r;
        } else {
            out.push_str("{{");
            rest = &tail[2..];
        }
    }
    out.push_str(rest);
    out
}

fn fill_block(block: &mut Block, title: &str, date: &str) {
    if let Some(text) = block.text.as_mut() {
        *text = fill_placeholders(text, title, date);
    }
    for child in &mut block.content {
        fill_block(child, title, date);
    }
}

fn is_template_path(path: &str) -> bool {
    path.strip_prefix(TEMPLATE_ROOT)
        .is_some_and(|rest| rest.starts_with('/') && rest.len() > 1)
}

fn collect(nodes: &[TreeNode], out: &mut Vec<TemplateEntry>) {
    for node in nodes {
        out.push(TemplateEntry {
            path: node.path.clone(),
            title: node.title.clone(),
        });
        collect(&node.children, out);
    }
}

impl Store {
    /// The templates `principal` may read, in tree order. Filtered by `tree_for`, the
    /// retriever every listing uses, so an unreadable template is not listed rather than
    /// listed and hidden. The root page itself is a container, not an offer.
    pub async fn templates_for(&self, principal: &Principal) -> Result<Vec<TemplateEntry>> {
        fn find(nodes: &[TreeNode]) -> Option<&TreeNode> {
            nodes.iter().find(|n| n.path == TEMPLATE_ROOT)
        }
        let tree = self.tree_for(principal).await?;
        let mut out = Vec::new();
        if let Some(root) = find(&tree) {
            collect(&root.children, &mut out);
        }
        Ok(out)
    }

    /// Create a page as `principal`, copying a template's body when one is named.
    ///
    /// The copy is taken once, here: the new page has no link back to the template, and
    /// editing either never changes the other. A template the caller cannot read is
    /// answered in the words of one that does not exist (ADR 0022).
    pub async fn create_page_for(
        &self,
        principal: &Principal,
        request: &CreateRequest,
        administers_destination: bool,
        today: &str,
    ) -> Result<CreateOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(CreateOutcome::Refused);
        }
        let baseline: Baseline = self.baseline_for(principal).await?;
        let title = request.title.trim();
        if title.is_empty() {
            return Ok(CreateOutcome::Blocked("a page needs a title".into()));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Ok(CreateOutcome::Blocked(format!(
                "a title may have at most {MAX_TITLE_CHARS} characters"
            )));
        }
        if let Some(reason) = title_problem(title) {
            return Ok(CreateOutcome::Blocked(reason.to_string()));
        }
        let slug = slugify(
            request
                .slug
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(title),
        );
        // Cap slug at 100 characters to avoid overly long paths
        let mut slug = slug;
        if slug.len() > 100 {
            // Truncate on a char boundary, then trim trailing '-'
            slug.truncate(100);
            while !slug.is_empty() && slug.ends_with('-') {
                slug.pop();
            }
        }
        if slug.is_empty() {
            return Ok(CreateOutcome::Blocked(format!(
                "«{title}» contains nothing an address can be made of"
            )));
        }

        let parent = request
            .parent
            .as_deref()
            .map(|p| p.trim().trim_matches('/'))
            .filter(|p| !p.is_empty())
            .map(|p| format!("/{p}"));
        // `history` is a route under every page, so a page called that is unreachable at
        // any depth; the rest shadow the app only at the top.
        if slug == "history" || (parent.is_none() && RESERVED_TOP_LEVEL.contains(&slug.as_str())) {
            return Ok(CreateOutcome::Blocked(format!(
                "«{slug}» is reserved: choose another address"
            )));
        }
        match &parent {
            Some(p) => {
                if self
                    .document_access_with_baseline(principal, p, Action::Read, baseline)
                    .await?
                    .is_none()
                {
                    return Ok(CreateOutcome::Blocked(format!("there is no page at {p}")));
                }
                if self
                    .document_access_with_baseline(principal, p, Action::Write, baseline)
                    .await?
                    .is_none()
                {
                    return Ok(CreateOutcome::Blocked(format!(
                        "you may not add pages under {p}"
                    )));
                }
            }
            None if !administers_destination => {
                return Ok(CreateOutcome::Blocked(
                    "only somebody who administers the whole wiki may put a page at the top level"
                        .into(),
                ));
            }
            None => {}
        }

        let path = format!("{}/{slug}", parent.as_deref().unwrap_or(""));
        let taken: Option<(String,)> = sqlx::query_as("SELECT path FROM documents WHERE path = ?1")
            .bind(&path)
            .fetch_optional(&self.pool)
            .await?;
        if taken.is_some() {
            return Ok(CreateOutcome::Blocked(format!(
                "there is already a page at {path}"
            )));
        }

        let (doc_type, language, mut body) = match request.template.as_deref() {
            None => (
                gw_core::DocumentType::Page,
                "de".to_string(),
                Block {
                    kind: gw_core::BlockKind::Doc,
                    attrs: Default::default(),
                    content: Vec::new(),
                    text: None,
                    marks: Vec::new(),
                },
            ),
            Some(t) => {
                let missing = || CreateOutcome::Blocked(format!("there is no template at {t}"));
                if !is_template_path(t) {
                    return Ok(missing());
                }
                let Some(access) = self
                    .document_access_with_baseline(principal, t, Action::Read, baseline)
                    .await?
                else {
                    return Ok(missing());
                };
                let d = access.document;
                (
                    d.doc_type.parse().unwrap_or(gw_core::DocumentType::Page),
                    d.language.clone(),
                    serde_json::from_str(&d.body)?,
                )
            }
        };
        fill_block(&mut body, title, today);

        let id = self
            .create_document(
                Author::Account(principal),
                &NewDocument {
                    parent_path: parent,
                    doc_type,
                    title: title.to_string(),
                    slug: Some(slug),
                    language,
                    // Fail closed: a new page is restricted until someone with admin on it
                    // says otherwise (ADR 0008), whatever the template's own visibility.
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                request.template.as_deref().map(|_| "from template"),
            )
            .await?;
        Ok(CreateOutcome::Created { path, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_auth::{Permission, Subject};
    use gw_core::{BlockKind, DocumentType};

    fn leaf(text: &str) -> Block {
        Block {
            kind: BlockKind::Paragraph,
            attrs: Default::default(),
            content: Vec::new(),
            text: Some(text.into()),
            marks: Vec::new(),
        }
    }

    fn doc(children: Vec<Block>) -> Block {
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: children,
            text: None,
            marks: Vec::new(),
        }
    }

    async fn page(store: &Store, parent: Option<&str>, title: &str, body: Block) {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
    }

    async fn grant(store: &Store, path: &str, who: &Principal, p: Permission) {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), p)
            .await
            .unwrap();
    }

    fn req(parent: Option<&str>, title: &str, template: Option<&str>) -> CreateRequest {
        CreateRequest {
            parent: parent.map(Into::into),
            title: title.into(),
            slug: None,
            template: template.map(Into::into),
        }
    }

    /// A store with `/vorlagen` (+ one template), `/raum`, and `anna` who writes `/raum`
    /// and reads the template.
    async fn world() -> (Store, Principal) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        page(&store, None, "Vorlagen", doc(vec![])).await;
        page(
            &store,
            Some("/vorlagen"),
            "Besprechung",
            doc(vec![leaf("{{titel}} am {{datum}}")]),
        )
        .await;
        page(&store, None, "Raum", doc(vec![])).await;
        let anna = Principal::test("anna", &[], &[]);
        grant(&store, "/raum", &anna, Permission::Write).await;
        grant(&store, "/vorlagen", &anna, Permission::Read).await;
        (store, anna)
    }

    #[test]
    fn only_the_two_placeholders_are_filled_and_once() {
        let f = |t| fill_placeholders(t, "A {{datum}}", "09.10.2026");
        assert_eq!(f("{{titel}} / {{datum}}"), "A {{datum}} / 09.10.2026");
        assert_eq!(f("{{andere}} {{ {{titel"), "{{andere}} {{ {{titel");
        assert_eq!(f("kein Platzhalter"), "kein Platzhalter");
    }

    #[tokio::test]
    async fn the_picker_lists_what_the_caller_may_read_and_nothing_else() {
        let (store, anna) = world().await;
        let got = store.templates_for(&anna).await.unwrap();
        assert_eq!(
            got,
            vec![TemplateEntry {
                path: "/vorlagen/besprechung".into(),
                title: "Besprechung".into()
            }]
        );
        let fremde = Principal::test("fremde", &[], &[]);
        assert!(store.templates_for(&fremde).await.unwrap().is_empty());
        assert!(store
            .templates_for(&Principal::anonymous())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn a_page_is_a_filled_copy_not_a_link() {
        let (store, anna) = world().await;
        let out = store
            .create_page_for(
                &anna,
                &req(Some("/raum"), "Montag", Some("/vorlagen/besprechung")),
                false,
                "09.10.2026",
            )
            .await
            .unwrap();
        let CreateOutcome::Created { path, .. } = out else {
            panic!("{out:?}")
        };
        assert_eq!(path, "/raum/montag");
        let made = store
            .document_for(&anna, &path, Action::Read)
            .await
            .unwrap()
            .unwrap();
        assert!(made.body.contains("Montag am 09.10.2026"), "{}", made.body);
        // The template is untouched.
        let tpl = store
            .document_for(&anna, "/vorlagen/besprechung", Action::Read)
            .await
            .unwrap()
            .unwrap();
        assert!(tpl.body.contains("{{titel}}"));
        assert_eq!(made.visibility, "restricted");
    }

    #[tokio::test]
    async fn an_unreadable_template_answers_like_a_missing_one() {
        let (store, anna) = world().await;
        page(&store, Some("/vorlagen"), "Geheim", doc(vec![leaf("x")])).await;
        // anna reads /vorlagen, so make a sibling tree she cannot read.
        page(&store, None, "Privat", doc(vec![])).await;
        let ask = |t: &'static str| {
            let store = &store;
            let anna = &anna;
            async move {
                store
                    .create_page_for(anna, &req(Some("/raum"), "X", Some(t)), false, "d")
                    .await
                    .unwrap()
            }
        };
        let missing = ask("/vorlagen/gibt-es-nicht").await;
        assert_eq!(
            missing,
            CreateOutcome::Blocked("there is no template at /vorlagen/gibt-es-nicht".into())
        );
        // A page outside the template subtree is not a template, readable or not.
        assert!(matches!(ask("/raum").await, CreateOutcome::Blocked(_)));
        // Revoke her read on templates entirely: existing and absent are one answer.
        let nobody = Principal::test("nobody", &[], &[]);
        grant(&store, "/raum", &nobody, Permission::Write).await;
        let real = store
            .create_page_for(
                &nobody,
                &req(Some("/raum"), "X", Some("/vorlagen/besprechung")),
                false,
                "d",
            )
            .await
            .unwrap();
        assert_eq!(
            real,
            CreateOutcome::Blocked("there is no template at /vorlagen/besprechung".into())
        );
    }

    #[tokio::test]
    async fn creating_needs_write_on_the_parent_or_admin_at_the_top() {
        let (store, anna) = world().await;
        let r = |p: Option<&str>, a: bool| {
            let store = &store;
            let anna = &anna;
            let rq = req(p, "Oben", None);
            async move { store.create_page_for(anna, &rq, a, "d").await.unwrap() }
        };
        assert!(matches!(
            r(Some("/raum"), false).await,
            CreateOutcome::Created { .. }
        ));
        // Read-only parent: told why, because she can see it.
        assert_eq!(
            r(Some("/vorlagen"), false).await,
            CreateOutcome::Blocked("you may not add pages under /vorlagen".into())
        );
        // Unseen parent: same words as an absent one.
        assert_eq!(
            r(Some("/privat"), false).await,
            CreateOutcome::Blocked("there is no page at /privat".into())
        );
        assert!(matches!(r(None, false).await, CreateOutcome::Blocked(_)));
        assert!(matches!(r(None, true).await, CreateOutcome::Created { .. }));
        let anon = store
            .create_page_for(&Principal::anonymous(), &req(None, "Z", None), true, "d")
            .await
            .unwrap();
        assert_eq!(anon, CreateOutcome::Refused);
    }

    #[tokio::test]
    async fn reserved_addresses_and_overlong_titles_are_refused() {
        let (store, anna) = world().await;
        let go = |parent: Option<&str>, title: String| {
            let store = &store;
            let anna = &anna;
            let rq = req(parent, &title, None);
            async move { store.create_page_for(anna, &rq, true, "d").await.unwrap() }
        };
        assert!(
            matches!(go(None, "Admin".into()).await, CreateOutcome::Blocked(m) if m.contains("reserved"))
        );
        assert!(
            matches!(go(Some("/raum"), "History".into()).await, CreateOutcome::Blocked(m) if m.contains("reserved"))
        );
        // Reserved only at the top: /raum/admin is fine.
        assert!(matches!(
            go(Some("/raum"), "Admin".into()).await,
            CreateOutcome::Created { .. }
        ));
        assert!(
            matches!(go(Some("/raum"), "x".repeat(201)).await, CreateOutcome::Blocked(m) if m.contains("at most"))
        );
    }

    #[tokio::test]
    async fn a_blank_title_is_refused_as_a_missing_title() {
        let (store, anna) = world().await;
        let out = store
            .create_page_for(&anna, &req(Some("/raum"), "   ", None), false, "d")
            .await
            .unwrap();
        assert_eq!(out, CreateOutcome::Blocked("a page needs a title".into()));
    }

    #[tokio::test]
    async fn a_traversing_parent_names_no_page() {
        let (store, anna) = world().await;
        for parent in ["/raum/../vorlagen", "/raum/%2e%2e", "/raum//x", "raum/./"] {
            let out = store
                .create_page_for(&anna, &req(Some(parent), "T", None), false, "d")
                .await
                .unwrap();
            assert!(
                matches!(out, CreateOutcome::Blocked(_)),
                "{parent}: {out:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_page_made_where_a_moved_page_used_to_be_ends_its_forward() {
        let (store, anna) = world().await;
        sqlx::query("INSERT INTO forwards (old_path, document_id) SELECT '/raum/alt', id FROM documents WHERE path = '/raum'")
            .execute(&store.pool)
            .await
            .unwrap();
        store
            .create_page_for(&anna, &req(Some("/raum"), "Alt", None), false, "d")
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM forwards")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn an_address_already_taken_is_refused() {
        let (store, anna) = world().await;
        let rq = req(Some("/raum"), "Doppelt", None);
        store.create_page_for(&anna, &rq, false, "d").await.unwrap();
        assert!(matches!(
            store.create_page_for(&anna, &rq, false, "d").await.unwrap(),
            CreateOutcome::Blocked(m) if m.contains("already")
        ));
    }

    #[tokio::test]
    async fn a_slug_is_capulated_at_100_chars() {
        let (store, anna) = world().await;
        // Create a slug that would be > 100 chars after slugification
        let long_title = "a".repeat(200); // 200 chars
        let rq = req(Some("/raum"), &long_title, None);
        let outcome = store.create_page_for(&anna, &rq, false, "d").await.unwrap();

        let CreateOutcome::Created { path, .. } = outcome else {
            panic!(
                "Expected page creation to succeed for title length {} (expected <= 200)",
                long_title.len()
            );
        };

        // Check that the slug segment (last part of path) is <= 100 chars
        let slug_segment = path.split('/').next_back().unwrap();
        assert!(
            slug_segment.len() <= 100,
            "Slug segment is {} chars: {}",
            slug_segment.len(),
            slug_segment
        );

        // The slug should have trailing '-' trimmed
        assert!(
            !slug_segment.ends_with('-'),
            "Slug ends with dash: {}",
            slug_segment
        );
    }

    #[tokio::test]
    async fn a_slug_of_only_punctuation_is_refused() {
        let (store, anna) = world().await;
        let rq = req(Some("/raum"), "!!! ??? ???", None);
        let outcome = store.create_page_for(&anna, &rq, false, "d").await.unwrap();

        assert!(
            matches!(outcome, CreateOutcome::Blocked(_)),
            "Empty slug should be refused: {outcome:?}"
        );
    }

    #[tokio::test]
    async fn a_title_with_a_line_break_is_refused_and_nothing_is_created() {
        let (store, anna) = world().await;
        let rq = req(Some("/raum"), "Seite\n- admin: gefälscht", None);
        let outcome = store.create_page_for(&anna, &rq, false, "d").await.unwrap();
        assert!(
            matches!(outcome, CreateOutcome::Blocked(_)),
            "a control character in a title must be refused: {outcome:?}"
        );
        let made: Option<(String,)> =
            sqlx::query_as("SELECT path FROM documents WHERE path LIKE '/raum/seite%'")
                .fetch_optional(&store.pool)
                .await
                .unwrap();
        assert!(made.is_none(), "a refused title still created {made:?}");
    }
}
