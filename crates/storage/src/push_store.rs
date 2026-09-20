//! `lumen_core::ext::PushBackend` bridge over [`PushSubscriptions`].
//!
//! Unlike [`SwStore`](crate::sw_store::SwStore) (an opaque JSON snapshot over
//! the generic [`StorageBackend`](lumen_core::ext::StorageBackend)),
//! subscriptions are structured rows the store already indexes by
//! `(origin, scope)` — `PushStore` is a thin adapter, not a codec.

use std::sync::Arc;

use lumen_core::ext::PushBackend;

use crate::push_subscriptions::PushSubscriptions;

/// [`PushBackend`] over a shared [`PushSubscriptions`] table.
///
/// One instance is shared (via `Arc`) across every origin/tab in the
/// process — the table itself partitions rows by `(origin, scope)`, so
/// unlike `SwStore` there is no need for one adapter per origin.
pub struct PushStore {
    subs: Arc<PushSubscriptions>,
}

impl std::fmt::Debug for PushStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushStore").finish()
    }
}

impl PushStore {
    /// Wrap an existing [`PushSubscriptions`] table.
    pub fn new(subs: Arc<PushSubscriptions>) -> Self {
        Self { subs }
    }
}

impl PushBackend for PushStore {
    fn push_subscribe(
        &self,
        origin: &str,
        scope: &str,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        user_visible_only: bool,
    ) {
        // Best-effort (trait contract): a storage failure must not abort the
        // JS `subscribe()` call that is already holding a resolved endpoint.
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let _ = self.subs.subscribe(
            origin,
            scope,
            endpoint,
            p256dh,
            auth,
            user_visible_only,
            created_at,
        );
    }

    fn push_get(&self, origin: &str, scope: &str) -> Option<(String, String, String, bool)> {
        let sub = self.subs.get_by_scope(origin, scope).ok()??;
        Some((sub.endpoint, sub.p256dh, sub.auth, sub.user_visible_only))
    }

    fn push_unsubscribe(&self, origin: &str, scope: &str) -> bool {
        let Ok(Some(sub)) = self.subs.get_by_scope(origin, scope) else {
            return false;
        };
        self.subs.unsubscribe(sub.id).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> PushStore {
        PushStore::new(Arc::new(PushSubscriptions::open_in_memory().unwrap()))
    }

    #[test]
    fn subscribe_then_get() {
        let store = make();
        store.push_subscribe("https://x.test", "/", "https://push/ep", "p256", "auth", true);
        let got = store.push_get("https://x.test", "/").unwrap();
        assert_eq!(got, ("https://push/ep".into(), "p256".into(), "auth".into(), true));
    }

    #[test]
    fn get_missing_is_none() {
        let store = make();
        assert!(store.push_get("https://x.test", "/").is_none());
    }

    #[test]
    fn resubscribe_same_scope_overwrites() {
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep1", "k1", "a1", true);
        store.push_subscribe("https://x.test", "/", "ep2", "k2", "a2", false);
        let got = store.push_get("https://x.test", "/").unwrap();
        assert_eq!(got, ("ep2".into(), "k2".into(), "a2".into(), false));
    }

    #[test]
    fn unsubscribe_removes_and_reports_existence() {
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep", "k", "a", true);
        assert!(store.push_unsubscribe("https://x.test", "/"));
        assert!(store.push_get("https://x.test", "/").is_none());
        assert!(!store.push_unsubscribe("https://x.test", "/"));
    }

    #[test]
    fn shared_across_clones_survives_handle_drop() {
        let subs = Arc::new(PushSubscriptions::open_in_memory().unwrap());
        let store_a = PushStore::new(Arc::clone(&subs));
        store_a.push_subscribe("https://x.test", "/", "ep", "k", "a", true);
        drop(store_a);
        let store_b = PushStore::new(subs);
        assert!(store_b.push_get("https://x.test", "/").is_some());
    }
}
