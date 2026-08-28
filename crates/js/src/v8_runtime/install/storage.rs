//! Секции `install_dom`: история, навигация, хранилища, cookie, реестр модулей.
//!
//! Вырезано из `V8JsRuntime::install_dom` батчем SPLIT-JS6 без правки тел:
//! секции жили внутри замыкания `self.run(…)` на отступе 4, то есть ровно на
//! отступе тела функции, а единственной правкой стала приставка контекста у
//! площадок `reg!` — см. [`super::reg`].

use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// `history.pushState`/`replaceState`/`go` bridges to the shell's session history.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_history(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    pending_history_url_updates: Arc<Mutex<Vec<crate::dom::HistoryUrlUpdate>>>,
    pending_history_traversals: Arc<Mutex<Vec<i32>>>,
) -> JsResult<()> {
    // ── history ──────────────────────────────────────────────────────────────
    {
        let hist = Arc::new(Mutex::new(HistoryState::new()));

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, 
            "_lumen_history_push",
            move |state_json: String, url: String| {
                h.lock().unwrap().push(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, 
            "_lumen_history_replace",
            move |state_json: String, url: String| {
                h.lock().unwrap().replace(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, "_lumen_history_go", move |delta: i32| -> bool {
            h.lock().unwrap().go(delta)
        });

        // Queue a real session-history traversal for the shell. `history.go(n)` /
        // `back` / `forward` call this so the shell (single authority) moves its
        // `nav_back`/`nav_fwd` stacks by `delta` and delivers the destination
        // popstate or reload — the JS `HistoryState` above is only a read-cache.
        let t = Arc::clone(&pending_history_traversals);
        reg!(scope, ctx, store, "_lumen_history_traverse", move |delta: i32| {
            t.lock().unwrap().push(delta);
        });

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, "_lumen_history_set_state", move |state_json: String| {
            h.lock().unwrap().set_state(state_json)
        });

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, "_lumen_history_length", move || -> u32 {
            h.lock().unwrap().length()
        });

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, "_lumen_history_state_json", move || -> String {
            h.lock().unwrap().state_json().to_string()
        });

        let h = Arc::clone(&hist);
        reg!(scope, ctx, store, "_lumen_history_url", move || -> String {
            h.lock().unwrap().url().to_string()
        });

        // Notify shell of pushState/replaceState URL changes so the address bar
        // can be updated without a page reload.  Called from history.pushState /
        // history.replaceState in WEB_API_SHIM after the JS HistoryState is updated.
        let q = Arc::clone(&pending_history_url_updates);
        reg!(scope, ctx, store, 
            "_lumen_history_push_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Push { url, new_state_json });
            }
        );

        let q = Arc::clone(&pending_history_url_updates);
        reg!(scope, ctx, store, 
            "_lumen_history_replace_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Replace { url, new_state_json });
            }
        );
    }
    Ok(())
}

/// Navigation API state and interception bridges.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_navigation_api(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    nav_state: Arc<Mutex<String>>,
    pending_navigation_updates: Arc<Mutex<Vec<crate::dom::NavUpdate>>>,
    pending_nav_intercepted: Arc<Mutex<Vec<(bool, bool)>>>,
) -> JsResult<()> {
    // ── Navigation API ──────────────────────────────────────────────────────────
    // Shell-backed Navigation API.  All mutations are queued via
    // `pending_navigation_updates`; the shell drains them in `about_to_wait`
    // and is the single authority for the nav_back / nav_fwd stacks.
    {
        let ns_entries = Arc::clone(&nav_state);
        let ns_index   = Arc::clone(&nav_state);
        let ns_back    = Arc::clone(&nav_state);
        let ns_fwd     = Arc::clone(&nav_state);
        let ns_set     = Arc::clone(&nav_state);
        let q          = Arc::clone(&pending_navigation_updates);
        let pi         = Arc::clone(&pending_nav_intercepted);

        // ── accessors (read nav_state JSON, locked only for copy) ────────────────
        reg!(scope, ctx, store, 
            "_lumen_navigation_entries_json",
            move || -> String {
                ns_entries.lock().map(|s| s.clone()).unwrap_or_default()
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_navigation_current_index",
            move || -> i32 {
                ns_index.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| v.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32)
                    .unwrap_or(0)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_navigation_can_go_back",
            move || -> bool {
                ns_back.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| {
                        let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                        let len = v.get("entries").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
                        idx > 0 && len > 0
                    })
                    .unwrap_or(false)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_navigation_can_go_forward",
            move || -> bool {
                ns_fwd.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| {
                        let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                        let len = v.get("entries").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
                        idx + 1 < (len as u64)
                    })
                    .unwrap_or(false)
            }
        );

        // ── state setter (called from shell via eval_js) ─────────────────────────
        reg!(scope, ctx, store, 
            "_lumen_navigation_set_state",
            move |json: String| {
                *ns_set.lock().unwrap() = json;
            }
        );

        // ── navigation action queue ──────────────────────────────────────────────
        reg!(scope, ctx, store, 
            "_lumen_navigation_report_intercept",
            move |intercepted: bool, cancelled: bool| {
                let mut q = pi.lock().unwrap();
                q.push((intercepted, cancelled));
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_navigation_request",
            move |action_code: u8, url: String, key: String, data: String| {
                let action = match action_code {
                    0 => NavAction::Push,
                    1 => NavAction::Replace,
                    2 => NavAction::Back,
                    3 => NavAction::Forward,
                    4 => NavAction::TraverseTo,
                    5 => NavAction::Reload,
                    6 => NavAction::InterceptedSuccess,
                    7 => NavAction::InterceptedError,
                    _ => return,
                };
                q.lock().unwrap().push((action, url, key, data));
            }
        );
    }
    Ok(())
}

/// `location.href =`, `assign`, `replace`, `reload`.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_navigation(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    nav_out: Arc<Mutex<Option<crate::dom::NavigateRequest>>>,
) -> JsResult<()> {
    // ── navigation (location.href =, assign, replace, reload) ────────────────
    {
        let nav = Arc::clone(&nav_out);
        reg!(scope, ctx, store, "_lumen_navigate", move |url: String, replace: bool| {
            *nav.lock().unwrap() = Some(if replace {
                NavigateRequest::Replace(url)
            } else {
                NavigateRequest::Push(url)
            });
        });

        let nav = Arc::clone(&nav_out);
        reg!(scope, ctx, store, "_lumen_reload", move || {
            *nav.lock().unwrap() = Some(NavigateRequest::Reload);
        });

        // BUG-383: `form.submit()` / `form.requestSubmit()`. The shell owns
        // encoding and the navigation, so the page only hands over the node
        // ids; `submitter` is -1 when the form was submitted with no control.
        let nav = Arc::clone(&nav_out);
        reg!(scope, ctx, store, "_lumen_request_form_submit", move |form: u32, submitter: i32| {
            *nav.lock().unwrap() = Some(NavigateRequest::SubmitForm { form, submitter });
        });
    }
    Ok(())
}

/// `localStorage` backed by the origin's `WebStorage`.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_local_storage(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    ls_store: Arc<Mutex<lumen_core::WebStorage>>,
) -> JsResult<()> {
    // ── localStorage ─────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ls_store);
        reg!(scope, ctx, store, "_lumen_ls_clear", move || {
            s.lock().unwrap().clear();
        });
    }
    Ok(())
}

/// `sessionStorage` backed by the tab's `WebStorage` (BUG-836).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_session_storage(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    ss_store: Arc<Mutex<lumen_core::WebStorage>>,
) -> JsResult<()> {
    // ── sessionStorage ────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ss_store);
        reg!(scope, ctx, store, "_lumen_ss_clear", move || {
            s.lock().unwrap().clear();
        });
    }
    Ok(())
}

/// IndexedDB persistence bridge.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_indexed_db(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
) -> JsResult<()> {
    // ── IndexedDB persistence ─────────────────────────────────────────────────
    // Registered only when a backend is supplied (None in unit tests / sandboxed
    // contexts → the JS shim falls back to in-heap-only databases via its
    // `typeof _lumen_idb_persist === 'function'` guards). The shim serializes the
    // whole per-origin database set into one opaque JSON snapshot; `_lumen_idb_load`
    // restores it on init, `_lumen_idb_persist` writes it after each mutating flush.
    if let Some(idb) = idb_backend {
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_load", move || -> Option<String> { b.load() });
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_persist", move |snapshot: String| {
            b.save(&snapshot);
        });
        // Structured (Phase 3) row-level path. The JS shim keeps the in-heap
        // database authoritative and the opaque snapshot (above) as the lossless
        // restore source; these primitives additionally mirror schema + records
        // into the per-origin SQLite tables so `databases()` and future row-level
        // queries survive a reload. No-op on blob-only backends (default trait impls).
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_schema_op", move |json: String| -> bool {
            match serde_json::from_str::<lumen_core::ext::IdbSchemaOp>(&json) {
                Ok(op) => b.apply_schema(&op).is_ok(),
                Err(_) => false,
            }
        });
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_commit_txn", move |json: String| -> bool {
            match serde_json::from_str::<Vec<lumen_core::ext::IdbRecordOp>>(&json) {
                Ok(ops) => b.commit_txn(&ops).is_ok(),
                Err(_) => false,
            }
        });
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_exec_op", move |json: String| -> Option<String> {
            serde_json::from_str::<lumen_core::ext::IdbRecordOp>(&json)
                .ok()
                .and_then(|op| b.exec_op(&op).ok())
                .and_then(|result| serde_json::to_string(&result).ok())
        });
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_db_version", move |db_name: String| -> i32 {
            b.db_version(&db_name) as i32
        });
        let b = Arc::clone(&idb);
        reg!(scope, ctx, store, "_lumen_idb_databases", move || -> String {
            let dbs = b.list_databases();
            serde_json::to_string(
                &dbs.iter()
                    .map(|(name, version)| serde_json::json!({ "name": name, "version": version }))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_string())
        });
    }
    Ok(())
}

/// `document.cookie` get/set over the `CookieProvider` (RFC 6265 §5.3-5.4).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_cookie(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    page_url: String,
    cookie_jar: Option<Arc<dyn lumen_core::ext::CookieProvider>>,
) -> JsResult<()> {
    // ── document.cookie (RFC 6265 §5.3-5.4) ─────────────────────────────────
    // The getter/setter wrap CookieProvider using host/scheme derived from
    // page_url parsed once at install time. Best-effort: if the URL cannot be
    // parsed (e.g. file://) we skip cookie injection silently.
    {
        let parsed = Url::parse(&page_url).ok();
        let host = parsed.as_ref().map(|u| u.host().to_ascii_lowercase()).unwrap_or_default();
        let is_secure = parsed.as_ref().map(|u| u.scheme() == "https").unwrap_or(false);

        if let Some(jar) = cookie_jar {
            let jar_get = Arc::clone(&jar);
            let host_get = host.clone();
            reg!(scope, ctx, store, "_lumen_cookie_get", move || -> String {
                jar_get.get_for_request(&host_get, "/", is_secure, None, false)
            });

            let host_set = host;
            reg!(scope, ctx, store, "_lumen_cookie_set", move |cookie_str: String| {
                jar.process_set_cookie(&cookie_str, &host_set, "/", is_secure, None);
            });
        } else {
            reg!(scope, ctx, store, "_lumen_cookie_get", move || -> String { String::new() });
            reg!(scope, ctx, store, "_lumen_cookie_set", move |_unused: String| {});
        }
    }
    Ok(())
}

/// ES module registry bridge (BUG-571).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_esm_registry(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── ES module registry bridge (BUG-571) ─────────────────────────────────
    // A `<script type=module>` inserted by page script is prepared entirely in
    // the shim (`_lumen_script_prepare`), which has no way to reach the
    // thread-local module map the loader compiles from. These two natives are
    // that bridge: the shim registers the module body, then calls `import()`,
    // whose host callback finds the source under the specifier it just wrote.
    // Both are plain map writes — no re-entry into V8, which the compat-layer
    // closure signature could not do anyway.
    reg!(scope, ctx, store, "_lumen_esm_register", move |specifier: String, source: String| {
        crate::v8_esm::register_source(&specifier, &source);
    });
    // Inline module bodies have no URL, so the loader mints a virtual
    // `lumen://inline-N` specifier for them; it is returned to the shim to be
    // handed straight back to `import()`.
    reg!(scope, ctx, store, "_lumen_esm_register_inline", move |source: String| -> String {
        crate::v8_esm::register_inline(&source)
    });
    Ok(())
}
