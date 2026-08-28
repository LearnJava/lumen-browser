//! Секции `install_dom`: сеть — Service Worker, Fetch, WebSocket, SSE, буфер обмена.
//!
//! Вырезано из `V8JsRuntime::install_dom` батчем SPLIT-JS6 без правки тел:
//! секции жили внутри замыкания `self.run(…)` на отступе 4, то есть ровно на
//! отступе тела функции, а единственной правкой стала приставка контекста у
//! площадок `reg!` — см. [`super::reg`].

use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// Service Worker registration, script activation and Cache Storage.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_service_worker(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    fp_sw_net: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    idb_sw: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
) -> JsResult<()> {
    // ── Service Worker / Cache Storage ───────────────────────────────────────
    {
        // SW registrations: origin+scope+scriptUrl stored in-memory.
        // Key: (origin, scope) → script_url
        type SwMap = std::collections::HashMap<(String, String), String>;
        let sw_regs: Arc<Mutex<SwMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Cache storage: origin → cache_name → url → (method, meta_json, body)
        // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{…}}
        // method is stored separately for O(1) `keys()` without re-parsing meta_json.
        type CacheEntry = (String, String, Vec<u8>);
        type CacheMap = std::collections::HashMap<String, std::collections::HashMap<String, std::collections::HashMap<String, CacheEntry>>>;
        let cache_data: Arc<Mutex<CacheMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        let sw = Arc::clone(&sw_regs);
        reg!(scope, ctx, store, 
            "_lumen_sw_register",
            move |origin: String, scope: String, script_url: String| {
                sw.lock().unwrap().insert((origin, scope), script_url);
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(scope, ctx, store, 
            "_lumen_sw_has_registration",
            move |origin: String| -> bool {
                sw.lock().unwrap().keys().any(|(o, _)| *o == origin)
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(scope, ctx, store, 
            "_lumen_sw_unregister",
            move |origin: String, scope: String| {
                sw.lock().unwrap().remove(&(origin, scope));
            }
        );

        // Persistence bindings — forward to SwBackend when provided.
        let sw_be = sw_backend.clone();
        reg!(scope, ctx, store, 
            "_lumen_sw_persist",
            move |_origin: String, snapshot: String| {
                if let Some(ref be) = sw_be {
                    be.save(&snapshot);
                }
            }
        );

        let sw_be2 = sw_backend.clone();
        reg!(scope, ctx, store, 
            "_lumen_sw_load",
            move |_origin: String| -> Option<String> {
                sw_be2.as_ref().and_then(|be| be.load())
            }
        );

        // _lumen_sw_activate_script(origin, scope, script_text) — PH3-20: SW fetch interception.
        // Called from the _sw_run_lifecycle JS shim when a SW finishes the activate phase.
        // Spawns a dedicated V8 thread for the SW (Ph3 V8 migration S10 —
        // `spawn_sw_worker_v8`, replacing the QuickJS-only `spawn_sw_worker` this
        // call site used before S10 landed) and registers it in sw_worker_store.
        {
            let sws = sw_worker_store.clone();
            let cbe_sw = cache_backend.clone();
            // Тот же провайдер, что у страницы: без него в области воркера нет
            // ни `importScripts` по сети, ни настоящего `fetch`, и воркер,
            // подключающий библиотеку, умирает на первой строке.
            let fp_sw = fp_sw_net.clone();
            let idb_sw = idb_sw.clone();
            reg!(scope, ctx, store, "_lumen_sw_activate_script", move |origin: String, scope: String, text: String| {
                if let (Some(store), Some(cache)) = (sws.as_ref(), cbe_sw.as_ref()) {
                    let handle = crate::sw_worker::spawn_sw_worker_v8(
                        origin.clone(),
                        scope.clone(),
                        text,
                        Arc::clone(cache),
                        fp_sw.clone(),
                        idb_sw.clone(),
                    );
                    store.lock().unwrap().insert((origin, scope), handle);
                }
            });
        }

        // Dispatch helpers: use SQLite backend when provided, fall back to in-memory map.
        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_put",
            // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{...}}
            // Grouped into one string to stay within rquickjs 5-arg IntoJsFunc limit.
            move |origin: String, cache_name: String, url: String, meta_json: String, body: Vec<u8>| {
                if let Some(ref be) = cbe {
                    be.cache_put(&origin, &cache_name, &url, &meta_json, &body);
                } else {
                    let method = cache_meta_method(&meta_json);
                    cd.lock()
                        .unwrap()
                        .entry(origin)
                        .or_default()
                        .entry(cache_name)
                        .or_default()
                        .insert(url, (method, meta_json, body));
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_match",
            move |origin: String, cache_name: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(_, body)| body)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, _, body)| body.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_match_info",
            // Returns the raw meta_json stored at put time (already JSON-encoded).
            move |origin: String, cache_name: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(meta, _)| meta)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, meta, _)| meta.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_match_any",
            move |origin: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(_, body)| body)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, _, body)) = cache.get(&url) {
                            return Some(body.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_match_any_info",
            move |origin: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(meta, _)| meta)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, meta, _)) = cache.get(&url) {
                            return Some(meta.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_delete",
            move |origin: String, cache_name: String, url: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete(&origin, &cache_name, &url)
                } else {
                    let mut guard = cd.lock().unwrap();
                    if let Some(caches) = guard.get_mut(&origin)
                        && let Some(cache) = caches.get_mut(&cache_name)
                    {
                        cache.remove(&url).is_some()
                    } else {
                        false
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_keys",
            move |origin: String, cache_name: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_keys(&origin, &cache_name).into_iter().map(|(u, _)| u).collect()
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .map(|cache| cache.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_keys_full",
            move |origin: String, cache_name: String| -> String {
                if let Some(ref be) = cbe {
                    let pairs = be.cache_keys(&origin, &cache_name);
                    let items: Vec<String> = pairs
                        .iter()
                        .map(|(url, method)| format!(r#"{{"url":"{url}","method":"{method}"}}"#))
                        .collect();
                    format!("[{}]", items.join(","))
                } else {
                    let guard = cd.lock().unwrap();
                    match guard.get(&origin).and_then(|c| c.get(&cache_name)) {
                        None => "[]".to_string(),
                        Some(cache) => {
                            let items: Vec<String> = cache
                                .iter()
                                .map(|(url, (method, _, _))| {
                                    format!(r#"{{"url":"{url}","method":"{method}"}}"#)
                                })
                                .collect();
                            format!("[{}]", items.join(","))
                        }
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_has",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_has(&origin, &cache_name)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.contains_key(&cache_name))
                        .unwrap_or(false)
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_delete_cache",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete_cache(&origin, &cache_name)
                } else if let Some(caches) = cd.lock().unwrap().get_mut(&origin) {
                    caches.remove(&cache_name).is_some()
                } else {
                    false
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(scope, ctx, store, 
            "_lumen_cache_names",
            move |origin: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_names(&origin)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );
    }
    Ok(())
}

/// The Fetch API — request dispatch, response bodies, streaming and aborts.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_fetch(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
) -> JsResult<()> {
    // ── Fetch API ─────────────────────────────────────────────────────────────
    {
        struct FetchCache {
            status: u16,
            status_text: String,
            headers: Vec<String>, // flat: [name, value, name, value, ...]
            body: Vec<u8>,
        }

        let cache: Arc<Mutex<Option<FetchCache>>> = Arc::new(Mutex::new(None));

        let fp2 = fetch_provider.clone();
        let fp_beacon = fetch_provider.clone();
        let fp_cancel = fetch_provider.clone();
        let fp_cancel_body = fetch_provider.clone();
        let c_cancel = Arc::clone(&cache);
        let c_cancel_body = Arc::clone(&cache);
        let fp_async = fetch_provider.clone();
        let c_async = Arc::clone(&cache);
        let (fp, c) = (fetch_provider, Arc::clone(&cache));
        reg!(scope, ctx, store, "_lumen_fetch_sync", move |url: String, method: String, headers: Vec<String>| -> bool {
            let Some(ref provider) = fp else { return false };
            let headers = pairs_from_flat(headers);
            match provider.fetch_request(&lumen_core::ext::JsFetchRequest {
                url: &url,
                method: &method,
                headers: &headers,
                body: None,
                token: None,
            }) {
                Ok(resp) => {
                    let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                    for (k, v) in resp.headers {
                        flat.push(k);
                        flat.push(v);
                    }
                    *c.lock().unwrap() = Some(FetchCache {
                        status: resp.status,
                        status_text: resp.status_text,
                        headers: flat,
                        body: resp.body,
                    });
                    true
                }
                Err(e) => {
                    eprintln!("fetch error: {e}");
                    false
                }
            }
        });

        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, "_lumen_fetch_get_status", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| u32::from(r.status))
        });

        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, "_lumen_fetch_get_status_text", move || -> String {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(String::new, |r| r.status_text.clone())
        });

        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, "_lumen_fetch_get_headers", move || -> Vec<String> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.headers.clone())
        });

        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, "_lumen_fetch_get_body", move || -> Vec<u8> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.body.clone())
        });

        // _lumen_fetch_body_length() → u32
        // Returns the byte length of the most recent cached response body.
        // Used by the pull()-based ReadableStream in Response.body to avoid
        // copying the full body into JS memory at construction time.
        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, "_lumen_fetch_body_length", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| r.body.len() as u32)
        });

        // _lumen_fetch_body_chunk(offset: u32, size: u32) → Vec<u8>
        // Returns bytes [offset .. offset+size] of the cached response body.
        // Called repeatedly by Response.body.pull() to stream large responses
        // without loading the entire body into JS at once (Fetch Standard §2.2).
        let c = Arc::clone(&cache);
        reg!(scope, ctx, store, 
            "_lumen_fetch_body_chunk",
            move |offset: u32, size: u32| -> Vec<u8> {
                let guard = c.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                let start = (offset as usize).min(body.len());
                let end = (start + size as usize).min(body.len());
                body[start..end].to_vec()
            }
        );

        // _lumen_check_sri_integrity(integrity) → bool
        // Verifies the cached response body against the SRI `integrity` string
        // (W3C SRI §3.3.5). Must be called after _lumen_fetch_sync / _lumen_fetch_sync_with_body
        // and before reading the body. Returns true if integrity is empty or passes.
        {
            let c_sri = Arc::clone(&cache);
            reg!(scope, ctx, store, "_lumen_check_sri_integrity", move |integrity: String| -> bool {
                let guard = c_sri.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                crate::sri::check_sri(body, &integrity)
            });
        }

        // _lumen_fetch_sync_with_body(url, method, content_type, body_bytes, headers) → bool
        // Used by fetch() when init.body is present (FormData, string, ArrayBuffer).
        // Shares the same FetchCache slot as _lumen_fetch_sync.
        {
            let fetch_provider2 = fp2;
            let c2 = Arc::clone(&cache);
            reg!(scope, ctx, store, 
                "_lumen_fetch_sync_with_body",
                move |url: String, method: String, content_type: String, body: Vec<u8>, headers: Vec<String>| -> bool {
                    let Some(ref provider) = fetch_provider2 else {
                        return false;
                    };
                    let headers = pairs_from_flat(headers);
                    match provider.fetch_request(&lumen_core::ext::JsFetchRequest {
                        url: &url,
                        method: &method,
                        headers: &headers,
                        body: Some(lumen_core::ext::JsFetchBody {
                            content_type: &content_type,
                            bytes: &body,
                        }),
                        token: None,
                    }) {
                        Ok(resp) => {
                            let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                            for (k, v) in resp.headers {
                                flat.push(k);
                                flat.push(v);
                            }
                            *c2.lock().unwrap() = Some(FetchCache {
                                status: resp.status,
                                status_text: resp.status_text,
                                headers: flat,
                                body: resp.body,
                            });
                            true
                        }
                        Err(e) => {
                            eprintln!("fetch_with_body error: {e}");
                            false
                        }
                    }
                }
            );
        }

        // _lumen_fetch_cancellable(url, method, timeout_ms, headers) → u32
        // In-flight-cancellable GET/HEAD. Returns 0 = ok (body in FetchCache),
        // 1 = network error, 2 = aborted/timed-out. When timeout_ms > 0 a detached
        // deadline thread flips the AbortToken; the network layer tears the socket
        // down, so a `fetch(url, {signal: AbortSignal.timeout(ms)})` against a slow
        // server actually aborts even though the JS thread is parked in the call.
        reg!(scope, ctx, store, "_lumen_fetch_cancellable", move |url: String, method: String, timeout_ms: u32, headers: Vec<String>| -> u32 {
            let Some(ref provider) = fp_cancel else { return 1 };
            let token = AbortToken::new();
            if timeout_ms > 0 {
                let t = token.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
                    t.abort();
                });
            }
            let headers = pairs_from_flat(headers);
            match provider.fetch_request(&lumen_core::ext::JsFetchRequest {
                url: &url,
                method: &method,
                headers: &headers,
                body: None,
                token: Some(&token),
            }) {
                Ok(resp) => {
                    let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                    for (k, v) in resp.headers { flat.push(k); flat.push(v); }
                    *c_cancel.lock().unwrap() = Some(FetchCache {
                        status: resp.status,
                        status_text: resp.status_text,
                        headers: flat,
                        body: resp.body,
                    });
                    0
                }
                Err(lumen_core::error::Error::Aborted(_)) => 2,
                Err(e) => { eprintln!("fetch error: {e}"); 1 }
            }
        });

        // _lumen_fetch_cancellable_with_body(url, method, content_type, body, timeout_ms, headers) → u32
        // Body-carrying (POST/PUT/...) sibling of _lumen_fetch_cancellable.
        reg!(scope, ctx, store, 
            "_lumen_fetch_cancellable_with_body",
            move |url: String, method: String, content_type: String, body: Vec<u8>, timeout_ms: u32, headers: Vec<String>| -> u32 {
                let Some(ref provider) = fp_cancel_body else { return 1 };
                let token = AbortToken::new();
                if timeout_ms > 0 {
                    let t = token.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
                        t.abort();
                    });
                }
                let headers = pairs_from_flat(headers);
                match provider.fetch_request(&lumen_core::ext::JsFetchRequest {
                    url: &url,
                    method: &method,
                    headers: &headers,
                    body: Some(lumen_core::ext::JsFetchBody {
                        content_type: &content_type,
                        bytes: &body,
                    }),
                    token: Some(&token),
                }) {
                    Ok(resp) => {
                        let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                        for (k, v) in resp.headers { flat.push(k); flat.push(v); }
                        *c_cancel_body.lock().unwrap() = Some(FetchCache {
                            status: resp.status,
                            status_text: resp.status_text,
                            headers: flat,
                            body: resp.body,
                        });
                        0
                    }
                    Err(lumen_core::error::Error::Aborted(_)) => 2,
                    Err(e) => { eprintln!("fetch_with_body error: {e}"); 1 }
                }
            }
        );

        // ── Async fetch (in-flight AbortController.abort) ────────────────────────
        // Runs the request on a background thread so a JS `abort()` fired *during*
        // the request (not just a pre-flight/timeout) flips the AbortToken and the
        // network layer tears the socket down. JS fetch() drives a setTimeout poll
        // loop that resolves/rejects once the worker finishes. No shell change: the
        // existing timer pump drives the poll. Mirrors the WS/SSE poll model.
        {
            /// Background fetch result: success payload, or a typed failure.
            enum AsyncOutcome {
                /// Completed response (headers flattened: [name, value, ...]).
                Ok {
                    status: u16,
                    status_text: String,
                    headers: Vec<String>,
                    body: Vec<u8>,
                },
                /// Network/transport error.
                NetError,
                /// Aborted in flight via the AbortToken.
                Aborted,
            }
            /// Per-handle state shared between the worker thread and the JS poll.
            struct AsyncFetchState {
                token: AbortToken,
                outcome: Option<AsyncOutcome>,
            }
            let async_map: Arc<Mutex<HashMap<u32, AsyncFetchState>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let async_next: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));

            // _lumen_fetch_async_start(url, method, content_type, body, has_body, headers) → handle u32 (0 = no provider)
            let am_start = Arc::clone(&async_map);
            reg!(scope, ctx, store, 
                "_lumen_fetch_async_start",
                move |url: String, method: String, content_type: String, body: Vec<u8>, has_body: bool, headers: Vec<String>| -> u32 {
                    let provider = match fp_async.as_ref() {
                        Some(p) => Arc::clone(p),
                        None => return 0,
                    };
                    let id = async_next.fetch_add(1, Ordering::Relaxed);
                    let token = AbortToken::new();
                    am_start
                        .lock()
                        .unwrap()
                        .insert(id, AsyncFetchState { token: token.clone(), outcome: None });
                    let map = Arc::clone(&am_start);
                    let headers = pairs_from_flat(headers);
                    std::thread::spawn(move || {
                        let res = provider.fetch_request(&lumen_core::ext::JsFetchRequest {
                            url: &url,
                            method: &method,
                            headers: &headers,
                            body: has_body.then(|| lumen_core::ext::JsFetchBody {
                                content_type: &content_type,
                                bytes: &body,
                            }),
                            token: Some(&token),
                        });
                        let outcome = match res {
                            Ok(r) => AsyncOutcome::Ok {
                                status: r.status,
                                status_text: r.status_text,
                                headers: r
                                    .headers
                                    .into_iter()
                                    .flat_map(|(k, v)| [k, v])
                                    .collect(),
                                body: r.body,
                            },
                            Err(lumen_core::error::Error::Aborted(_)) => AsyncOutcome::Aborted,
                            Err(_) => AsyncOutcome::NetError,
                        };
                        if let Some(s) = map.lock().unwrap().get_mut(&id) {
                            s.outcome = Some(outcome);
                        }
                    });
                    id
                }
            );

            // _lumen_fetch_async_poll(handle) → 0 pending, 1 ok, 2 net-error, 3 aborted
            let am_poll = Arc::clone(&async_map);
            reg!(scope, ctx, store, "_lumen_fetch_async_poll", move |id: u32| -> u32 {
                let map = am_poll.lock().unwrap();
                match map.get(&id) {
                    None => 2,
                    Some(s) => match s.outcome {
                        None => 0,
                        Some(AsyncOutcome::Ok { .. }) => 1,
                        Some(AsyncOutcome::NetError) => 2,
                        Some(AsyncOutcome::Aborted) => 3,
                    },
                }
            });

            // _lumen_fetch_async_abort(handle) → flips the token (worker tears the socket down)
            let am_abort = Arc::clone(&async_map);
            reg!(scope, ctx, store, "_lumen_fetch_async_abort", move |id: u32| {
                if let Some(s) = am_abort.lock().unwrap().get(&id) {
                    s.token.abort();
                }
            });

            // _lumen_fetch_async_commit(handle) → moves a completed Ok result into the
            // global FetchCache slot so Response._fromFetchCache reads it. Returns false
            // if the handle is unknown or not in the Ok state.
            let am_commit = Arc::clone(&async_map);
            reg!(scope, ctx, store, "_lumen_fetch_async_commit", move |id: u32| -> bool {
                let mut map = am_commit.lock().unwrap();
                match map.get_mut(&id) {
                    None => false,
                    Some(s) => match s.outcome.take() {
                        Some(AsyncOutcome::Ok { status, status_text, headers, body }) => {
                            *c_async.lock().unwrap() = Some(FetchCache {
                                status,
                                status_text,
                                headers,
                                body,
                            });
                            true
                        }
                        other => {
                            s.outcome = other;
                            false
                        }
                    },
                }
            });

            // _lumen_fetch_async_free(handle) → drop the per-handle state
            let am_free = Arc::clone(&async_map);
            reg!(scope, ctx, store, "_lumen_fetch_async_free", move |id: u32| {
                am_free.lock().unwrap().remove(&id);
            });
        }

        // ── Per-response stream slots ────────────────────────────────────────────
        // Each call to Response._fromFetchCache() allocates a dedicated slot so the
        // body can be consumed independently of subsequent fetch() calls that would
        // otherwise overwrite the single FetchCache slot.
        //
        // _lumen_stream_alloc()                  → u32  (0 = empty body)
        // _lumen_stream_length(id: u32)          → u32
        // _lumen_stream_chunk(id, offset, size)  → Vec<u8>
        // _lumen_stream_free(id: u32)
        {
            let stream_slots: Arc<Mutex<HashMap<u32, Vec<u8>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let stream_next: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));

            let (ss_alloc, sn, c_sa) = (
                Arc::clone(&stream_slots),
                Arc::clone(&stream_next),
                Arc::clone(&cache),
            );
            reg!(scope, ctx, store, "_lumen_stream_alloc", move || -> u32 {
                let body = {
                    let guard = c_sa.lock().unwrap();
                    guard.as_ref().map_or_else(Vec::new, |r| r.body.clone())
                };
                if body.is_empty() {
                    return 0;
                }
                let id = sn.fetch_add(1, Ordering::Relaxed);
                ss_alloc.lock().unwrap().insert(id, body);
                id
            });

            let ss_len = Arc::clone(&stream_slots);
            reg!(scope, ctx, store, "_lumen_stream_length", move |id: u32| -> u32 {
                ss_len.lock().unwrap().get(&id).map_or(0, |b| b.len() as u32)
            });

            let ss_chunk = Arc::clone(&stream_slots);
            reg!(scope, ctx, store, 
                "_lumen_stream_chunk",
                move |id: u32, offset: u32, size: u32| -> Vec<u8> {
                    let guard = ss_chunk.lock().unwrap();
                    let body = guard.get(&id).map_or(&[] as &[u8], |b| b.as_slice());
                    let start = (offset as usize).min(body.len());
                    let end = (start + size as usize).min(body.len());
                    body[start..end].to_vec()
                }
            );

            let ss_free = Arc::clone(&stream_slots);
            reg!(scope, ctx, store, "_lumen_stream_free", move |id: u32| {
                ss_free.lock().unwrap().remove(&id);
            });
        }

        // _lumen_send_beacon(url, body, content_type) → bool
        // Beacon API (W3C Beacon §3): fire-and-forget POST; response is ignored.
        // Returns false if no network provider is available, true if the request was queued.
        // The actual POST runs on a detached background thread so the JS caller is not blocked.
        {
            let fp = fp_beacon;
            reg!(scope, ctx, store, 
                "_lumen_send_beacon",
                move |url: String, body: String, content_type: String| -> bool {
                    let Some(ref provider) = fp else { return false };
                    let ct = if content_type.is_empty() {
                        "text/plain;charset=UTF-8".to_string()
                    } else {
                        content_type
                    };
                    let p = Arc::clone(provider);
                    std::thread::spawn(move || {
                        let _ = p.fetch_with_body_sync(&url, "POST", &ct, body.as_bytes());
                    });
                    true
                }
            );
        }
    }
    Ok(())
}

/// Clipboard API read/write.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_clipboard(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── Clipboard API ─────────────────────────────────────────────────────────
    // _lumen_clipboard_read()      → String (system clipboard plain text, "" if none)
    // _lumen_clipboard_write(text) → void   (replace system clipboard text)
    //
    // Both forward to the process-global clipboard provider installed by the shell
    // (`lumen_js::set_clipboard_provider`). With no provider (tests, dump modes)
    // read returns "" and write is a no-op, so navigator.clipboard still resolves.
    reg!(scope, ctx, store, "_lumen_clipboard_read", || -> String {
        crate::clipboard::read_text()
    });
    reg!(scope, ctx, store, "_lumen_clipboard_write", |text: String| {
        crate::clipboard::write_text(&text);
    });
    Ok(())
}

/// WebAuthn / `navigator.credentials` stubs.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_webauthn(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── WebAuthn / navigator.credentials ──────────────────────────────────────
    // _lumen_webauthn_create(packed) → JSON   (attestation result or {ok:false})
    // _lumen_webauthn_get(packed)    → JSON   (assertion result or {ok:false})
    // _lumen_webauthn_uvpa()         → bool   (platform authenticator available)
    //
    // `packed` is a `|`-separated string of base64url fields (see crate::credentials).
    // All forward to the process-global CredentialProvider installed by the shell
    // (`lumen_js::set_credential_provider`). With no provider, create/get return
    // {ok:false,error:"NotAllowedError"} so navigator.credentials still resolves.
    reg!(scope, ctx, store, "_lumen_webauthn_create", |packed: String| -> String {
        crate::credentials::create(packed)
    });
    reg!(scope, ctx, store, "_lumen_webauthn_get", |packed: String| -> String {
        crate::credentials::get(packed)
    });
    reg!(scope, ctx, store, "_lumen_webauthn_uvpa", || -> bool {
        crate::credentials::uvpa_available()
    });
    Ok(())
}

/// The WebSocket API over the shell's `JsWebSocketProvider`.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_websocket(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
) -> JsResult<()> {
    // ── WebSocket API ─────────────────────────────────────────────────────────
    // Phase 0 model: synchronous connect, background recv thread, JS polls.
    // _lumen_ws_connect(url)  → handle u32 (0 = error)
    // _lumen_ws_send(h, text) → bool
    // _lumen_ws_send_bin(h, data) → bool
    // _lumen_ws_close(h, code, reason)
    // _lumen_ws_poll(h) → Option<String> (JSON event or null)
    {
        use std::collections::HashMap;

        // Registry: handle → Box<dyn JsWebSocketSession>
        // Wrapped in Arc<Mutex<>> so each closure captures its own Arc clone.
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsWebSocketSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, wp) = (Arc::clone(&registry), Arc::clone(&next_id), ws_provider);
        reg!(scope, ctx, store, "_lumen_ws_connect", move |url: String, proto_csv: String| -> u32 {
            let Some(ref provider) = wp else { return 0 };
            let protos: Vec<String> = proto_csv
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            match provider.connect(&url, &protos) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS WebSocket] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, "_lumen_ws_send", move |handle: u32, text: String| -> bool {
            let mut map = reg_c.lock().unwrap();
            if let Some(sess) = map.get_mut(&handle) {
                sess.send_text(&text).is_ok()
            } else {
                false
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, 
            "_lumen_ws_send_bin",
            move |handle: u32, data: Vec<u8>| -> bool {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    sess.send_binary(&data).is_ok()
                } else {
                    false
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, 
            "_lumen_ws_close",
            move |handle: u32, code: u32, reason: String| {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    let _ = sess.close(code as u16, &reason);
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, 
            "_lumen_ws_poll",
            move |handle: u32| -> Option<String> {
                let map = reg_c.lock().unwrap();
                let sess = map.get(&handle)?;
                sess.poll().map(|ev| match ev {
                    JsWsEvent::Open => {
                        let proto = sess.protocol().replace('\\', "\\\\").replace('"', "\\\"");
                        format!(r#"{{"t":"open","protocol":"{proto}"}}"#)
                    }
                    JsWsEvent::Message { data, is_binary } => {
                        if is_binary {
                            // Encode binary payload as base64-like hex for Phase 0.
                            let hex: String =
                                data.iter().map(|b| format!("{b:02x}")).collect();
                            format!(r#"{{"t":"msg","bin":true,"data":"{hex}"}}"#)
                        } else {
                            let text = String::from_utf8_lossy(&data);
                            // Minimal JSON-escape: replace \ and " only.
                            let escaped = text
                                .replace('\\', "\\\\")
                                .replace('"', "\\\"")
                                .replace('\n', "\\n")
                                .replace('\r', "\\r");
                            format!(r#"{{"t":"msg","bin":false,"data":"{escaped}"}}"#)
                        }
                    }
                    JsWsEvent::Close { code, reason } => {
                        let c = code.unwrap_or(1000);
                        let r = reason
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"close","code":{c},"reason":"{r}"}}"#)
                    }
                    JsWsEvent::Error(msg) => {
                        let m = msg
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"error","msg":"{m}"}}"#)
                    }
                })
            }
        );
    }
    Ok(())
}

/// `TextDecoder` (WHATWG Encoding Standard §8-9).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_text_decoder(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── TextDecoder (WHATWG Encoding Standard §8–9) ───────────────────────────
    // BUG-357: label canonicalization/RangeError, real multi-encoding decode,
    // fatal-mode error detection and BOM handling are bridged to
    // `lumen_encoding` — the same decoder the shell already runs for
    // `<meta charset>` documents — rather than a new dependency (tech-stack.md
    // explicitly rejects `encoding_rs` in favor of this crate's own tables).
    // Its decode is a stateless whole-buffer function (no incremental decoder
    // object), so streaming (`{stream:true}`) chunk-boundary handling — and
    // the one-BOM-check-per-stream rule for `ignoreBOM` — lives JS-side in the
    // WEB_API_SHIM `TextDecoder` wrapper; these natives are plain functions.
    // _lumen_text_encoding_for_label(label) → canonical name (lowercase) or
    //   undefined if the label is unknown (including the encodings this
    //   browser doesn't implement, e.g. Shift_JIS/GBK/windows-1252 — see the
    //   dependency-policy note above; this is a deliberate scope decision,
    //   not a bug).
    // _lumen_text_decode(canonical, bytes, ignoreBOM, fatal) → decoded string,
    //   or undefined if `fatal` and decoding produced a replacement character
    //   (lumen_encoding never panics or hard-fails — every decode error, from
    //   any of the supported encodings, surfaces as U+FFFD, so its presence
    //   is an exact fatal-mode signal).
    {
        reg!(scope, ctx, store, 
            "_lumen_text_encoding_for_label",
            move |label: String| -> Option<String> {
                lumen_encoding::Encoding::from_label(&label).map(|enc| enc.name().to_string())
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_text_decode",
            move |canonical: String, bytes: Vec<u8>, ignore_bom: bool, fatal: bool| -> Option<String> {
                let encoding = lumen_encoding::Encoding::from_label(&canonical)
                    .unwrap_or(lumen_encoding::Encoding::Utf8);
                let out = lumen_encoding::decode_to_string_opts(encoding, &bytes, ignore_bom);
                if fatal && out.contains('\u{FFFD}') {
                    None
                } else {
                    Some(out)
                }
            }
        );
    }
    Ok(())
}

/// Server-Sent Events (HTML LS §9.2).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_sse(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
) -> JsResult<()> {
    // ── Server-Sent Events API (HTML Living Standard §9.2) ───────────────────
    // Phase 0 model: background recv thread buffers events, JS polls.
    // _lumen_sse_connect(url) → handle u32 (0 = error / no provider)
    // _lumen_sse_poll(handle) → Option<String> (JSON event or null)
    // _lumen_sse_close(handle)
    {
        use std::collections::HashMap;

        /// JSON-escape a string into a quoted JSON string literal (`"..."`).
        ///
        /// Handles the characters that must be escaped per RFC 8259 §7:
        /// `"`, `\`, and the C0 control set (`\n`/`\r`/`\t`/`\b`/`\f` plus `\u00XX`).
        fn json_str(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{08}' => out.push_str("\\b"),
                    '\u{0c}' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }

        // Registry: handle → Box<dyn JsSseSession>
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsSseSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, sp) = (Arc::clone(&registry), Arc::clone(&next_id), sse_provider);
        reg!(scope, ctx, store, "_lumen_sse_connect", move |url: String| -> u32 {
            let Some(ref provider) = sp else { return 0 };
            match provider.connect_sse(&url) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS SSE] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, "_lumen_sse_poll", move |handle: u32| -> Option<String> {
            let map = reg_c.lock().unwrap();
            let sess = map.get(&handle)?;
            sess.poll().map(|ev| match ev {
                JsSseEvent::Open => r#"{"t":"open"}"#.to_string(),
                JsSseEvent::Message {
                    event_type,
                    data,
                    id,
                } => {
                    let id_json = id
                        .as_deref()
                        .map_or_else(|| "null".to_string(), json_str);
                    format!(
                        r#"{{"t":"message","event":{},"data":{},"id":{}}}"#,
                        json_str(&event_type),
                        json_str(&data),
                        id_json
                    )
                }
                JsSseEvent::Retry(ms) => {
                    format!(r#"{{"t":"retry","ms":{ms}}}"#)
                }
                JsSseEvent::Reconnecting => r#"{"t":"reconnecting"}"#.to_string(),
                JsSseEvent::Close => r#"{"t":"close"}"#.to_string(),
                JsSseEvent::Error(e) => {
                    format!(r#"{{"t":"error","message":{}}}"#, json_str(&e))
                }
            })
        });

        let reg_c = Arc::clone(&registry);
        reg!(scope, ctx, store, "_lumen_sse_close", move |handle: u32| {
            if let Some(mut sess) = reg_c.lock().unwrap().remove(&handle) {
                sess.close();
            }
        });
    }
    Ok(())
}
