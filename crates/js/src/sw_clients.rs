//! SW-MSG-S4: outbound message queue from service worker threads to pages
//! (Service Workers §3.2.5 `Client.postMessage`).
//!
//! Routing model mirrors `broadcast_channel.rs`'s process-global hub: a SW
//! thread's `Client.prototype.postMessage` calls the `_lumen_sw_post_to_client`
//! native, which pushes the (client_id, scope, payload) triple into
//! [`HUB`]; the destination page's own pump
//! (`V8JsRuntime::pump_workers` → [`deliver`]) takes the entries matching
//! its `sw_client_id` and hands them to the page shim
//! (`_lumen_deliver_sw_messages`), which fires `message` on
//! `navigator.serviceWorker`.
//!
//! There is no wake-up channel from the SW thread to the page — delivery
//! happens on the page's next tick, the same async shape `Worker`'s
//! outbound queue (`worker.rs`) already has. Entries for a client that died
//! before delivery stay in the hub until its id matches no live pump; the
//! client registry itself arrives with SW-MSG-S6.

use std::sync::{Mutex, OnceLock};

#[cfg(feature = "v8-backend")]
use lumen_core::JsRuntime;

/// One undelivered SW→page message.
struct SwOutMessage {
    /// The page client the message is addressed to (`Client.id` of the
    /// `source` descriptor the page stamped on the reverse-direction
    /// envelope).
    client_id: u64,
    /// The registration scope of the posting worker — the page shim looks
    /// the `ServiceWorker` event `source` up by it.
    scope: String,
    /// Payload, already JSON-encoded by the SW shim (`structuredClone` +
    /// `JSON.stringify`, the same wire compromise as `Worker.postMessage`).
    data_json: String,
}

#[derive(Default)]
struct SwHub {
    outbox: Vec<SwOutMessage>,
}

static HUB: OnceLock<Mutex<SwHub>> = OnceLock::new();

/// Lazily-initialised handle to the process-global SW→page hub.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn hub() -> &'static Mutex<SwHub> {
    HUB.get_or_init(|| Mutex::new(SwHub::default()))
}

/// Queue a message a service worker posted to a page client (SW §3.2.5).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn push_out(client_id: u64, scope: String, data_json: String) {
    if let Ok(mut h) = hub().lock() {
        h.outbox.push(SwOutMessage { client_id, scope, data_json });
    }
}

/// Remove (and return, in posting order) everything queued for `client_id`.
#[cfg(feature = "v8-backend")]
fn take_for(client_id: u64) -> Vec<(String, String)> {
    let Ok(mut h) = hub().lock() else { return Vec::new() };
    let (mut taken, mut kept) = (Vec::new(), Vec::new());
    for m in h.outbox.drain(..) {
        if m.client_id == client_id {
            taken.push((m.scope, m.data_json));
        } else {
            kept.push(m);
        }
    }
    h.outbox = kept;
    taken
}

/// Deliver everything queued for `client_id` to the page behind `rt` by
/// calling `_lumen_deliver_sw_messages([{scope, json}, …])` — `json` is
/// embedded as raw JSON, so the shim reads it already parsed (BUG-1814's
/// no-re-parse convention).
#[cfg(feature = "v8-backend")]
pub(crate) fn deliver(rt: &crate::v8_runtime::V8JsRuntime, client_id: u64) {
    let items = take_for(client_id);
    if items.is_empty() {
        return;
    }
    let arr: Vec<serde_json::Value> = items
        .iter()
        .map(|(scope, data_json)| {
            serde_json::json!({
                "scope": scope,
                "json": serde_json::from_str::<serde_json::Value>(data_json)
                    .unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();
    let json = serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string());
    let _ = rt.eval(&format!(
        "if(typeof _lumen_deliver_sw_messages==='function')_lumen_deliver_sw_messages({json})"
    ));
}
