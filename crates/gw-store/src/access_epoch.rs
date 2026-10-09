//! The access epoch: a number that moves when who-may-do-what may have changed (ADR 0027).
//!
//! Migration 0019 owns the writes (triggers). This module owns the listening: an update hook
//! on the one-row `access_epoch` table marks the connection dirty, and the commit hook turns
//! that into a bump of an in-memory [`tokio::sync::watch`] — so a subscriber hears about a
//! change when it is durable, never for a transaction that was rolled back.
//!
//! **Why "when committed" is enough for a reader.** The pool holds ONE connection (see
//! [`crate::Store::open`]), so a subscriber that re-reads after hearing the bump queues behind
//! the committing transaction and sees its result. A pool with more connections would need the
//! bump moved to after the commit returns.

use sqlx::sqlite::{SqliteConnection, SqliteOperation};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

/// The sending half, shared by the connection hooks and [`crate::Store`].
#[derive(Clone, Default)]
pub(crate) struct AccessEpoch {
    sender: Arc<watch::Sender<u64>>,
}

impl AccessEpoch {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.sender.subscribe()
    }

    pub(crate) fn bump(&self) {
        self.sender.send_modify(|n| *n += 1);
    }

    /// Install the hooks on a freshly opened connection. Called from the pool's
    /// `after_connect`, so a connection the pool replaces is hooked again.
    pub(crate) async fn attach(&self, connection: &mut SqliteConnection) -> sqlx::Result<()> {
        let dirty = Arc::new(AtomicBool::new(false));
        let mut handle = connection.lock_handle().await?;

        let marked = Arc::clone(&dirty);
        handle.set_update_hook(move |change| {
            if change.table == "access_epoch" && change.operation == SqliteOperation::Update {
                marked.store(true, Ordering::SeqCst);
            }
        });

        let epoch = self.clone();
        let committing = Arc::clone(&dirty);
        handle.set_commit_hook(move || {
            if committing.swap(false, Ordering::SeqCst) {
                epoch.bump();
            }
            true
        });

        handle.set_rollback_hook(move || dirty.store(false, Ordering::SeqCst));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use gw_auth::{Permission, Subject};

    async fn store() -> Store {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_local_principal("anna", "Anna", None, "$argon2id$fake")
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO documents (id, path, slug, doc_type, title, body)
             VALUES ('d1', '/a', 'a', 'page', 'A', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        store
    }

    /// Run `change` and report whether the epoch moved, as a subscriber would see it.
    async fn moved<F, Fut>(store: &Store, change: F) -> bool
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let mut rx = store.access_epoch();
        rx.borrow_and_update();
        change().await;
        rx.has_changed().unwrap()
    }

    async fn anna(store: &Store) -> String {
        store
            .principal_by_username("anna")
            .await
            .unwrap()
            .unwrap()
            .0
            .id
    }

    async fn sql(store: &Store, statement: &str) {
        sqlx::query(statement).execute(&store.pool).await.unwrap();
    }

    #[tokio::test]
    async fn every_grant_change_moves_the_epoch() {
        let store = store().await;
        let id = anna(&store).await;
        assert!(
            moved(&store, || async {
                store
                    .add_grant("/a", Subject::Principal(id.clone()), Permission::Write)
                    .await
                    .unwrap();
            })
            .await,
            "adding a grant"
        );
        assert!(
            moved(&store, || async {
                store
                    .remove_grant("/a", &Subject::Principal(id.clone()), Permission::Write)
                    .await
                    .unwrap();
            })
            .await,
            "removing a grant"
        );
    }

    #[tokio::test]
    async fn deactivation_a_group_change_and_a_session_ending_move_it() {
        let store = store().await;
        let id = anna(&store).await;
        store.create_session(&id, "hash", 3600).await.unwrap();
        assert!(
            moved(&store, || async {
                store.set_principal_active(&id, false).await.unwrap()
            })
            .await,
            "deactivation"
        );
        assert!(
            moved(&store, || async {
                sql(&store, "UPDATE principals SET groups = '[\"admins\"]'").await
            })
            .await,
            "a changed group list"
        );
        // Deactivation already ended the first session; this is a plain logout.
        store.create_session(&id, "hash2", 3600).await.unwrap();
        assert!(
            moved(&store, || async {
                store.delete_session("hash2").await.unwrap();
            })
            .await,
            "a session ending"
        );
    }

    #[tokio::test]
    async fn a_move_a_trashing_and_a_visibility_change_move_it() {
        let store = store().await;
        for (what, statement) in [
            ("a move", "UPDATE documents SET path = '/b'"),
            ("a new parent", "UPDATE documents SET parent_path = '/x'"),
            ("visibility", "UPDATE documents SET visibility = 'public'"),
            (
                "trashing",
                "UPDATE documents SET deleted_at = datetime('now'), deleted_root = 'd1',
                 deleted_by = 'x', deleted_by_name = 'X'",
            ),
            ("purging", "DELETE FROM documents"),
        ] {
            assert!(
                moved(&store, || async { sql(&store, statement).await }).await,
                "{what}"
            );
        }
    }

    #[tokio::test]
    async fn writes_that_change_nobody_s_access_do_not_move_it() {
        let store = store().await;
        let id = anna(&store).await;
        assert!(
            !moved(&store, || async {
                // A publish, a title edit, and a login that refreshes an unchanged group list.
                sql(
                    &store,
                    "UPDATE documents SET body = '{\"kind\":\"doc\"}', title = 'B'",
                )
                .await;
                sql(
                    &store,
                    "UPDATE principals SET last_seen_at = datetime('now'), groups = groups",
                )
                .await;
                store.create_session(&id, "hash", 3600).await.unwrap();
            })
            .await,
            "ordinary writes were announced as access changes"
        );
    }

    #[tokio::test]
    async fn a_rolled_back_change_is_not_announced() {
        let store = store().await;
        let mut rx = store.access_epoch();
        rx.borrow_and_update();
        let mut tx = store.pool.begin().await.unwrap();
        sqlx::query("UPDATE documents SET path = '/b'")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert!(!rx.has_changed().unwrap(), "a rollback was announced");
        // And the dirty mark did not leak into the next, unrelated commit.
        sql(&store, "UPDATE documents SET title = 'C'").await;
        assert!(
            !rx.has_changed().unwrap(),
            "a stale dirty mark announced a later commit"
        );
    }

    #[tokio::test]
    async fn a_change_is_announced_only_once_it_is_durable() {
        let store = store().await;
        let mut rx = store.access_epoch();
        rx.borrow_and_update();
        let mut tx = store.pool.begin().await.unwrap();
        sqlx::query("UPDATE documents SET path = '/b'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(!rx.has_changed().unwrap(), "announced before the commit");
        tx.commit().await.unwrap();
        assert!(rx.has_changed().unwrap(), "not announced at the commit");
    }
}
