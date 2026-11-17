//! Service Worker fetch interception — SQLite-backed реализация
//! `lumen_core::ext::FetchInterceptor`.
//!
//! При каждом fetch: (1) проверяем, есть ли SW с scope, покрывающим URL;
//! (2) если есть — пробуем диспетчеризацию в реальный SW-поток (PH3-20);
//! (3) если SW-поток не ответил или не зарегистрирован — ищем ответ в
//! CacheStorage любого именованного кэша для этого origin.
//! Возвращаем первое найденное тело. Если SW нет или кэш пуст — возвращаем
//! None, запрос уходит в сеть штатно.
//!
//! Имя кэша не указывается явно: interceptor проверяет все кэши origin-а
//! (как `caches.match()` без имени в JS). Для GET-запросов (ответ из кэша
//! всегда GET).

use std::sync::Arc;

use lumen_core::ext::{FetchInterceptor, SwWorkerStore};
use lumen_core::url::Url;

use crate::cache_storage::CacheStorage;
use crate::service_workers::ServiceWorkers;

/// SQLite-backed SW fetch interceptor.
///
/// Проверяет SW-регистрации и CacheStorage при каждом fetch из lumen-network.
/// Конструируется в shell и передаётся в `HttpClient::with_interceptor()`.
pub struct ServiceWorkerInterceptor {
    /// Active Service Worker registrations from SQLite.
    pub sw_store: Arc<ServiceWorkers>,
    /// CacheStorage backend from SQLite — used as fallback when SW thread
    /// does not respond.
    pub cache_store: Arc<CacheStorage>,
    /// Live SW execution threads keyed by `(origin, scope)`.
    /// When `Some`, fetch requests are dispatched to the SW thread first
    /// before falling back to the cache-only lookup.
    pub sw_worker_store: Option<SwWorkerStore>,
}

impl ServiceWorkerInterceptor {
    /// Create an interceptor with cache-only SW interception (Phase 0 behaviour).
    pub fn new(sw_store: Arc<ServiceWorkers>, cache_store: Arc<CacheStorage>) -> Self {
        Self {
            sw_store,
            cache_store,
            sw_worker_store: None,
        }
    }

    /// Attach a `SwWorkerStore` so that incoming fetch requests are dispatched
    /// to real SW execution threads when they are available (PH3-20).
    ///
    /// The same `Arc` must be passed to `QuickJsRuntime::with_sw_worker_store`
    /// so that both sides share the same worker registry.
    pub fn with_sw_workers(mut self, store: SwWorkerStore) -> Self {
        self.sw_worker_store = Some(store);
        self
    }
}

impl FetchInterceptor for ServiceWorkerInterceptor {
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    fn intercept(&self, url: &Url, origin: &str) -> Option<Vec<u8>> {
        // 1. Есть ли SW, scope которого покрывает путь этого URL?
        // find_for_url ожидает path (как /app/page), не полный URL.
        //
        // 1. PH3-20: маршрутизация в реальный SW-поток через `sw_worker_store`.
        // Это работает независимо от SQLite `ServiceWorkers`: shell хранит
        // регистрации в in-memory map + SwBackend JSON-снапшоте, а активный
        // SW-поток регистрируется напрямую в `sw_worker_store` ключом
        // `(origin, scope)`. Выбираем worker с самым длинным scope-prefix-ом,
        // покрывающим путь URL (SW service-worker selection — longest match).
        if let Some(store) = &self.sw_worker_store {
            let path = url.path();
            let (tx_opt, routes_json) = {
                let workers = store.lock().unwrap();
                workers
                    .iter()
                    .filter(|((o, scope), _)| o == origin && path.starts_with(scope.as_str()))
                    .max_by_key(|((_, scope), _)| scope.len())
                    .map(|(_, h)| (h.tx.clone(), h.routes_json.clone()))
                    .map(|(tx, routes)| (Some(tx), Some(routes)))
                    .unwrap_or((None, None))
            };
            // SW-PRELOAD-ROUTER: static-router rules (`InstallEvent.addRoutes`)
            // apply before the dispatch — the first matching rule wins. The
            // engine only sees the URL here, so `requestMethod`/`requestMode`/
            // `requestDestination`/`runningStatus` conditions are ignored
            // (everything reaching this path is a GET; the rest would need
            // request context in `FetchInterceptor`, BUG-1744 territory).
            // shortcut: conditions other than urlPattern/or/not are not
            // evaluated; upgrade when the interceptor carries the request.
            if let Some(json) = routes_json.as_ref().and_then(|r| r.lock().unwrap().clone())
                && let Some(source) = Self::match_routes(&json, url)
            {
                match source.as_str() {
                    // `network`: the request goes to the network untouched —
                    // no dispatch, no cache.
                    "network" => return None,
                    // `cache`: answer from CacheStorage only — check the
                    // origin's named caches directly (the SQLite-fallback
                    // section below requires a registration, which the shell
                    // path does not have).
                    "cache" => {
                        let cache_names = self.cache_store.list_cache_names(origin).ok()?;
                        for name in &cache_names {
                            if let Ok(Some(entry)) =
                                self.cache_store.match_(origin, name, url.as_str(), "GET")
                            {
                                return Some(entry.response_body);
                            }
                        }
                        return None;
                    }
                    // `fetch-event` and `race-network-and-fetch-handler` both
                    // dispatch to the SW (a real race needs the parallel
                    // network leg — shortcut).
                    _ => {}
                }
            }
            if let Some(tx) = tx_opt
                && let Some(body) = Self::dispatch_to_worker(&tx, url)
            {
                return Some(body);
            }
            // No worker matched, timeout, or respondWith(undefined) — fall through.
        }

        // 2. SQLite-backed fallback (Phase 0 behaviour / persisted deployments).
        // Если SQLite ServiceWorkers пуст (как в текущем shell) — None, выходим.
        let _reg = self.sw_store.find_for_url(origin, url.path()).ok()??;

        // 3. Проверяем все именованные кэши origin-а (как caches.match()).
        // CacheStorage хранит полный URL в request_url.
        let cache_names = self.cache_store.list_cache_names(origin).ok()?;
        for name in &cache_names {
            if let Ok(Some(entry)) =
                self.cache_store.match_(origin, name, url.as_str(), "GET")
            {
                return Some(entry.response_body);
            }
        }
        None
    }
}

impl ServiceWorkerInterceptor {
    /// SW-PRELOAD-ROUTER: evaluate the worker's static-router rules (the
    /// JSON of `InstallEvent.addRoutes`) against the request URL. The first
    /// rule whose condition matches wins; returns its `source`. A rule's
    /// condition is either flat or `or: [conditions]`; each condition matches
    /// when `urlPattern` is a prefix of the URL path or of the full URL
    /// (string URLPattern shorthand — full URLPattern syntax is out of scope),
    /// `not` inverts, unmatched/unspecified conditions are ignored.
    fn match_routes(routes_json: &str, url: &Url) -> Option<String> {
        let rules: serde_json::Value = serde_json::from_str(routes_json).ok()?;
        let rules = rules.as_array()?;
        for rule in rules {
            let source = rule.get("source")?.as_str()?;
            let matches = match rule.get("or").and_then(|v| v.as_array()) {
                Some(conds) => conds.iter().any(|c| Self::cond_matches(c, url).unwrap_or(false)),
                None => Self::cond_matches(rule, url)?,
            };
            let negated = rule.get("not").and_then(|v| v.as_bool()).unwrap_or(false)
                || rule.get("condition").and_then(|c| c.get("not")).and_then(|v| v.as_bool()).unwrap_or(false);
            if matches != negated {
                return Some(source.to_string());
            }
        }
        None
    }

    /// One routing condition: `urlPattern` (prefix shorthand), `not` inverted.
    fn cond_matches(cond: &serde_json::Value, url: &Url) -> Option<bool> {
        let pattern = cond.get("urlPattern")?.as_str()?;
        if pattern.is_empty() {
            return Some(true);
        }
        let matched = url.path().starts_with(pattern) || url.as_str().starts_with(pattern);
        let negated = cond.get("not").and_then(|v| v.as_bool()).unwrap_or(false);
        Some(matched != negated)
    }

    /// Dispatch a GET fetch request to a SW execution thread and wait for its
    /// `respondWith()` body. Returns `None` on timeout, channel error, or when
    /// the SW did not call `respondWith()` with a body.
    fn dispatch_to_worker(
        tx: &std::sync::mpsc::Sender<lumen_core::ext::SwWorkerMessage>,
        url: &Url,
    ) -> Option<Vec<u8>> {
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
        tx.send(lumen_core::ext::SwWorkerMessage::Fetch(lumen_core::ext::SwFetchRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            response_tx,
        }))
        .ok()?;
        // Wait up to 5 s for the SW to respond.
        response_rx
            .recv_timeout(std::time::Duration::from_millis(5_000))
            .ok()
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_storage::CacheStorage;
    use crate::service_workers::{ServiceWorkers, UpdateViaCache};

    fn make() -> ServiceWorkerInterceptor {
        ServiceWorkerInterceptor::new(
            Arc::new(ServiceWorkers::open_in_memory().unwrap()),
            Arc::new(CacheStorage::open_in_memory().unwrap()),
        )
    }

    #[test]
    fn no_sw_registered_returns_none() {
        let si = make();
        let url = Url::parse("https://example.com/page").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn sw_registered_but_cache_empty_returns_none() {
        let si = make();
        si.sw_store
            .register(
                "https://example.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/api/data").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn sw_registered_cache_hit_returns_body() {
        let si = make();
        si.sw_store
            .register(
                "https://example.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://example.com",
                "v1",
                "https://example.com/api/data",
                "GET",
                200,
                "",
                b"cached body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/api/data").unwrap();
        let result = si.intercept(&url, "https://example.com");
        assert_eq!(result, Some(b"cached body".to_vec()));
    }

    #[test]
    fn sw_scope_mismatch_returns_none() {
        let si = make();
        // SW покрывает только /app/, а запрос к /other/
        si.sw_store
            .register(
                "https://example.com",
                "/app/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://example.com",
                "v1",
                "https://example.com/other/page",
                "GET",
                200,
                "",
                b"body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/other/page").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn different_origin_sw_does_not_intercept() {
        let si = make();
        si.sw_store
            .register(
                "https://other.com",
                "/",
                "/sw.js",
                UpdateViaCache::Imports,
                0,
            )
            .unwrap();
        si.cache_store
            .put(
                "https://other.com",
                "v1",
                "https://example.com/page",
                "GET",
                200,
                "",
                b"body",
                0,
            )
            .unwrap();
        let url = Url::parse("https://example.com/page").unwrap();
        // origin=example.com, SW есть только для other.com
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    // ── PH3-20: worker-store routing (shell path — no SQLite registration) ──────

    /// Spawn a fake SW thread that replies to fetch requests for `url` with `body`.
    /// Models a real SW execution thread without depending on `lumen-js`.
    fn fake_sw_thread(
        match_url: &'static str,
        body: &'static [u8],
    ) -> lumen_core::ext::SwWorkerHandle {
        let (tx, rx) = std::sync::mpsc::channel::<lumen_core::ext::SwWorkerMessage>();
        let thread = std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let lumen_core::ext::SwWorkerMessage::Fetch(req) = msg else {
                    continue;
                };
                let resp = if req.url == match_url {
                    Some(body.to_vec())
                } else {
                    None
                };
                let _ = req.response_tx.send(resp);
            }
        });
        lumen_core::ext::SwWorkerHandle {
        tx,
        _thread: thread,
        wid: 0,
        routes_json: std::sync::Arc::new(std::sync::Mutex::new(None)),
    }
    }

    fn make_with_workers(store: SwWorkerStore) -> ServiceWorkerInterceptor {
        ServiceWorkerInterceptor::new(
            Arc::new(ServiceWorkers::open_in_memory().unwrap()),
            Arc::new(CacheStorage::open_in_memory().unwrap()),
        )
        .with_sw_workers(store)
    }

    #[test]
    fn worker_store_routes_without_sqlite_registration() {
        // No SQLite `register()` — only a live worker in the store, as in the shell.
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            fake_sw_thread("https://example.com/app.js", b"sw body"),
        );
        let si = make_with_workers(store);
        let url = Url::parse("https://example.com/app.js").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"sw body".to_vec())
        );
    }

    #[test]
    fn worker_store_longest_scope_prefix_wins() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        // Broad scope returns nothing for this URL; narrow scope owns it.
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            fake_sw_thread("___never___", b"root"),
        );
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/app/".to_string()),
            fake_sw_thread("https://example.com/app/data.json", b"app body"),
        );
        let si = make_with_workers(store);
        let url = Url::parse("https://example.com/app/data.json").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"app body".to_vec())
        );
    }

    #[test]
    fn worker_scope_mismatch_falls_through_to_none() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/app/".to_string()),
            fake_sw_thread("https://example.com/app/x", b"x"),
        );
        let si = make_with_workers(store);
        // /other/ is outside the SW scope → no worker matches, no cache → None.
        let url = Url::parse("https://example.com/other/y").unwrap();
        assert!(si.intercept(&url, "https://example.com").is_none());
    }

    #[test]
    fn worker_respondwith_none_falls_through_to_cache() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            // Worker matches the scope but returns None for this URL.
            fake_sw_thread("___never___", b"unused"),
        );
        let si = make_with_workers(store);
        // SQLite registration + cache entry exist → fall-through serves them.
        si.sw_store
            .register("https://example.com", "/", "/sw.js", UpdateViaCache::Imports, 0)
            .unwrap();
        si.cache_store
            .put("https://example.com", "v1", "https://example.com/cached", "GET", 200, "", b"from cache", 0)
            .unwrap();
        let url = Url::parse("https://example.com/cached").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"from cache".to_vec())
        );
    }

    // ── SW-PRELOAD-ROUTER: static-router rules applied before dispatch ─────────

    /// Point the live worker's `routes_json` slot at a rules JSON.
    fn with_routes(handle: &lumen_core::ext::SwWorkerHandle, rules: &str) {
        *handle.routes_json.lock().unwrap() = Some(rules.to_string());
    }

    #[test]
    fn route_source_network_skips_dispatch_and_cache() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let handle = fake_sw_thread("https://example.com/static/x", b"sw body");
        with_routes(
            &handle,
            r#"[{"source":"network","urlPattern":"/static/"}]"#,
        );
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            handle,
        );
        let si = make_with_workers(store);
        // The worker WOULD answer this URL and a cache entry exists, but the
        // `network` route must send the request straight to the network (None).
        si.cache_store
            .put("https://example.com", "v1", "https://example.com/static/x", "GET", 200, "", b"cached", 0)
            .unwrap();
        let url = Url::parse("https://example.com/static/x").unwrap();
        assert_eq!(si.intercept(&url, "https://example.com"), None);
    }

    #[test]
    fn route_source_cache_serves_cache_without_dispatch() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        // No SQLite registration — the SQLite fallback section would return
        // None; the `cache` route must check the named caches directly.
        let handle = fake_sw_thread("https://example.com/cache/y", b"sw body");
        with_routes(
            &handle,
            r#"[{"source":"cache","cacheName":"v1","urlPattern":"/cache/"}]"#,
        );
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            handle,
        );
        let si = make_with_workers(store);
        si.cache_store
            .put("https://example.com", "v1", "https://example.com/cache/y", "GET", 200, "", b"route cache", 0)
            .unwrap();
        let url = Url::parse("https://example.com/cache/y").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"route cache".to_vec())
        );
    }

    #[test]
    fn route_source_fetch_event_dispatches() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let handle = fake_sw_thread("https://example.com/app/z", b"sw body");
        with_routes(
            &handle,
            r#"[{"source":"fetch-event","urlPattern":"/app/"}]"#,
        );
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            handle,
        );
        let si = make_with_workers(store);
        let url = Url::parse("https://example.com/app/z").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"sw body".to_vec())
        );
    }

    #[test]
    fn route_url_pattern_mismatch_leaves_dispatch_alone() {
        let store: SwWorkerStore = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let handle = fake_sw_thread("https://example.com/other/w", b"sw body");
        with_routes(
            &handle,
            r#"[{"source":"network","urlPattern":"/static/"}]"#,
        );
        store.lock().unwrap().insert(
            ("https://example.com".to_string(), "/".to_string()),
            handle,
        );
        let si = make_with_workers(store);
        // The pattern does not match — no route fires, normal dispatch runs.
        let url = Url::parse("https://example.com/other/w").unwrap();
        assert_eq!(
            si.intercept(&url, "https://example.com"),
            Some(b"sw body".to_vec())
        );
    }
}
