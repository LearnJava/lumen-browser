//! `window.open()` / `window.opener` cross-tab `postMessage` (BUG-797,
//! GAP-NAVCTX срез 4).
//!
//! **Why this cannot be `frame_bridge.rs`'s model verbatim.** `FRAME_OUTBOX`
//! (`frame_bridge.rs`) addresses a sibling *document* by the pointer of its
//! shared `Arc<Mutex<Document>>`, and every frame of the active tab is pumped
//! every tick via `_lumen_frame_pump_messages()` — cheap, because all of them
//! belong to the one `js_ctx` the shell is already driving. A `window.open()`
//! target is a different *tab*, and per ADR-016 M2.2 the shell drives exactly
//! one live `js_ctx` at a time; every other tab's runtime sits parked inside
//! `Lumen::bg_tabs` (`PageSnapshot::js_ctx`), acted on only by direct calls
//! from shell code (the same technique `switch_tab` already uses to run a GC
//! pass on a tab that just went to the background). So this hub is addressed
//! by **tab id** (`u32`, the `TabEntry::id` HTML LS calls the browsing
//! context), not by a document pointer, and delivery is not a JS-side pump —
//! it is the shell iterating its own tab table each tick and calling
//! `PersistentJs::eval_js` directly on whichever handle (active or parked)
//! owns the target id.
//!
//! **The token indirection.** `window.open()` returns its `WindowProxy` stub
//! synchronously, but the shell does not create the actual tab until it drains
//! `window_open_requests` on a later tick (`about_to_wait`) — so the stub
//! cannot be given a real tab id yet. It is given a `token` instead
//! (`alloc_token`), and a script that calls `.postMessage()` on it before the
//! tab exists queues under that token (`post_to_token`) until the shell calls
//! `resolve_token` once the tab is created, which both remembers the
//! token↔tab-id pair (for the reverse direction below) and flushes the queued
//! messages to the now-real tab id.
//!
//! **The reverse direction.** A popup posting to `window.opener` addresses its
//! *opener's* tab id directly — the popup is told that id right after creation
//! (`_lumen_opener_tab_id`, set once via the shell's follow-up `eval_js`, the
//! same place that resolves the token above). For the opener's delivered
//! `MessageEvent.source` to be the exact object its own `window.open()` call
//! returned, the delivery has to name that object by the same `token` the
//! opener's shim closed over — so `post_to_tab` also takes the popup's own tab
//! id and the shell converts it to a token via [`token_for_tab`] when it
//! builds the delivery script.
//!
//! **Known remaining gap.** The follow-up `eval_js` that plants
//! `_lumen_own_tab_id`/`_lumen_opener_tab_id` on the popup runs after
//! `Lumen::navigate_to` returns from creating it. A synchronous top-of-page
//! script that calls `window.opener.postMessage(...)` before yielding once
//! races this — such a call currently sees `_lumen_opener_tab_id` still
//! unset and silently drops. Deferred/`onload`-driven posts (the common case
//! and everything the srez's live probe exercises) are unaffected. Closing
//! this needs the id threaded through `install_dom` itself, out of scope here.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering};

/// Per-target queue cap. A page that floods `postMessage` before its target
/// ever drains loses the overflow silently rather than growing unbounded —
/// same trade-off as `frame_bridge.rs`'s `FRAME_OUTBOX_CAP`.
const QUEUE_CAP: usize = 256;

/// Which side originated a queued message, and what the *receiving* side
/// needs in order to build the right `MessageEvent.source`.
#[derive(Debug, Clone)]
enum Origin {
    /// From the opener, to a popup. The popup has exactly one opener, so no
    /// further addressing is needed — the shim answers with `window.opener`.
    FromOpener,
    /// From a popup, to its opener. `token` is the same value the opener's
    /// `window.open()` call returned, so the delivery script can look the
    /// exact stub object back up in the opener's own registry.
    FromChild { token: u32 },
}

/// One queued cross-tab message awaiting delivery.
#[derive(Debug, Clone)]
struct Pending {
    origin: Origin,
    /// Structured-clone payload, already `JSON.stringify`-ed by the sender.
    json: String,
    /// Sender's origin string, for `MessageEvent.origin`.
    from_origin: String,
}

#[derive(Default)]
struct Hub {
    /// Target tab id -> queued messages, ready for the next drain.
    by_tab: HashMap<u32, Vec<Pending>>,
    /// Target token -> queued messages, for a popup the shell has not yet
    /// created a tab id for.
    by_token: HashMap<u32, Vec<Pending>>,
    /// Token -> tab id, once the shell has created the popup.
    token_to_tab: HashMap<u32, u32>,
    /// Tab id -> token, the reverse of the above — needed so a message
    /// *from* the popup can be delivered to the opener addressed by the same
    /// token the opener's stub closed over.
    tab_to_token: HashMap<u32, u32>,
}

static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
static NEXT_TOKEN: AtomicU32 = AtomicU32::new(1);

fn hub() -> &'static Mutex<Hub> {
    HUB.get_or_init(|| Mutex::new(Hub::default()))
}

/// Mint a fresh token for a `window.open()` call, before the shell has
/// created the resulting tab.
pub fn alloc_token() -> u32 {
    NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
}

/// Record that `token` now names the real tab `tab_id`, and flush any
/// messages queued under the token to that tab's queue.
///
/// Called once by the shell, right after it creates the popup tab
/// (`about_to_wait`'s `window_open_requests` drain).
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn resolve_token(token: u32, tab_id: u32) {
    let mut h = hub().lock().unwrap();
    h.token_to_tab.insert(token, tab_id);
    h.tab_to_token.insert(tab_id, token);
    if let Some(pending) = h.by_token.remove(&token) {
        let queue = h.by_tab.entry(tab_id).or_default();
        for msg in pending {
            if queue.len() >= QUEUE_CAP {
                break;
            }
            queue.push(msg);
        }
    }
}

/// Look up the token the opener's `window.open()` call received for the
/// popup now known as `tab_id` — `None` if `tab_id` was never a popup (or its
/// mapping has not been resolved yet).
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn token_for_tab(tab_id: u32) -> Option<u32> {
    hub().lock().unwrap().tab_to_token.get(&tab_id).copied()
}

/// Queue a message from the opener to the popup addressed by `token`
/// (`window.open()`'s return value calling `.postMessage()`).
///
/// Queued under the token if the popup's tab id is not resolved yet;
/// [`resolve_token`] flushes it once it is.
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn post_to_token(token: u32, json: String, from_origin: String) {
    let mut h = hub().lock().unwrap();
    let msg = Pending { origin: Origin::FromOpener, json, from_origin };
    if let Some(&tab_id) = h.token_to_tab.get(&token) {
        let queue = h.by_tab.entry(tab_id).or_default();
        if queue.len() < QUEUE_CAP {
            queue.push(msg);
        }
    } else {
        let queue = h.by_token.entry(token).or_default();
        if queue.len() < QUEUE_CAP {
            queue.push(msg);
        }
    }
}

/// Queue a message from the popup `from_tab_id` to its opener `target_tab_id`
/// (`window.opener.postMessage()`).
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn post_to_opener(target_tab_id: u32, from_tab_id: u32, json: String, from_origin: String) {
    let mut h = hub().lock().unwrap();
    // The receiving opener's registry is keyed by the token its own
    // `window.open()` call returned, not by the popup's tab id — recover it
    // before the id disappears into the queue.
    let Some(&token) = h.tab_to_token.get(&from_tab_id) else {
        return;
    };
    let queue = h.by_tab.entry(target_tab_id).or_default();
    if queue.len() < QUEUE_CAP {
        queue.push(Pending { origin: Origin::FromChild { token }, json, from_origin });
    }
}

/// One message drained for delivery: `(is_from_opener, token_if_from_child,
/// json, from_origin)`. `token_if_from_child` is `0` (unused) when
/// `is_from_opener` is `true`.
pub type DrainedMessage = (bool, u32, String, String);

/// Drain every message addressed to `tab_id`, ready for the shell to turn
/// each into an `eval_js` call on that tab's (possibly parked) runtime.
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn drain(tab_id: u32) -> Vec<DrainedMessage> {
    let mut h = hub().lock().unwrap();
    let Some(pending) = h.by_tab.remove(&tab_id) else {
        return Vec::new();
    };
    pending
        .into_iter()
        .map(|p| match p.origin {
            Origin::FromOpener => (true, 0, p.json, p.from_origin),
            Origin::FromChild { token } => (false, token, p.json, p.from_origin),
        })
        .collect()
}

/// JSON-encode [`drain`] for the native binding: an array of `{fromOpener,
/// token, data, origin}` objects. `data` is parsed back out of the sender's
/// `JSON.stringify` text (mirrors `frame_bridge.rs`'s `_lumen_frame_take_messages`)
/// so the receiver's `JSON.parse` of the whole envelope hands back the real
/// structured-clone value, not a doubly-escaped string. Empty string (not
/// `"[]"`) when there is nothing to deliver, so the shim can skip `JSON.parse`
/// on the hot path where most ticks have no message at all.
pub fn drain_json(tab_id: u32) -> String {
    let items: Vec<serde_json::Value> = drain(tab_id)
        .into_iter()
        .map(|(from_opener, token, json, origin)| {
            serde_json::json!({
                "fromOpener": from_opener,
                "token": token,
                "data": serde_json::from_str::<serde_json::Value>(&json)
                    .unwrap_or(serde_json::Value::Null),
                "origin": origin,
            })
        })
        .collect();
    if items.is_empty() {
        return String::new();
    }
    serde_json::to_string(&items).unwrap_or_default()
}

/// Drop every mapping/queue that names `tab_id` — called when a tab closes so
/// the hub does not grow across a whole session's worth of popups.
#[allow(clippy::unwrap_used)] // унаследовано, docs/lint-policy.md §10
pub fn forget_tab(tab_id: u32) {
    let mut h = hub().lock().unwrap();
    h.by_tab.remove(&tab_id);
    if let Some(token) = h.tab_to_token.remove(&tab_id) {
        h.token_to_tab.remove(&token);
        h.by_token.remove(&token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test mints its own token/tab-id range via `alloc_token`/large
    // literals so the process-global hub's state from other tests sharing the
    // same `cargo test` binary cannot collide.

    #[test]
    fn message_to_unresolved_token_queues_then_flushes() {
        let token = alloc_token();
        post_to_token(token, "\"hi\"".to_string(), "https://opener.example".to_string());
        resolve_token(token, 900_001);
        let drained = drain(900_001);
        assert_eq!(drained.len(), 1);
        assert!(drained[0].0);
        assert_eq!(drained[0].2, "\"hi\"");
    }

    #[test]
    fn message_to_resolved_token_delivers_immediately() {
        let token = alloc_token();
        resolve_token(token, 900_002);
        post_to_token(token, "\"x\"".to_string(), "https://opener.example".to_string());
        let drained = drain(900_002);
        assert_eq!(drained.len(), 1);
    }

    #[test]
    fn child_to_opener_carries_the_opener_s_own_token() {
        let token = alloc_token();
        resolve_token(token, 900_003);
        post_to_opener(900_004, 900_003, "\"pong\"".to_string(), "https://popup.example".to_string());
        let drained = drain(900_004);
        assert_eq!(drained.len(), 1);
        assert!(!drained[0].0);
        assert_eq!(drained[0].1, token);
    }

    #[test]
    fn child_to_opener_without_a_resolved_token_is_dropped() {
        // from_tab_id never went through resolve_token — no token to answer
        // the opener's registry with, so the message cannot be delivered
        // meaningfully and is dropped rather than crashing.
        post_to_opener(900_005, 900_006, "\"lost\"".to_string(), "https://popup.example".to_string());
        assert!(drain(900_005).is_empty());
    }

    #[test]
    fn forget_tab_clears_token_and_queue() {
        let token = alloc_token();
        resolve_token(token, 900_007);
        forget_tab(900_007);
        assert_eq!(token_for_tab(900_007), None);
        post_to_token(token, "\"late\"".to_string(), "https://opener.example".to_string());
        // Forgotten tab id is no longer resolved, so this re-queues under the
        // token instead of being delivered to the (now defunct) tab id.
        assert!(drain(900_007).is_empty());
    }
}
