//! Network surface of a dedicated or shared `WorkerGlobalScope` — `fetch()` and
//! `XMLHttpRequest` over the synchronous `JsFetchProvider` bridge (BUG-778),
//! built on the page's own `Headers`/`Response`/`Request` (WORKER-1 срез 5).
//!
//! Those classes reach the worker as verbatim slices of the page shim
//! ([`crate::dom::HEADERS_SHIM`], [`crate::dom::FETCH_BODY_SHIM`], installed by
//! [`crate::dom::install_worker_exposed_v8`]). Their network factory,
//! `_lumen_response_from_fetch_cache`, pulls the body through the
//! `_lumen_stream_*` natives out of "the last fetched body" — so this module
//! registers natives of exactly that shape over a per-scope [`BodyStore`], and
//! the worker's `fetch()` produces the same `Response` a page `fetch()` does
//! instead of the old hand-written mini-class (whose `new Response(buffer)`
//! had an empty body and whose status 0 turned into 200).
//!
//! The service-worker scope keeps its own `fetch`/`Response`
//! ([`crate::sw_worker`]) — its responses travel back to the page through
//! `respondWith`, a contract this module does not touch.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn5};
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsResult;
use lumen_core::ext::JsRuntime as _;

/// `fetch()`/`XMLHttpRequest` for a worker scope, over the natives
/// [`install_worker_net_v8`] registers. Read verbatim (`include_str!`), so
/// nothing in it is escaped.
pub(crate) const WORKER_NET_SHIM: &str = include_str!("shim/worker_net_shim.js");

/// Response bodies of one worker scope. `last` is the body of the most recent
/// `_lumen_worker_net_fetch` — the worker's counterpart of the page's single
/// `FetchCache` slot; `_lumen_stream_alloc` moves it into its own slot so a
/// later fetch cannot overwrite a body still being read.
#[derive(Default)]
struct BodyStore {
    last: Vec<u8>,
    slots: HashMap<u32, Vec<u8>>,
    next: u32,
}

/// `body[offset .. offset + size]`, clamped to the body.
fn chunk(body: &[u8], offset: u32, size: u32) -> Vec<u8> {
    let start = (offset as usize).min(body.len());
    let end = start.saturating_add(size as usize).min(body.len());
    body[start..end].to_vec()
}

/// Performs one worker request and returns its metadata as JSON
/// `{status, statusText, headers: [[name, value]…], url, redirected}`; the body
/// goes to `store.last`. `None` is a network error (no provider, or the
/// provider failed) — the shim turns it into `TypeError`/`error`.
fn net_fetch(
    provider: Option<&dyn lumen_core::ext::JsFetchProvider>,
    store: &Mutex<BodyStore>,
    url: &str,
    method: &str,
    headers_flat: &[String],
    body: Option<&[u8]>,
    content_type: &str,
) -> Option<String> {
    let provider = provider?;
    let headers: Vec<(String, String)> = headers_flat
        .chunks_exact(2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    let req = lumen_core::ext::JsFetchRequest {
        url,
        method,
        headers: &headers,
        body: body.map(|bytes| lumen_core::ext::JsFetchBody { content_type, bytes }),
        mode: "",
        destination: "",
        token: None,
    };
    let resp = match provider.fetch_request(&req) {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("[worker] fetch: {url}: {e}");
            return None;
        }
    };
    let pairs: Vec<[String; 2]> = resp.headers.into_iter().map(|(k, v)| [k, v]).collect();
    // BUG-984: `resp.url` is the final hop; a test double may leave it empty.
    let final_url = if resp.url.is_empty() { url.to_string() } else { resp.url };
    let redirected = final_url != url;
    if let Ok(mut s) = store.lock() {
        s.last = resp.body;
    }
    serde_json::to_string(&serde_json::json!({
        "status": resp.status,
        "statusText": resp.status_text,
        "headers": pairs,
        "url": final_url,
        "redirected": redirected,
    }))
    .ok()
}

/// Registers the worker's network natives and evaluates [`WORKER_NET_SHIM`].
///
/// Natives: `_lumen_worker_net_fetch(url, method, headers_flat, body, content_type)`,
/// the page-shaped `_lumen_fetch_body_length`/`_lumen_fetch_body_chunk` (the
/// last body) and `_lumen_stream_alloc`/`_length`/`_chunk`/`_free` (a body
/// moved into its own slot). Call after [`crate::dom::install_worker_exposed_v8`]
/// (the shim builds on its `Response`/`Request`) and after the flavour's
/// `_lumen_worker_base_url` global is set.
pub(crate) fn install_worker_net_v8(
    rt: &V8JsRuntime,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    let store: Arc<Mutex<BodyStore>> = Arc::new(Mutex::new(BodyStore::default()));

    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_worker_net_fetch",
        into_v8_fn5(
            move |url: String,
                  method: String,
                  headers_flat: Vec<String>,
                  body: Option<Vec<u8>>,
                  content_type: String|
                  -> Option<String> {
                net_fetch(fetch_provider.as_deref(), &st, &url, &method, &headers_flat, body.as_deref(), &content_type)
            },
        ),
    )?;

    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_fetch_body_length",
        into_v8_fn0(move || -> u32 { st.lock().map_or(0, |s| s.last.len() as u32) }),
    )?;
    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_fetch_body_chunk",
        into_v8_fn2(move |offset: u32, size: u32| -> Vec<u8> {
            st.lock().map_or_else(|_| Vec::new(), |s| chunk(&s.last, offset, size))
        }),
    )?;

    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_stream_alloc",
        into_v8_fn0(move || -> u32 {
            let Ok(mut s) = st.lock() else { return 0 };
            let body = std::mem::take(&mut s.last);
            if body.is_empty() {
                return 0;
            }
            s.next += 1;
            let id = s.next;
            s.slots.insert(id, body);
            id
        }),
    )?;
    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_stream_length",
        into_v8_fn1(move |id: u32| -> u32 {
            st.lock().map_or(0, |s| s.slots.get(&id).map_or(0, |b| b.len() as u32))
        }),
    )?;
    let st = Arc::clone(&store);
    rt.register_native(
        "_lumen_stream_chunk",
        into_v8_fn3(move |id: u32, offset: u32, size: u32| -> Vec<u8> {
            st.lock()
                .map_or_else(|_| Vec::new(), |s| s.slots.get(&id).map_or_else(Vec::new, |b| chunk(b, offset, size)))
        }),
    )?;
    let st = store;
    rt.register_native(
        "_lumen_stream_free",
        into_v8_fn1(move |id: u32| {
            if let Ok(mut s) = st.lock() {
                s.slots.remove(&id);
            }
        }),
    )?;

    rt.eval(WORKER_NET_SHIM)?;
    Ok(())
}

#[cfg(test)]
#[path = "worker_net_tests.rs"]
mod tests;
