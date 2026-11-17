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

/// SW-MSG-S6: a live page client known to the hub (SW §5.2 `Client` / §5.3
/// `WindowClient`). `controlled_by` is the registration scope whose claim
/// (`clients.claim()`) took this client over; `None` until a claim lands —
/// the finer "controlled from activation" propagation is BUG-1742's
/// lifecycle rewrite, not this slice.
#[derive(Clone)]
pub(crate) struct SwClientInfo {
    pub id: u64,
    pub origin: String,
    pub url: String,
    pub visibility_state: String,
    pub focused: bool,
    pub controlled_by: Option<String>,
}

impl SwClientInfo {
    /// The JSON descriptor the SW shim reifies into a `WindowClient`.
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "url": self.url,
            "type": "window",
            "frameType": "top-level",
            "visibilityState": self.visibility_state,
            "focused": self.focused,
        })
    }
}

#[derive(Default)]
struct SwHub {
    /// SW-MSG-S6: live page clients in registration order (the order
    /// `matchAll` reports — "most recently focused" needs focus tracking
    /// the engine does not have yet).
    clients: Vec<SwClientInfo>,
    /// SW-MSG-S6: `clients.claim()` control messages awaiting delivery —
    /// the page shim's `_lumen_deliver_sw_control` fires `controllerchange`.
    controls: Vec<(u64, String)>,
    outbox: Vec<SwOutMessage>,
    /// SW-MODULE s4: main-script evaluation results from SW isolates,
    /// fanned out to every live client of the worker's origin — the page
    /// shim settles pending `register()`/`update()` promises with them.
    eval_results: Vec<(u64, String, bool)>,
    /// SW-MSG-S5: messages posted by a `MessagePort` living inside a SW
    /// isolate to its transferred partner on a page — routed by port id
    /// alone (the page's `_lumenPortRegistry` holds the endpoint), so the
    /// client id is not needed, mirroring the dedicated-worker
    /// `WorkerPortMessageQueue`.
    out_ports: Vec<(u32, String)>,
    /// SW-MSG-S5: which page (client id) holds the local endpoint of each
    /// bridged port. Port ids are unique only within one page's registry
    /// (`_lumenPortRegistry` per isolate), so the port-addressed outbox is
    /// delivered through this map — otherwise two pages could each claim the
    /// other's id-0 port.
    bound_ports: std::collections::HashMap<u64, std::collections::HashSet<u32>>,
}

static HUB: OnceLock<Mutex<SwHub>> = OnceLock::new();

/// Lazily-initialised handle to the process-global SW→page hub.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn hub() -> &'static Mutex<SwHub> {
    HUB.get_or_init(|| Mutex::new(SwHub::default()))
}

/// SW-MSG-S6: create or refresh this page's hub record. Called by the page
/// shim at init (`_sw_client_sync`), on `pushState` URL moves, and from the
/// runtime `Drop` (as [`unregister_client`]).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn upsert_client(id: u64, origin: String, url: String, visibility_state: String) {
    if let Ok(mut h) = hub().lock() {
        if let Some(c) = h.clients.iter_mut().find(|c| c.id == id) {
            c.url = url;
            c.visibility_state = visibility_state;
        } else {
            h.clients.push(SwClientInfo {
                id,
                origin,
                url,
                visibility_state,
                focused: false,
                controlled_by: None,
            });
        }
    }
}

/// SW-MSG-S6: the page runtime died — drop its hub record (its outbox
/// entries become undeliverable and are dropped lazily by [`take_for`]).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn unregister_client(id: u64) {
    if let Ok(mut h) = hub().lock() {
        h.clients.retain(|c| c.id != id);
    }
}

/// SW-MSG-S6: `clients.matchAll({type, includeUncontrolled})` — descriptor
/// JSON for this worker's origin, restricted to `type` `'window'`/`'all'`
/// (the engine has no worker/sharedworker clients) and, without
/// `includeUncontrolled`, to clients this scope has claimed.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn clients_json(
    origin: &str,
    scope: &str,
    type_filter: &str,
    include_uncontrolled: bool,
) -> Option<String> {
    if type_filter != "window" && type_filter != "all" {
        return Some("[]".to_string());
    }
    let h = hub().lock().ok()?;
    let list: Vec<serde_json::Value> = h
        .clients
        .iter()
        .filter(|c| {
            c.origin == origin
                && (include_uncontrolled || c.controlled_by.as_deref() == Some(scope))
        })
        .map(SwClientInfo::to_json)
        .collect();
    serde_json::to_string(&list).ok()
}

/// SW-MSG-S6: `clients.get(id)` — descriptor JSON for `id` within this
/// worker's origin, `None` for an unknown or foreign-origin client.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn client_json(origin: &str, id: u64) -> Option<String> {
    let h = hub().lock().ok()?;
    h.clients
        .iter()
        .find(|c| c.origin == origin && c.id == id)
        .map(SwClientInfo::to_json)
        .and_then(|j| serde_json::to_string(&j).ok())
}

/// SW-MSG-S6: `clients.claim()` — take over every same-origin client whose
/// url sits inside `scope` and is not yet controlled by this scope; each
/// takeover queues a control message the page's pump delivers as
/// `controllerchange`. Returns the number of clients claimed.
///
/// shortcut: "not controlled by a more specific registration" from SW
/// §5.1 claim step 4 is not modelled — single-registration pages are the
/// only shape the engine has (BUG-1742 owns the real registration model).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn claim(origin: &str, scope: &str) -> u32 {
    let mut claimed = 0;
    if let Ok(mut h) = hub().lock() {
        let mut newly: Vec<u64> = Vec::new();
        let prefix = format!("{origin}{scope}");
        for c in h.clients.iter_mut() {
            if c.origin == origin
                && c.url.starts_with(&prefix)
                && c.controlled_by.as_deref() != Some(scope)
            {
                c.controlled_by = Some(scope.to_string());
                newly.push(c.id);
            }
        }
        claimed = newly.len() as u32;
        for id in newly {
            h.controls.push((id, scope.to_string()));
        }
    }
    claimed
}

/// Drain the control messages queued for `client_id` (posting order).
#[cfg(feature = "v8-backend")]
fn take_controls(client_id: u64) -> Vec<String> {
    let Ok(mut h) = hub().lock() else { return Vec::new() };
    let (mut taken, mut kept) = (Vec::new(), Vec::new());
    for (cid, scope) in h.controls.drain(..) {
        if cid == client_id {
            taken.push(scope);
        } else {
            kept.push((cid, scope));
        }
    }
    h.controls = kept;
    taken
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

/// Queue a message a `MessagePort` inside a SW isolate posted to the page
/// endpoint of its bridge (SW-MSG-S5).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn push_out_port(port_id: u32, data_json: String) {
    if let Ok(mut h) = hub().lock() {
        h.out_ports.push((port_id, data_json));
    }
}

/// Record that the page behind `client_id` holds the local endpoint of
/// `port_id` (SW-MSG-S5) — its pump is the one allowed to drain that port's
/// outbox entries.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn bind_port(client_id: u64, port_id: u32) {
    if let Ok(mut h) = hub().lock() {
        h.bound_ports.entry(client_id).or_default().insert(port_id);
    }
}

/// SW-MODULE s4: report the main-script evaluation outcome of a SW isolate
/// (SW §9.2 "Expect evaluation failed") to every live client of `origin` —
/// the registering page (there may be several) settles its pending
/// `register()`/`update()` promise accordingly.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub fn push_eval_result(origin: &str, scope: String, ok: bool) {
    if let Ok(mut h) = hub().lock() {
        let ids: Vec<u64> =
            h.clients.iter().filter(|c| c.origin == origin).map(|c| c.id).collect();
        for id in ids {
            h.eval_results.push((id, scope.clone(), ok));
        }
    }
}

/// SW-MODULE s4: take the evaluation results queued for `client_id`.
#[cfg(feature = "v8-backend")]
fn take_eval_results(client_id: u64) -> Vec<(String, bool)> {
    let Ok(mut h) = hub().lock() else { return Vec::new() };
    let (mut taken, mut kept) = (Vec::new(), Vec::new());
    for (cid, scope, ok) in h.eval_results.drain(..) {
        if cid == client_id {
            taken.push((scope, ok));
        } else {
            kept.push((cid, scope, ok));
        }
    }
    h.eval_results = kept;
    taken
}

/// Deliver everything queued for `client_id` to the page behind `rt` by
/// calling `_lumen_deliver_sw_messages([{scope, json}, …])` — `json` is
/// embedded as raw JSON, so the shim reads it already parsed (BUG-1814's
/// no-re-parse convention).
#[cfg(feature = "v8-backend")]
pub(crate) fn deliver(rt: &crate::v8_runtime::V8JsRuntime, client_id: u64) {
    let items = take_for(client_id);
    let scopes = take_controls(client_id);
    let evals = take_eval_results(client_id);
    if items.is_empty() && scopes.is_empty() && evals.is_empty() {
        return;
    }
    if !items.is_empty() {
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
    // SW-MSG-S6: `clients.claim()` control messages — the page shim fires
    // `controllerchange` for scopes it has an active registration for.
    if !scopes.is_empty() {
        let json = serde_json::to_string(
            &scopes.iter().map(|s| serde_json::json!({ "scope": s })).collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());
        let _ = rt.eval(&format!(
            "if(typeof _lumen_deliver_sw_control==='function')_lumen_deliver_sw_control({json})"
        ));
    }
    // SW-MODULE s4: main-script evaluation results — the page shim settles
    // the pending `register()`/`update()` promises for each scope.
    if !evals.is_empty() {
        let arr: Vec<serde_json::Value> = evals
            .iter()
            .map(|(scope, ok)| serde_json::json!({ "scope": scope, "ok": ok }))
            .collect();
        let json = serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string());
        let _ = rt.eval(&format!(
            "if(typeof _lumen_deliver_sw_eval_results==='function')_lumen_deliver_sw_eval_results({json})"
        ));
    }
}

/// SW-MSG-S5: drain the port-addressed outbox entries whose endpoint is
/// registered to `client_id`'s page and hand them to
/// `_lumen_deliver_port_messages` (the same delivery function the dedicated
/// worker pump uses, routed by port id through `_lumenPortRegistry`).
/// Entries for other pages (or unbound ports) stay queued.
#[cfg(feature = "v8-backend")]
pub(crate) fn deliver_ports(rt: &crate::v8_runtime::V8JsRuntime, client_id: u64) {
    let items = {
        let Ok(mut h) = hub().lock() else { return };
        let Some(bound) = h.bound_ports.get(&client_id).cloned() else { return };
        let (mut taken, mut kept) = (Vec::new(), Vec::new());
        for e in h.out_ports.drain(..) {
            if bound.contains(&e.0) {
                taken.push(e);
            } else {
                kept.push(e);
            }
        }
        h.out_ports = kept;
        taken
    };
    if items.is_empty() {
        return;
    }
    let arr: Vec<serde_json::Value> = items
        .iter()
        .map(|(port_id, data_json)| {
            serde_json::json!({
                "id": port_id,
                "json": serde_json::from_str::<serde_json::Value>(data_json)
                    .unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();
    let json = serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string());
    let _ = rt.eval(&format!(
        "if(typeof _lumen_deliver_port_messages==='function')_lumen_deliver_port_messages({json})"
    ));
}
