//! `lumen_core::ext::PushBackend` bridge over [`PushSubscriptions`].
//!
//! Unlike [`SwStore`](crate::sw_store::SwStore) (an opaque JSON snapshot over
//! the generic [`StorageBackend`](lumen_core::ext::StorageBackend)),
//! subscriptions are structured rows the store already indexes by
//! `(origin, scope)` — `PushStore` is a thin adapter, not a codec.

use std::sync::Arc;

use lumen_core::ext::PushBackend;

use crate::permissions::{PermissionKind, PermissionState, Permissions};
use crate::push_crypto;
use crate::push_messages::PushMessages;
use crate::push_subscriptions::PushSubscriptions;

/// [`PushBackend`] over a shared [`PushSubscriptions`] table plus a
/// [`Permissions`] store for `PushManager.permissionState()` (срез 3).
///
/// One instance is shared (via `Arc`) across every origin/tab in the
/// process — both tables partition rows by origin (subscriptions further by
/// scope), so unlike `SwStore` there is no need for one adapter per origin.
///
/// `messages` (срез 4) is owned outright, not shared via the constructor —
/// unlike subscriptions/permissions it has no cross-restart value (an
/// undelivered push message queued in one process run is meaningless in the
/// next), so there is nothing external to inject. `None` only if the
/// in-memory SQLite connection itself fails to open — best-effort, like the
/// rest of this trait: `push_deliver`/`push_take_pending` then act as if the
/// queue were always empty rather than panicking.
pub struct PushStore {
    subs: Arc<PushSubscriptions>,
    permissions: Arc<Permissions>,
    messages: Option<PushMessages>,
}

impl std::fmt::Debug for PushStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushStore").finish()
    }
}

impl PushStore {
    /// Wrap an existing [`PushSubscriptions`] table and [`Permissions`] store.
    pub fn new(subs: Arc<PushSubscriptions>, permissions: Arc<Permissions>) -> Self {
        Self {
            subs,
            permissions,
            messages: PushMessages::open_in_memory().ok(),
        }
    }
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl PushBackend for PushStore {
    fn push_subscribe(
        &self,
        origin: &str,
        scope: &str,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        private_key: &str,
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
            private_key,
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

    fn push_permission_state(&self, origin: &str) -> String {
        // Best-effort (trait contract): a storage failure reads as "prompt",
        // the same answer as no grant on record — it must never read as
        // "granted", which would silently unblock subscribe().
        match self.permissions.query(origin, &PermissionKind::Push, now_unix_secs()) {
            Ok(PermissionState::Granted) => "granted".to_string(),
            Ok(PermissionState::Denied) => "denied".to_string(),
            Ok(PermissionState::Prompt) | Err(_) => "prompt".to_string(),
        }
    }

    fn push_set_permission(&self, origin: &str, state: &str) {
        let state = match state {
            "granted" => PermissionState::Granted,
            "denied" => PermissionState::Denied,
            _ => PermissionState::Prompt,
        };
        let _ = self.permissions.set(origin, &PermissionKind::Push, state, None);
    }

    fn push_deliver(&self, origin: &str, scope: &str, payload: &[u8]) -> bool {
        let Ok(Some(sub)) = self.subs.get_by_scope(origin, scope) else {
            return false;
        };
        let Some(plaintext) = push_crypto::decrypt(&sub.private_key, &sub.auth, payload) else {
            return false;
        };
        let Some(messages) = self.messages.as_ref() else {
            return false;
        };
        messages.enqueue(sub.id, &plaintext, now_unix_secs()).is_ok()
    }

    fn push_take_pending(&self, origin: &str, scope: &str) -> Option<Vec<u8>> {
        let sub = self.subs.get_by_scope(origin, scope).ok()??;
        self.messages.as_ref()?.take_oldest(sub.id).ok()?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> PushStore {
        PushStore::new(
            Arc::new(PushSubscriptions::open_in_memory().unwrap()),
            Arc::new(Permissions::open_in_memory().unwrap()),
        )
    }

    #[test]
    fn subscribe_then_get() {
        let store = make();
        store.push_subscribe("https://x.test", "/", "https://push/ep", "p256", "auth", "priv", true);
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
        store.push_subscribe("https://x.test", "/", "ep1", "k1", "a1", "priv", true);
        store.push_subscribe("https://x.test", "/", "ep2", "k2", "a2", "priv", false);
        let got = store.push_get("https://x.test", "/").unwrap();
        assert_eq!(got, ("ep2".into(), "k2".into(), "a2".into(), false));
    }

    #[test]
    fn unsubscribe_removes_and_reports_existence() {
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep", "k", "a", "priv", true);
        assert!(store.push_unsubscribe("https://x.test", "/"));
        assert!(store.push_get("https://x.test", "/").is_none());
        assert!(!store.push_unsubscribe("https://x.test", "/"));
    }

    #[test]
    fn shared_across_clones_survives_handle_drop() {
        let subs = Arc::new(PushSubscriptions::open_in_memory().unwrap());
        let perms = Arc::new(Permissions::open_in_memory().unwrap());
        let store_a = PushStore::new(Arc::clone(&subs), Arc::clone(&perms));
        store_a.push_subscribe("https://x.test", "/", "ep", "k", "a", "priv", true);
        drop(store_a);
        let store_b = PushStore::new(subs, perms);
        assert!(store_b.push_get("https://x.test", "/").is_some());
    }

    #[test]
    fn permission_state_defaults_to_prompt() {
        let store = make();
        assert_eq!(store.push_permission_state("https://x.test"), "prompt");
    }

    #[test]
    fn permission_state_reflects_explicit_grant_and_denial() {
        let store = make();
        store.push_set_permission("https://x.test", "granted");
        assert_eq!(store.push_permission_state("https://x.test"), "granted");
        store.push_set_permission("https://x.test", "denied");
        assert_eq!(store.push_permission_state("https://x.test"), "denied");
    }

    #[test]
    fn permission_state_is_isolated_per_origin() {
        let store = make();
        store.push_set_permission("https://a.test", "denied");
        assert_eq!(store.push_permission_state("https://a.test"), "denied");
        assert_eq!(store.push_permission_state("https://b.test"), "prompt");
    }

    #[test]
    fn permission_set_back_to_prompt_via_unrecognised_state() {
        let store = make();
        store.push_set_permission("https://x.test", "granted");
        store.push_set_permission("https://x.test", "nonsense");
        assert_eq!(store.push_permission_state("https://x.test"), "prompt");
    }

    /// Срез 4 DoD: `push_deliver` decrypts a spec-shaped WebPush body with
    /// the subscription's real (mock-relay-encrypted) key material and
    /// queues the plaintext for `push_take_pending`.
    #[test]
    fn deliver_then_take_pending_roundtrips_plaintext() {
        let (private_key, p256dh, auth) = push_crypto::test_keypair();
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep", &p256dh, &auth, &private_key, true);
        let payload = push_crypto::encrypt_for_test(&p256dh, &auth, b"hello from push service").unwrap();
        assert!(store.push_deliver("https://x.test", "/", &payload));
        assert_eq!(
            store.push_take_pending("https://x.test", "/"),
            Some(b"hello from push service".to_vec())
        );
        assert_eq!(store.push_take_pending("https://x.test", "/"), None);
    }

    #[test]
    fn deliver_without_subscription_fails() {
        let store = make();
        let (_, p256dh, auth) = push_crypto::test_keypair();
        let payload = push_crypto::encrypt_for_test(&p256dh, &auth, b"hi").unwrap();
        assert!(!store.push_deliver("https://x.test", "/", &payload));
    }

    #[test]
    fn deliver_with_corrupted_payload_fails_and_queues_nothing() {
        let (private_key, p256dh, auth) = push_crypto::test_keypair();
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep", &p256dh, &auth, &private_key, true);
        assert!(!store.push_deliver("https://x.test", "/", b"not a valid webpush body"));
        assert_eq!(store.push_take_pending("https://x.test", "/"), None);
    }

    #[test]
    fn take_pending_is_fifo_across_multiple_deliveries() {
        let (private_key, p256dh, auth) = push_crypto::test_keypair();
        let store = make();
        store.push_subscribe("https://x.test", "/", "ep", &p256dh, &auth, &private_key, true);
        let first = push_crypto::encrypt_for_test(&p256dh, &auth, b"first").unwrap();
        let second = push_crypto::encrypt_for_test(&p256dh, &auth, b"second").unwrap();
        assert!(store.push_deliver("https://x.test", "/", &first));
        assert!(store.push_deliver("https://x.test", "/", &second));
        assert_eq!(store.push_take_pending("https://x.test", "/"), Some(b"first".to_vec()));
        assert_eq!(store.push_take_pending("https://x.test", "/"), Some(b"second".to_vec()));
    }

    #[test]
    fn take_pending_without_subscription_is_none() {
        let store = make();
        assert_eq!(store.push_take_pending("https://x.test", "/"), None);
    }

    #[test]
    fn deliver_is_isolated_per_scope() {
        let (private_key, p256dh, auth) = push_crypto::test_keypair();
        let store = make();
        store.push_subscribe("https://x.test", "/a/", "ep", &p256dh, &auth, &private_key, true);
        let (private_key_b, p256dh_b, auth_b) = push_crypto::test_keypair();
        store.push_subscribe("https://x.test", "/b/", "ep", &p256dh_b, &auth_b, &private_key_b, true);
        let payload = push_crypto::encrypt_for_test(&p256dh, &auth, b"for-a").unwrap();
        assert!(store.push_deliver("https://x.test", "/a/", &payload));
        assert_eq!(store.push_take_pending("https://x.test", "/b/"), None);
        assert_eq!(store.push_take_pending("https://x.test", "/a/"), Some(b"for-a".to_vec()));
    }
}
