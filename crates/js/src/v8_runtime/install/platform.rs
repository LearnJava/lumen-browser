//! Секции `install_dom`: консоль, часы, геометрия, скролл, окно, криптография.
//!
//! Вырезано из `V8JsRuntime::install_dom` батчем SPLIT-JS6 без правки тел:
//! секции жили внутри замыкания `self.run(…)` на отступе 4, то есть ровно на
//! отступе тела функции, а единственной правкой стала приставка контекста у
//! площадок `reg!` — см. [`super::reg`].

use super::reg;
#[allow(unused_imports)]
use super::super::*;

/// `console.log`/`warn`/`error` sinks feeding the shell's message buffer.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_console(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
) -> JsResult<()> {
    // ── console ──────────────────────────────────────────────────────────────
    {
        let buf_log = Arc::clone(&console_messages);
        reg!(scope, ctx, store, "_lumen_console_log", move |msg: String| {
            eprintln!("[JS] {msg}");
            buf_log.lock().unwrap().push((0, msg));
        });
        let buf_warn = Arc::clone(&console_messages);
        reg!(scope, ctx, store, "_lumen_console_warn", move |msg: String| {
            eprintln!("[JS warn] {msg}");
            buf_warn.lock().unwrap().push((1, msg));
        });
        let buf_err = Arc::clone(&console_messages);
        reg!(scope, ctx, store, "_lumen_console_error", move |msg: String| {
            eprintln!("[JS error] {msg}");
            buf_err.lock().unwrap().push((2, msg));
        });
    }
    Ok(())
}

/// `window.print()` — queues a print-preview request for the shell (W-2).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_print(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    print_requests: Arc<Mutex<Vec<crate::dom::PrintRequest>>>,
) -> JsResult<()> {
    // ── window.print() (W-2) ──────────────────────────────────────────────────
    {
        let pr = Arc::clone(&print_requests);
        reg!(scope, ctx, store, "_lumen_print_dialog", move || {
            eprintln!("[window.print()] Opening print preview dialog");
            pr.lock().unwrap().push(PrintRequest::default());
        });
    }
    Ok(())
}

/// `<dialog>` focus/blur requests (HTML LS §6.6.3).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_dialog_focus(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
) -> JsResult<()> {
    // ── dialog focus management (HTML LS §6.6.3) ─────────────────────────────
    // `showModal()` calls `_lumen_request_focus(nid)` to focus the first autofocus
    // element (or the dialog itself).  `close()` calls `_lumen_request_focus(prev)`
    // to restore focus to the element that was active before the dialog opened.
    // The shell drains these via `take_focus_requests()` after each JS pump.
    {
        let pfr = Arc::clone(&pending_focus_requests);
        reg!(scope, ctx, store, "_lumen_request_focus", move |nid: u32| {
            pfr.lock().unwrap().push(Some(nid));
        });
        let pfr2 = Arc::clone(&pending_focus_requests);
        reg!(scope, ctx, store, "_lumen_request_blur", move || {
            pfr2.lock().unwrap().push(None);
        });
    }
    Ok(())
}

/// `performance.now()` / `Date.now()` clock source, deterministic mode included.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_performance_now(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    deterministic_seed: Option<u64>,
    monotonic_clock: bool,
    deterministic_clock_ms: Arc<AtomicU64>,
) -> JsResult<()> {
    // ── performance.now() — high-resolution timestamp ────────────────────────
    // Returns milliseconds since Unix epoch as f64; JS shim subtracts
    // the time-origin captured at install_dom_api time to give DOMHighResTimeStamp.
    // In deterministic mode (8F) always returns 0 so Date.now()/performance.now()
    // are frozen at the epoch, making rendering output independent of wall-clock
    // time — unless DEVX-16's `--monotonic-clock` is set, in which case each
    // call advances `deterministic_clock_ms` by 1 ms instead of staying at 0.
    let det_time = deterministic_seed.is_some();
    reg!(scope, ctx, store, "_lumen_now_ms", move || -> f64 {
        if det_time {
            if monotonic_clock {
                deterministic_clock_ms.fetch_add(1, Ordering::Relaxed) as f64
            } else {
                0.0
            }
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        }
    });
    Ok(())
}

/// Timer and `requestAnimationFrame` wakeup notifications for the shell.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_timer_wakeup(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    raf_pending: Arc<AtomicBool>,
) -> JsResult<()> {
    // ── timer wakeup notification ─────────────────────────────────────────────
    // Called by _lumen_tick_timers / setTimeout / setInterval JS shims when a
    // timer is scheduled. Stores the earliest pending deadline (Unix epoch ms)
    // so the shell event loop can set ControlFlow::WaitUntil accordingly.
    {
        let tw = Arc::clone(&timer_wakeup);
        reg!(scope, ctx, store, "_lumen_request_wakeup", move |deadline_ms: f64| {
            let mut lock = tw.lock().unwrap();
            match *lock {
                None => *lock = Some(deadline_ms),
                Some(prev) if deadline_ms < prev => *lock = Some(deadline_ms),
                _ => {}
            }
        });
    }

    // Called by requestAnimationFrame when a callback is queued.
    // Shell reads this after each rendering step to decide whether to request
    // the next redraw for JS animation loops.
    {
        let raf = Arc::clone(&raf_pending);
        reg!(scope, ctx, store, "_lumen_mark_raf_pending", move || {
            raf.store(true, Ordering::Relaxed);
        });
    }
    Ok(())
}

/// Element box geometry backing `getBoundingClientRect` and the observers.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_element_geometry(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    viewport_size: Arc<Mutex<[f32; 2]>>,
) -> JsResult<()> {
    // ── element geometry (for getBoundingClientRect / ResizeObserver / IntersectionObserver) ──
    // Returns [x, y, width, height] for the given NodeId in viewport-relative CSS px,
    // or undefined if the node has no layout box (display:none, not laid out yet, etc.).
    {
        let lr = Arc::clone(&layout_rects);
        reg!(scope, ctx, store, "_lumen_get_bounding_rect", move |nid: u32| -> Option<Vec<f64>> {
            lr.lock()
                .unwrap()
                .get(&nid)
                .map(|r| vec![f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3])])
        });
    }

    // Returns [width, height] of the current viewport in CSS px.
    {
        let vs = Arc::clone(&viewport_size);
        reg!(scope, ctx, store, "_lumen_get_viewport_size", move || -> Vec<f64> {
            let s = *vs.lock().unwrap();
            vec![f64::from(s[0]), f64::from(s[1])]
        });
    }
    Ok(())
}

/// `window.matchMedia` evaluation (CSS Media Queries L4 §4.2).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_match_media(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── window.matchMedia (CSS Media Queries L4 §4.2) ────────────────────────
    // Parses `query` as a media query and evaluates it against an ad-hoc
    // MediaContext built from the supplied viewport size + user-preference
    // flags. Pure function — no captures: parse_media_query and MediaQuery::matches
    // are stateless. Returns `true` when the query currently matches.
    reg!(scope, ctx, store, 
        "_lumen_match_media",
        |query: String, w: f64, h: f64, dark: bool, reduced_motion: bool| -> bool {
            let mq = lumen_css_parser::parse_media_query(&query);
            let ctx = lumen_css_parser::MediaContext {
                media_type: "screen".to_owned(),
                width: w as f32,
                height: h as f32,
                prefers_dark: dark,
                prefers_reduced_motion: reduced_motion,
                forced_colors: false,
                ..Default::default()
            };
            mq.matches(&ctx)
        }
    );
    Ok(())
}

/// `CSS.supports()` backing — and, sharing the region, the lazy-image load queue.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_css_supports_and_lazy_images(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
) -> JsResult<()> {
    // ── CSS.supports() backing (CSS Conditional Rules L3 §6) ──────────────────
    // Two-argument form: CSS.supports(property, value) → check property name.
    // Intentionally ignores value in Phase 0 (property-name check is sufficient
    // for the feature-detection patterns real sites use).
    reg!(scope, ctx, store, 
        "_lumen_css_supports_prop",
        |prop: String, _value: String| -> bool {
            lumen_css_parser::SUPPORTED_PROPERTIES
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&prop))
        }
    );
    // One-argument form: CSS.supports(conditionText) → parse + evaluate.
    reg!(scope, ctx, store, 
        "_lumen_css_supports_cond",
        |condition: String| -> bool {
            lumen_css_parser::parse_supports_condition(&condition)
                .evaluate(lumen_css_parser::SUPPORTED_PROPERTIES)
        }
    );

    // Queues a lazy image load request.  Called by `_lumen_deliver_lazy_images()` in JS
    // when an image registered via `_lumen_init_lazy_images` enters the lazy-load margin.
    // Shell drains via `QuickJsRuntime::take_lazy_image_requests` after each layout.
    {
        let req = Arc::clone(&lazy_img_requests);
        reg!(scope, ctx, store, "_lumen_request_lazy_image_load", move |nid: u32, url: String| {
            req.lock().unwrap().push((nid, url));
        });
    }
    Ok(())
}

/// Scroll offsets and scroll requests for containers and the page.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_scroll_state(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    page_scroll_y: Arc<Mutex<f32>>,
) -> JsResult<()> {
    // ── scroll state (for scrollTop/scrollLeft/scrollWidth/scrollHeight) ─────────
    // Returns [scroll_x, scroll_y, scroll_width, scroll_height] for an overflow container,
    // or undefined if the node is not a scroll container.
    {
        let ss = Arc::clone(&scroll_states);
        reg!(scope, ctx, store, "_lumen_get_scroll_state", move |nid: u32| -> Option<Vec<f64>> {
            ss.lock()
                .unwrap()
                .get(&nid)
                .map(|s| vec![f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), f64::from(s[3])])
        });
    }
    // Queues a programmatic scroll request.  Shell drains via `take_scroll_requests()`.
    {
        let ps = Arc::clone(&pending_scrolls);
        reg!(scope, ctx, store, "_lumen_request_scroll", move |nid: u32, x: f64, y: f64| {
            ps.lock().unwrap().push((nid, x as f32, y as f32));
        });
    }
    // Queues a page-level scroll request from window.scrollTo/scrollBy.
    // `smooth=1` → start_smooth_scroll; `smooth=0` → scroll_to (instant).
    {
        let pps = Arc::clone(&pending_page_scrolls);
        reg!(scope, ctx, store, "_lumen_request_page_scroll", move |y: f64, smooth: u32| {
            pps.lock().unwrap().push((y as f32, smooth != 0));
        });
    }
    // Returns current page scroll Y for window.scrollY / window.pageYOffset.
    {
        let psy = Arc::clone(&page_scroll_y);
        reg!(scope, ctx, store, "_lumen_get_page_scroll_y", move || -> f64 {
            f64::from(*psy.lock().unwrap())
        });
    }
    Ok(())
}

/// `window.open()` popup requests drained by the shell.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_window_open(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    window_open_requests: Arc<Mutex<Vec<crate::dom::PopupRequest>>>,
) -> JsResult<()> {
    // ── window.open() popup requests ────────────────────────────────────────────
    // Queues a popup window request. Shell drains via `take_window_open_requests()`.
    // `features` is the raw feature string ("width=800,height=600,..."); we parse
    // `width=` and `height=` here so the shell receives typed values.
    {
        let wor = Arc::clone(&window_open_requests);
        reg!(scope, ctx, store, 
            "_lumen_window_open",
            move |url: String, target: String, features: String| {
                let mut width: u32 = 800;
                let mut height: u32 = 600;
                for part in features.split(',') {
                    let part = part.trim();
                    if let Some(v) = part.strip_prefix("width=") {
                        width = v.trim().parse().unwrap_or(800);
                    } else if let Some(v) = part.strip_prefix("height=") {
                        height = v.trim().parse().unwrap_or(600);
                    }
                }
                wor.lock().unwrap().push(PopupRequest { url, target, width, height });
            }
        );
    }
    Ok(())
}

/// Fullscreen API requests (WHATWG Fullscreen §4).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_fullscreen(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    fullscreen_requests: Arc<Mutex<Vec<crate::dom::FullscreenRequest>>>,
) -> JsResult<()> {
    // ── Fullscreen API (WHATWG Fullscreen §4) ────────────────────────────────────
    // Shell drains via `take_fullscreen_requests()` and calls `window.set_fullscreen()`.
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!(scope, ctx, store, "_lumen_fs_enter", move |nid: u32| {
            fs_req.lock().unwrap().push(FullscreenRequest::Enter { nid });
        });
    }
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!(scope, ctx, store, "_lumen_fs_exit", move || {
            fs_req.lock().unwrap().push(FullscreenRequest::Exit);
        });
    }
    Ok(())
}

/// Pointer Lock API stubs (W3C Pointer Lock L2 §2-4).
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_pointer_lock(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── Pointer Lock API (W3C Pointer Lock L2 §2-4) ────────────────────────────────
    // requestPointerLock(element_nid) — lock pointer to element.
    // Phase 0: in-memory lock. Phase 1: integrate with shell to capture cursor.
    reg!(scope, ctx, store, "_lumen_ptr_lock_request", move |nid: u32| {
        crate::pointer_lock::request_pointer_lock(nid);
    });

    // exitPointerLock() — release pointer lock.
    reg!(scope, ctx, store, "_lumen_exit_ptr_lock", move || {
        crate::pointer_lock::exit_pointer_lock();
    });

    // pointerLockElement getter — returns locked element or null.
    reg!(scope, ctx, store, "_lumen_ptr_lock_element", move || -> Option<u32> {
        crate::pointer_lock::get_locked_element_nid()
    });
    Ok(())
}

/// `window.getComputedStyle` and custom-property reads off the cascade snapshot.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_computed_styles(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
) -> JsResult<()> {
    // ── Computed styles (window.getComputedStyle) ────────────────────────────────
    // Returns the resolved CSS value for `prop` on node `nid`, or "" if unknown.
    {
        let cs = Arc::clone(&computed_styles);
        reg!(scope, ctx, store, "_lumen_get_computed_style", move |nid: u32, prop: String| -> String {
            cs.lock()
                .unwrap()
                .get(&nid)
                .and_then(|m| m.get(&prop))
                .cloned()
                .unwrap_or_default()
        });
    }
    // Resolved value of the custom property `prop` (`--`-prefixed) on node
    // `nid`, or "" when the node declares/inherits none (BUG-732). Separate
    // from `_lumen_get_computed_style` because custom properties live in their
    // own inherited, `Arc`-shared map — see `V8JsRuntime::custom_properties`.
    {
        let cp = Arc::clone(&custom_properties);
        reg!(scope, ctx, store, "_lumen_get_custom_property", move |nid: u32, prop: String| -> String {
            cp.lock()
                .unwrap()
                .get(&nid)
                .and_then(|m| m.get(&prop))
                .cloned()
                .unwrap_or_default()
        });
    }
    Ok(())
}

/// `_lumen_drain_microtasks` — a no-op under V8, kept so the global exists.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_microtask_drain(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
) -> JsResult<()> {
    // ── Microtask drain ─────────────────────────────────────────────────────
    // TODO(v8-s3): needs isolate access — draining V8's microtask queue requires
    // `scope.perform_microtask_checkpoint()` on the isolate, which compat-layer
    // closures (JsValue-level only) cannot reach. Stubbed as a no-op so the global
    // exists; V8 auto-runs microtasks after each script/task by default so this
    // primitive (only used to force-flush in QuickJS unit tests) is not required
    // for correctness under V8. Revisit if a future slice needs manual draining.
    reg!(scope, ctx, store, "_lumen_drain_microtasks", move || {});
    Ok(())
}

/// Web Crypto and SubtleCrypto — and, sharing the region, Trusted Types, `chrome.runtime` and CSS Typed OM.
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_crypto_and_typed_om(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    doc: Arc<Mutex<lumen_dom::Document>>,
    dom_dirty: Arc<AtomicBool>,
    dom_touched: Arc<Mutex<DomTouched>>,
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
) -> JsResult<()> {
    // ── Web Crypto API ──────────────────────────────────────────────────────
    {
        // Returns `n` cryptographically-random bytes as a Vec<u8> (JS Array of
        // integers 0–255). Capped at 65 536 per call per WebCrypto spec §10.1.3.
        reg!(scope, ctx, store, "_lumen_get_random_bytes", |n: u32| -> Vec<u8> {
            let len = (n as usize).min(65_536);
            let mut buf = vec![0u8; len];
            getrandom::getrandom(&mut buf).unwrap_or(());
            buf
        });

        // Computes a SHA digest using the named algorithm.
        // `algo` must be one of "SHA-1", "SHA-256", "SHA-384", "SHA-512".
        // `data` is the raw input bytes.  Returns empty Vec on unknown algo.
        reg!(scope, ctx, store, 
            "_lumen_sha_digest",
            |algo: String, data: Vec<u8>| -> Vec<u8> {
                // sha1::Digest trait must be in scope to call sha1::Sha1::digest().
                use sha1::Digest as _;
                match algo.as_str() {
                    "SHA-1" => sha1::Sha1::digest(&data).to_vec(),
                    "SHA-256" => sha2::Sha256::digest(&data).to_vec(),
                    "SHA-384" => sha2::Sha384::digest(&data).to_vec(),
                    "SHA-512" => sha2::Sha512::digest(&data).to_vec(),
                    _ => Vec::new(),
                }
            }
        );

        // Compression Streams codecs (`CompressionStream` /
        // `DecompressionStream`). Stateful and keyed by an opaque handle
        // because the spec compresses/decompresses per chunk — the one-shot
        // `bytes -> bytes` pair these replaced had nowhere to keep the codec
        // between chunks, so nothing was decoded until `writer.close()`
        // (BUG-846). Status-byte protocol: `crate::compression`.
        reg!(scope, ctx, store, "_lumen_cs_new", |format: String, decompress: bool| -> f64 {
            f64::from(crate::compression::cs_new(&format, decompress))
        });
        reg!(scope, ctx, store, "_lumen_cs_push", |handle: f64, data: Vec<u8>| -> Vec<u8> {
            crate::compression::cs_push(handle as u32, &data)
        });
        reg!(scope, ctx, store, "_lumen_cs_finish", |handle: f64| -> Vec<u8> {
            crate::compression::cs_finish(handle as u32)
        });
        reg!(scope, ctx, store, "_lumen_cs_free", |handle: f64| {
            crate::compression::cs_free(handle as u32);
        });
    }

    // SubtleCrypto: generateKey/importKey/exportKey/sign/verify/encrypt/decrypt
    // The underlying key store and algorithm functions in `crate::subtle_crypto`
    // are plain Rust (no JS-engine dependency), so this is a thin wrapper.
    {
        reg!(scope, ctx, store, 
            "_lumen_subtle_generate_key",
            |alg_json: String, extractable: bool, usages_json: String| -> String {
                crate::subtle_crypto::generate_key(&alg_json, extractable, &usages_json)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_import_key",
            |format: String, key_data: Vec<u8>, alg_json: String, extractable: bool, usages_json: String| -> String {
                crate::subtle_crypto::import_key(&format, key_data, &alg_json, extractable, &usages_json)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_export_key",
            |format: String, key_id: u32| -> Vec<u8> {
                crate::subtle_crypto::export_key(&format, key_id).unwrap_or_default()
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_export_key_or_err",
            |format: String, key_id: u32| -> String {
                match crate::subtle_crypto::export_key(&format, key_id) {
                    Ok(bytes) => {
                        if bytes.first() == Some(&b'{') || bytes.first() == Some(&b'[') {
                            format!("ok:{}", String::from_utf8_lossy(&bytes))
                        } else {
                            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                            format!("hex:{hex}")
                        }
                    }
                    Err(e) => format!("err:{e}"),
                }
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_sign",
            |alg_json: String, key_id: u32, data: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::sign_data(&alg_json, key_id, &data)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_verify",
            |alg_json: String, key_id: u32, sig: Vec<u8>, data: Vec<u8>| -> bool {
                crate::subtle_crypto::verify_signature(&alg_json, key_id, &sig, &data)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_encrypt",
            |key_id: u32, iv: Vec<u8>, aad: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_gcm_encrypt(key_id, &iv, &aad, &plaintext)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_decrypt",
            |key_id: u32, iv: Vec<u8>, aad: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_gcm_decrypt(key_id, &iv, &aad, &ciphertext)
            }
        );

        reg!(scope, ctx, store, "_lumen_subtle_key_info", |key_id: u32| -> String {
            crate::subtle_crypto::key_info(key_id)
        });

        reg!(scope, ctx, store, 
            "_lumen_subtle_aes_cbc_encrypt",
            |key_id: u32, iv: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_cbc_encrypt(key_id, &iv, &plaintext)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_aes_cbc_decrypt",
            |key_id: u32, iv: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_cbc_decrypt(key_id, &iv, &ciphertext)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_aes_ctr_crypt",
            |key_id: u32, counter: Vec<u8>, length: u32, data: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::aes_ctr_crypt(key_id, &counter, length, &data)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_derive_bits",
            |alg_json: String, key_id: u32, length_bits: u32| -> Vec<u8> {
                crate::subtle_crypto::derive_bits(&alg_json, key_id, length_bits)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_rsa_oaep_encrypt",
            |key_id: u32, label: Vec<u8>, plaintext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::rsa_oaep_encrypt(key_id, &label, &plaintext)
            }
        );

        reg!(scope, ctx, store, 
            "_lumen_subtle_rsa_oaep_decrypt",
            |key_id: u32, label: Vec<u8>, ciphertext: Vec<u8>| -> Vec<u8> {
                crate::subtle_crypto::rsa_oaep_decrypt(key_id, &label, &ciphertext)
            }
        );
    }

    // Trusted Types API: trustedTypes.createPolicy(), TrustedHTML/Script/ScriptURL
    // (S12b-24-trusted-types) The shim itself is plain JS (`TRUSTED_TYPES_SHIM`, no
    // rquickjs-specific API), so it's evaluated inline alongside `WEB_API_SHIM` further
    // down in this function rather than reusing `crate::trusted_types::install_trusted_types_bindings`
    // (that helper takes an `rquickjs::Ctx` and stays QuickJS-only).

    // D-6: Extension system — chrome.runtime.sendMessage() native binding.
    // Phase 0: no-op; the message is logged to stderr for debugging.
    // Phase 1: shell wires a real IPC channel between content scripts and extension background.
    reg!(scope, ctx, store, "_lumen_chrome_runtime_send_message", |msg: String| {
        let _ = msg;
    });

    // CSS Typed OM API: element.attributeStyleMap / computedStyleMap()
    //
    // BUG-387: the two maps have **separate** backing bindings on purpose.
    // `attributeStyleMap` reflects only the inline `style=""` attribute (that is
    // what the spec says a mutable `StylePropertyMap` is), while
    // `computedStyleMap()` must answer from the cascade — it reads the same
    // `computed_styles` / `custom_properties` snapshots as
    // `window.getComputedStyle` (`_lumen_get_computed_style`,
    // `_lumen_get_custom_property` above), never the inline attribute.
    {
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_style_property", move |nid: u32, prop: String| -> String {
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    let parsed = _parse_style_string(style_attr);
                    return parsed.get(&_css_property_key(&prop)).cloned().unwrap_or_default();
                }
            }
            String::new()
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_set_style_property", move |nid: u32, prop: String, val: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let old_style = doc.get(node_id).get_attr("style").map(|s| s.to_string());
                let mut parsed = if let Some(style) = old_style.as_deref() {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                parsed.insert(_css_property_key(&prop), val);
                let css_text = _serialize_style_map(&parsed);
                set_attribute(&mut doc, node_id, "style", &css_text);
                if old_style.as_deref() != Some(css_text.as_str()) {
                    record_dom_touch(&touched, node_id);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        let touched = Arc::clone(&dom_touched);
        reg!(scope, ctx, store, "_lumen_delete_style_property", move |nid: u32, prop: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let old_style = doc.get(node_id).get_attr("style").map(|s| s.to_string());
                let mut parsed = if let Some(style) = old_style.as_deref() {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                parsed.remove(&_css_property_key(&prop));
                let css_text = _serialize_style_map(&parsed);
                if css_text.is_empty() {
                    remove_attribute(&mut doc, node_id, "style");
                } else {
                    set_attribute(&mut doc, node_id, "style", &css_text);
                }
                let new_style = if css_text.is_empty() { None } else { Some(css_text.as_str()) };
                if old_style.as_deref() != new_style {
                    record_dom_touch(&touched, node_id);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        });
        // No `_lumen_has_style_property` here any more (BUG-387): `has()` is
        // now `get() !== undefined` on both maps, which is what §6.1 says it
        // means and keeps the two answers from ever disagreeing. The old native
        // answered a `contains_key` over the inline attribute and was reachable
        // from the inline map only — exactly the kind of second reader this bug
        // was about.
        //
        // Declarations of the inline `style=""` attribute, as a JSON array of
        // `[property, value]` pairs sorted by property name — the iteration
        // source of `attributeStyleMap`. Used to return the literal `"[]"`
        // (BUG-387): the map's `entries()`/`keys()`/`values()` were dead, and
        // dead in a way that threw, since the JS shim called `.entries()` on
        // that *string*.
        let d = Arc::clone(&doc);
        reg!(scope, ctx, store, "_lumen_get_style_entries", move |nid: u32| -> String {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    pairs = _parse_style_string(style_attr).into_iter().collect();
                }
            }
            _style_entries_to_json(pairs)
        });
        // Iteration source of `computedStyleMap()`: the resolved cascade, i.e.
        // exactly what `getComputedStyle` answers from — standard properties
        // plus this node's resolved custom properties (BUG-732 keeps the latter
        // in their own `Arc`-shared map, so they are merged here rather than
        // stored per node).
        let cs = Arc::clone(&computed_styles);
        let cp = Arc::clone(&custom_properties);
        reg!(scope, ctx, store, "_lumen_get_computed_style_entries", move |nid: u32| -> String {
            let mut pairs: Vec<(String, String)> = Vec::new();
            if let Ok(map) = cs.lock()
                && let Some(m) = map.get(&nid)
            {
                pairs.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            if let Ok(map) = cp.lock()
                && let Some(m) = map.get(&nid)
            {
                // A custom property that could not be resolved is published
                // with an empty value (guaranteed-invalid); the Typed OM map
                // has nothing to hand out for it, so it is not an entry.
                pairs.extend(
                    m.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())),
                );
            }
            _style_entries_to_json(pairs)
        });
    }
    Ok(())
}
