//! Process-global network-intercept registry (WebDriver BiDi `network.addIntercept` +
//! `continueRequest`/`failRequest`, BUG-295 remainder).
//!
//! Same process-global shape as the offline/UA-override toggles in `lib.rs`
//! (`GLOBAL_OFFLINE`/`GLOBAL_UA_OVERRIDE`): the BiDi session's intercept rules
//! are synced here so [`pause_for_intercept`] — called from the one
//! `fetch_with_redirect` chokepoint every fetch path already funnels through —
//! sees them regardless of which `HttpClient`/thread issues the request.
//!
//! Unlike the offline/UA toggles, an intercept match doesn't just flip a flag:
//! it genuinely pauses the calling thread until a BiDi client resolves it
//! (`resolve_intercept`) or [`INTERCEPT_DECISION_TIMEOUT`] elapses. The
//! calling thread is always fetch()'s own worker thread (see
//! `crates/js/src/dom.rs`'s async fetch bridge), never the JS engine or UI
//! thread, so this pause cannot freeze the page or the browser chrome.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, OnceLock, RwLock};
use std::time::Duration;

/// One network intercept rule registered via `network.addIntercept` and
/// synced from the live BiDi session (`BidiState::intercepts`) to this
/// process-global registry.
#[derive(Clone, Debug)]
pub struct GlobalIntercept {
    /// Opaque intercept identifier (BiDi `intercept`), used only for removal.
    pub id: String,
    /// Phases at which this rule applies. Only `"beforeRequestSent"` is
    /// currently paused on — `"responseStarted"`/`"authRequired"` are
    /// accepted (stored) but not yet acted on, an explicit, narrower residual
    /// than before this fix (see `bugs/BUG-295-FIXED.md`).
    pub phases: Vec<String>,
    /// URL patterns to match (BiDi urlPattern `type: "string"` — exact match
    /// against the request's serialized URL). Empty means match-all.
    pub url_patterns: Vec<String>,
}

/// Decision delivered for a paused request via `network.continueRequest`
/// (`Continue`) or `network.failRequest` (`Fail`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterceptDecision {
    /// Let the request proceed unmodified.
    Continue,
    /// Fail the request as if blocked by the client.
    Fail,
}

/// Maximum time a request pauses waiting for a BiDi client's decision before
/// it's treated as failed. Not mandated by WebDriver BiDi (a real client is
/// expected to decide promptly) — bounds a client that registers an intercept
/// and never resolves it, so one forgotten intercept can't hang a fetch
/// forever (same spirit as `NAVIGATE_LOAD_TIMEOUT_MS`/`DEFAULT_TIMEOUT`
/// elsewhere in the automation stack).
const INTERCEPT_DECISION_TIMEOUT: Duration = Duration::from_secs(30);

static GLOBAL_INTERCEPTS: RwLock<Vec<GlobalIntercept>> = RwLock::new(Vec::new());

/// Register (or replace, if `id` already exists) a global intercept rule.
pub fn add_global_intercept(rule: GlobalIntercept) {
    if let Ok(mut rules) = GLOBAL_INTERCEPTS.write() {
        rules.retain(|r| r.id != rule.id);
        rules.push(rule);
    }
}

/// Remove a previously registered global intercept rule by id. No-op if unknown.
pub fn remove_global_intercept(id: &str) {
    if let Ok(mut rules) = GLOBAL_INTERCEPTS.write() {
        rules.retain(|r| r.id != id);
    }
}

/// Whether any registered rule matches `url` for `phase`.
fn matches_active_intercept(url: &str, phase: &str) -> bool {
    GLOBAL_INTERCEPTS.read().is_ok_and(|rules| {
        rules.iter().any(|r| {
            r.phases.iter().any(|p| p == phase)
                && (r.url_patterns.is_empty() || r.url_patterns.iter().any(|p| p == url))
        })
    })
}

/// One request currently paused awaiting a BiDi decision.
struct PendingIntercept {
    /// URL of the paused request (`network.beforeRequestSent` event data).
    url: String,
    /// `None` while paused; set once `resolve_intercept` delivers a decision.
    decision: Option<InterceptDecision>,
    /// Whether [`drain_new_intercept_announcements`] has already reported this pause.
    announced: bool,
}

/// Registry of in-flight paused requests, keyed by opaque request id, plus
/// the condvar the waiting fetch thread(s) block on.
struct InterceptRegistry {
    pending: HashMap<String, PendingIntercept>,
    next_id: u64,
}

/// `(registry, decision_condvar, registered_condvar)`: `decision_condvar` is
/// notified by [`resolve_intercept`] when a pause's decision is set (what
/// [`pause_for_intercept`]'s own wait blocks on); `registered_condvar` is
/// notified by [`pause_for_intercept`] when a new pause is inserted (what
/// [`wait_for_new_intercept_announcement`] blocks on) — two separate condvars
/// on the same mutex so a decision-resolve can't spuriously wake a caller
/// that is only waiting for a new registration, and vice versa.
fn registry() -> &'static (Mutex<InterceptRegistry>, Condvar, Condvar) {
    static REGISTRY: OnceLock<(Mutex<InterceptRegistry>, Condvar, Condvar)> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        (
            Mutex::new(InterceptRegistry { pending: HashMap::new(), next_id: 1 }),
            Condvar::new(),
            Condvar::new(),
        )
    })
}

/// If `url` matches an active intercept rule for `phase`, register a pending
/// pause and block the calling thread until [`resolve_intercept`] decides it
/// or [`INTERCEPT_DECISION_TIMEOUT`] elapses. Returns `Ok(())` to continue the
/// request unmodified, `Err(reason)` to fail it — same contract the
/// offline/mixed-content/filter blocks in `fetch_with_redirect` already
/// follow (caller propagates as a network error, no `RequestStarted`/
/// `RequestCompleted`).
///
/// A no-op (`Ok(())`) when nothing matches — the common case, so every fetch
/// pays only one `RwLock::read` + a linear scan of (usually zero) rules.
pub fn pause_for_intercept(url: &str, phase: &str) -> Result<(), String> {
    if !matches_active_intercept(url, phase) {
        return Ok(());
    }
    let (mutex, decision_condvar, registered_condvar) = registry();

    let request_id = {
        let Ok(mut reg) = mutex.lock() else {
            return Err("intercept registry poisoned".to_owned());
        };
        let id = format!("intercept-{}", reg.next_id);
        reg.next_id += 1;
        reg.pending
            .insert(id.clone(), PendingIntercept { url: url.to_owned(), decision: None, announced: false });
        id
    };
    registered_condvar.notify_all();

    let Ok(guard) = mutex.lock() else {
        return Err("intercept registry poisoned".to_owned());
    };
    let wait_result = decision_condvar.wait_timeout_while(guard, INTERCEPT_DECISION_TIMEOUT, |reg| {
        reg.pending.get(&request_id).is_some_and(|p| p.decision.is_none())
    });
    let Ok((mut guard, timeout)) = wait_result else {
        return Err("intercept registry poisoned".to_owned());
    };
    let decision = guard.pending.remove(&request_id).and_then(|p| p.decision);
    drop(guard);

    match decision {
        Some(InterceptDecision::Continue) => Ok(()),
        Some(InterceptDecision::Fail) => {
            Err(format!("net::ERR_BLOCKED_BY_CLIENT (intercept {request_id})"))
        }
        None => {
            debug_assert!(timeout.timed_out(), "removed without a decision but not timed out");
            Err(format!(
                "net::ERR_BLOCKED_BY_CLIENT (intercept {request_id} timed out awaiting a decision)"
            ))
        }
    }
}

/// Deliver a decision for a paused request (`network.continueRequest`/
/// `network.failRequest`). Returns `true` if `request_id` matched an
/// outstanding pause, `false` for an unknown id — an unknown id is not an
/// error at this layer (the BiDi handler ACKs either way, mirroring the
/// bare-ACK tolerance it already had before real bookkeeping existed).
pub fn resolve_intercept(request_id: &str, decision: InterceptDecision) -> bool {
    let (mutex, decision_condvar, _registered_condvar) = registry();
    let Ok(mut reg) = mutex.lock() else { return false };
    let Some(pending) = reg.pending.get_mut(request_id) else { return false };
    pending.decision = Some(decision);
    drop(reg);
    decision_condvar.notify_all();
    true
}

/// Drain requests newly paused since the last call — data for the
/// `network.beforeRequestSent` event a BiDi connection should deliver.
/// Each pending request is reported at most once.
pub fn drain_new_intercept_announcements() -> Vec<(String, String)> {
    let (mutex, _decision_condvar, _registered_condvar) = registry();
    let Ok(mut reg) = mutex.lock() else { return Vec::new() };
    reg.pending
        .iter_mut()
        .filter(|(_, p)| !p.announced)
        .map(|(id, p)| {
            p.announced = true;
            (id.clone(), p.url.clone())
        })
        .collect()
}

/// Block until a request newly paused (since the last announcement) appears,
/// or `timeout` elapses — the event-driven counterpart to
/// [`drain_new_intercept_announcements`]'s poll. `pause_for_intercept` notifies
/// `registered_condvar` right after inserting a pending pause, so this wakes
/// as soon as one exists instead of on a fixed poll schedule; that schedule
/// (`for _ in 0..N { drain(); sleep(5ms) }`) is what made
/// `global_intercept_pauses_real_fetch_until_resolved` and this module's own
/// blocking tests flake under `scoped-test.sh`'s parallel load (BUG-783): the
/// fetch thread could simply not get scheduled within the fixed wall-clock
/// budget. Returns an empty `Vec` on timeout, same as an empty poll loop.
pub fn wait_for_new_intercept_announcement(timeout: Duration) -> Vec<(String, String)> {
    let (mutex, _decision_condvar, registered_condvar) = registry();
    let Ok(guard) = mutex.lock() else { return Vec::new() };
    let wait_result =
        registered_condvar.wait_timeout_while(guard, timeout, |reg| !reg.pending.values().any(|p| !p.announced));
    let Ok((mut guard, _timeout)) = wait_result else { return Vec::new() };
    guard
        .pending
        .iter_mut()
        .filter(|(_, p)| !p.announced)
        .map(|(id, p)| {
            p.announced = true;
            (id.clone(), p.url.clone())
        })
        .collect()
}

/// Serializes every test (in this module AND `lib.rs`'s own
/// `global_intercept_pauses_real_fetch_until_resolved`) that touches
/// `GLOBAL_INTERCEPTS`/the pending-request registry — both are process-wide,
/// so two such tests running concurrently under cargo's default parallel
/// test execution would double-count each other's `drain_new_intercept_
/// announcements()` results or have one test's `cleanup()` clear state a
/// concurrently-running test still needs (same rationale as
/// `lib.rs::tests::BUG_295_GLOBAL_TEST_LOCK`, a separate lock for the
/// offline/UA-override globals).
#[cfg(test)]
pub(crate) static GLOBAL_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;

    fn cleanup() {
        if let Ok(mut rules) = GLOBAL_INTERCEPTS.write() {
            rules.clear();
        }
        let (mutex, _, _) = registry();
        if let Ok(mut reg) = mutex.lock() {
            reg.pending.clear();
        }
    }

    #[test]
    fn non_matching_url_is_noop() {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        add_global_intercept(GlobalIntercept {
            id: "i1".into(),
            phases: vec!["beforeRequestSent".into()],
            url_patterns: vec!["http://example.test/only-this".into()],
        });
        assert_eq!(pause_for_intercept("http://example.test/other", "beforeRequestSent"), Ok(()));
        cleanup();
    }

    #[test]
    fn continue_decision_unblocks_with_ok() {
        // Guard held for the whole test (not just around each call) — a
        // background thread keeps the pause open across the polling loop
        // below, a window another lock-holding test must not run inside.
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        // A specific (non-match-all) URL — the pause stays open for up to
        // ~1s below; an empty `url_patterns` would match ANY concurrently
        // running *unrelated* test's real fetch too (those don't take this
        // lock, since they don't know about intercepts at all).
        let url = "http://example.test/continue-decision-unblocks-with-ok";
        add_global_intercept(GlobalIntercept {
            id: "i2".into(),
            phases: vec!["beforeRequestSent".into()],
            url_patterns: vec![url.to_owned()],
        });
        let handle = thread::spawn(move || pause_for_intercept(url, "beforeRequestSent"));
        // Wait until the request is registered, then resolve it as Continue.
        let announced = wait_for_new_intercept_announcement(StdDuration::from_secs(5));
        assert_eq!(announced.len(), 1, "expected exactly one paused request to be announced");
        let (request_id, seen_url) = &announced[0];
        assert_eq!(seen_url, url);
        assert!(resolve_intercept(request_id, InterceptDecision::Continue));
        assert_eq!(handle.join().expect("thread panicked"), Ok(()));
        cleanup();
    }

    #[test]
    fn fail_decision_unblocks_with_err() {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        let url = "http://example.test/fail-decision-unblocks-with-err";
        add_global_intercept(GlobalIntercept {
            id: "i3".into(),
            phases: vec!["beforeRequestSent".into()],
            url_patterns: vec![url.to_owned()],
        });
        let handle = thread::spawn(move || pause_for_intercept(url, "beforeRequestSent"));
        let announced = wait_for_new_intercept_announcement(StdDuration::from_secs(5));
        let request_id = announced.into_iter().next().map(|(id, _)| id).expect("request never registered");
        assert!(resolve_intercept(&request_id, InterceptDecision::Fail));
        let result = handle.join().expect("thread panicked");
        assert!(result.is_err());
        cleanup();
    }

    #[test]
    fn unknown_request_id_resolve_returns_false() {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        assert!(!resolve_intercept("nonexistent-request-id", InterceptDecision::Fail));
    }

    #[test]
    fn phase_mismatch_does_not_pause() {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        add_global_intercept(GlobalIntercept {
            id: "i4".into(),
            phases: vec!["responseStarted".into()],
            url_patterns: vec![],
        });
        assert_eq!(pause_for_intercept("http://example.test/page", "beforeRequestSent"), Ok(()));
        cleanup();
    }

    #[test]
    fn remove_global_intercept_stops_matching() {
        let _guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cleanup();
        add_global_intercept(GlobalIntercept {
            id: "i5".into(),
            phases: vec!["beforeRequestSent".into()],
            url_patterns: vec![],
        });
        remove_global_intercept("i5");
        assert_eq!(pause_for_intercept("http://example.test/page", "beforeRequestSent"), Ok(()));
        cleanup();
    }
}
