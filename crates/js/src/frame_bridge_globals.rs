//! BUG-979 — natives backing `winFacade`'s fallback onto a peer frame's real
//! `globalThis`: `_lumen_f_global_get`/`_lumen_f_global_call`.
//!
//! Split out of `frame_bridge.rs` (already over the file-size cap — see
//! `CLAUDE.md` §Code conventions) rather than grown into it. Registered
//! alongside [`crate::frame_bridge::install_frame_bridge_v8`] against the
//! SAME registry (`v8_runtime.rs`'s `install_dom`), reusing its bid
//! resolution ([`crate::frame_bridge::resolve_slot`]) and accessibility gate
//! — a fallback global read/call is a strictly MORE powerful capability than
//! anything the existing `_lumen_f_*` whitelist exposes (arbitrary same-
//! origin code execution via `peer_global_call`), so it is gated on
//! `binding.accessible` exactly like `.document`/mutation natives, never
//! looser.

use crate::frame_bridge::{FrameDocRegistry, resolve_slot};
use crate::frame_peer_bridge::{enter_call, envelope_tag};
use crate::v8_compat::{into_v8_fn2, into_v8_fn3};
use lumen_core::JsValue;
use std::sync::Arc;

/// Resolve `bid` to `(peer handle, caller doc key, target doc key)`, or an
/// envelope explaining why not (missing binding / not accessible / no live
/// peer runtime). Locks the registry only long enough to clone the `Arc`s it
/// needs — the actual cross-isolate call happens after the lock is dropped,
/// so it never blocks unrelated natives on this registry for the duration of
/// a peer's `run()`.
fn resolve_peer(
    registry: &FrameDocRegistry,
    bid: u32,
) -> Result<(Arc<dyn crate::frame_peer_bridge::FramePeerBridge>, usize, usize), JsValue> {
    let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(binding) = resolve_slot(&reg, bid) else {
        return Err(envelope_tag("absent"));
    };
    if !binding.accessible {
        return Err(envelope_tag("absent"));
    }
    // BUG-1682: фрейм, которого shell уже выбросил, — такой же «нет рантайма».
    let Some(peer) = binding.peer.as_ref().and_then(std::sync::Weak::upgrade) else {
        return Err(envelope_tag("absent"));
    };
    let from = reg.self_key.unwrap_or(0);
    let to = Arc::as_ptr(&binding.doc) as usize;
    Ok((peer, from, to))
}

/// Register `_lumen_f_global_get`/`_lumen_f_global_call` against `registry`
/// — call once per V8 context, right after
/// [`crate::frame_bridge::install_frame_bridge_v8`] against the same
/// `Arc<Mutex<FrameDocSlots>>`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_frame_bridge_globals_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    registry: FrameDocRegistry,
) -> lumen_core::JsResult<()> {
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_global_get",
            into_v8_fn2(move |bid: u32, name: String| -> JsValue {
                match resolve_peer(&reg, bid) {
                    Ok((peer, from, to)) => match enter_call(from, to) {
                        Ok(_guard) => peer.peer_global_get(&name),
                        Err(env) => env,
                    },
                    Err(env) => env,
                }
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_global_call",
            into_v8_fn3(move |bid: u32, name: String, args: JsValue| -> JsValue {
                let args = match args {
                    JsValue::Array(items) => items,
                    JsValue::Null | JsValue::Undefined => Vec::new(),
                    other => vec![other],
                };
                match resolve_peer(&reg, bid) {
                    Ok((peer, from, to)) => match enter_call(from, to) {
                        Ok(_guard) => peer.peer_global_call(&name, &args),
                        Err(env) => env,
                    },
                    Err(env) => {
                        let _ = args;
                        env
                    }
                }
            }),
        )?;
    }
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)] // test code — same as `frame_bridge.rs`'s own test module
    use super::*;
    use crate::frame_bridge::{FrameDocBinding, FrameDocSlots, install_frame_bridge_v8};
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use std::sync::Mutex;

    /// Minimal two-isolate fixture: a "parent" runtime with a `frames[0]`
    /// binding pointing at a "child" runtime that defines `window.mark` and
    /// `window.greet(name)`. `accessible` controls whether the binding is
    /// gated open, matching `frame_bridge.rs`'s own cross-isolate test setup
    /// (`sibling_runtime_over`-shaped, but with a real second runtime instead
    /// of sharing one `Document`, since this bug is about the JS heap, not
    /// the DOM tree).
    fn with_parent_and_child(
        child_script: &str,
        accessible: bool,
        f: impl FnOnce(&V8JsRuntime),
    ) {
        let child_rt = Arc::new(V8JsRuntime::new().unwrap());
        child_rt.eval("var window = globalThis;").unwrap();
        child_rt.eval(child_script).unwrap();

        let parent_rt = V8JsRuntime::new().unwrap();
        parent_rt.eval("var window = globalThis;").unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        install_frame_bridge_v8(&parent_rt, Arc::clone(&registry)).unwrap();
        install_frame_bridge_globals_v8(&parent_rt, Arc::clone(&registry)).unwrap();
        let child_doc = Arc::new(Mutex::new(lumen_html_parser::parse("<html></html>")));
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: child_doc,
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible,
            opaque_id: None,
            peer: Some(Arc::downgrade(&child_rt) as std::sync::Weak<dyn crate::frame_peer_bridge::FramePeerBridge>),
        });
        f(&parent_rt);
        // Реестр держит ребёнка слабо (BUG-1682) — живым его держит фикстура.
        drop(child_rt);
    }

    fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
        use lumen_core::ext::JsRuntime as _;
        match rt.eval(script).unwrap() {
            lumen_core::JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    fn eval_bool(rt: &V8JsRuntime, script: &str) -> bool {
        use lumen_core::ext::JsRuntime as _;
        matches!(rt.eval(script).unwrap(), lumen_core::JsValue::Bool(true))
    }

    #[test]
    fn reads_a_plain_global_the_child_declared_itself() {
        with_parent_and_child("window.mark = 'hello-from-child';", true, |rt| {
            assert_eq!(
                eval_str(rt, "_lumen_f_global_get(0, 'mark').value"),
                "hello-from-child"
            );
        });
    }

    #[test]
    fn absent_global_reads_as_absent_not_a_thrown_error() {
        with_parent_and_child("", true, |rt| {
            assert_eq!(eval_str(rt, "_lumen_f_global_get(0, 'nope').kind"), "absent");
        });
    }

    #[test]
    fn a_function_global_reads_as_the_function_marker() {
        with_parent_and_child("window.prepareForTest = function(){};", true, |rt| {
            assert_eq!(eval_str(rt, "_lumen_f_global_get(0, 'prepareForTest').kind"), "function");
        });
    }

    #[test]
    fn calls_a_global_function_the_child_declared_and_reads_its_result() {
        // Mirrors the real WPT shape this bug is about:
        // `testWindow.prepareForTest(...)` — a plain top-level function
        // declaration in the framed document, called synchronously by the
        // parent and its return value used the same turn.
        with_parent_and_child(
            "window.prepareForTest = function(tag) { return 'prepared:' + tag; };",
            true,
            |rt| {
                assert_eq!(
                    eval_str(rt, "_lumen_f_global_call(0, 'prepareForTest', ['case-1']).value"),
                    "prepared:case-1"
                );
            },
        );
    }

    #[test]
    fn content_window_facade_reaches_a_real_global_the_child_declared() {
        // End-to-end shape of the bug: `iframe.contentWindow.foo` (the
        // fixture's `host_nid: 7`, same as `_lumen_frame_content_window`'s
        // `hostNid` argument) reaching a plain global the child's own script
        // declared, through `winFacade`'s Proxy fallback — not the raw
        // `_lumen_f_global_get`/`_lumen_f_global_call` natives directly, the
        // way the other tests in this module exercise it.
        with_parent_and_child(
            "window.mark = 'hi'; window.prepareForTest = function(tag) { return 'prepared:' + tag; };",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "_lumen_frame_content_window(7).mark === 'hi'"
                ));
                assert!(eval_bool(
                    rt,
                    "_lumen_frame_content_window(7).prepareForTest('case-1') === 'prepared:case-1'"
                ));
                // Still fully whitelisted for the fixed IDL set — the Proxy's
                // `get` trap must defer to the real own property instead of
                // round-tripping through the bridge for names it already has.
                assert!(eval_bool(
                    rt,
                    "typeof _lumen_frame_content_window(7).postMessage === 'function' \
                     && _lumen_frame_content_window(7).closed === false"
                ));
            },
        );
    }

    #[test]
    fn new_on_facade_constructor_builds_a_real_instance_not_a_stub() {
        // BUG-1099: `new iframe.contentWindow.Worker(url)`.
        with_parent_and_child("window.Ctor = function() { this.peer = true; };", true, |rt| {
            use lumen_core::ext::JsRuntime as _;
            rt.eval(
                "globalThis.Ctor = function(a) { this.arg = a; }; \
                 Ctor.prototype.stop = function() { return 'stopped'; };",
            )
            .unwrap();
            assert!(eval_bool(
                rt,
                "var o = new (_lumen_frame_content_window(7).Ctor)(5); \
                 o.arg === 5 && o.stop() === 'stopped'"
            ));
        });
    }

    #[test]
    fn calling_a_missing_function_reports_an_error_envelope_not_a_panic() {
        with_parent_and_child("", true, |rt| {
            assert!(eval_bool(rt, "_lumen_f_global_call(0, 'nope', []).kind === 'error'"));
        });
    }

    #[test]
    fn a_thrown_exception_surfaces_as_an_error_envelope() {
        with_parent_and_child(
            "window.boom = function() { throw new TypeError('nope'); };",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var r = _lumen_f_global_call(0, 'boom', []); \
                     r.kind === 'error' && r.message.indexOf('nope') !== -1"
                ));
            },
        );
    }

    #[test]
    fn cross_origin_binding_never_leaks_a_global() {
        // accessible=false: same shape as cross-origin/opaque-sandbox
        // bindings everywhere else in this bridge — a global read/call is
        // strictly more powerful than the existing whitelist, so it must
        // never be reachable when `.document`/mutation natives already are
        // not.
        with_parent_and_child("window.secret = 'leak-me-not';", false, |rt| {
            assert_eq!(eval_str(rt, "_lumen_f_global_get(0, 'secret').kind"), "absent");
        });
    }

    /// BUG-1682: same-origin родитель и ребёнок ссылаются друг на друга
    /// (`frames[i].peer` ↔ `parent.peer`), но оба рантайма умирают, как только
    /// их отпускает владелец — shell. Раньше это был цикл `Arc`, и поток с
    /// изолятом каждого фрейма жил до конца процесса.
    #[test]
    fn mutual_peers_do_not_keep_runtimes_alive() {
        use crate::frame_peer_bridge::FramePeerBridge;
        let parent = Arc::new(V8JsRuntime::new().unwrap());
        let child = Arc::new(V8JsRuntime::new().unwrap());
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse("<iframe></iframe>")));
        let child_doc = Arc::new(Mutex::new(lumen_html_parser::parse("<p>hi")));
        parent.register_frame_document(
            7,
            child_doc,
            "https://a.test/child".to_owned(),
            None,
            true,
            false,
            Some(Arc::clone(&child) as Arc<dyn FramePeerBridge>),
        );
        child.register_parent_document(
            7,
            parent_doc,
            "https://a.test/".to_owned(),
            None,
            true,
            Some(Arc::clone(&parent) as Arc<dyn FramePeerBridge>),
        );
        let (weak_parent, weak_child) = (Arc::downgrade(&parent), Arc::downgrade(&child));
        drop(child);
        assert!(weak_child.upgrade().is_none(), "ребёнка держал реестр родителя");
        drop(parent);
        assert!(weak_parent.upgrade().is_none(), "родителя держал слот parent ребёнка");
    }

    /// BUG-1682: биндинг рантайма, которого уже нет, отвечает как биндинг
    /// без рантайма, а не падает.
    #[test]
    fn dead_peer_reads_as_absent() {
        let parent_rt = V8JsRuntime::new().unwrap();
        parent_rt.eval("var window = globalThis;").unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        install_frame_bridge_v8(&parent_rt, Arc::clone(&registry)).unwrap();
        install_frame_bridge_globals_v8(&parent_rt, Arc::clone(&registry)).unwrap();
        let child_rt = Arc::new(V8JsRuntime::new().unwrap());
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(lumen_html_parser::parse("<html></html>"))),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible: true,
            opaque_id: None,
            peer: Some(Arc::downgrade(&child_rt) as std::sync::Weak<dyn crate::frame_peer_bridge::FramePeerBridge>),
        });
        drop(child_rt);
        assert_eq!(eval_str(&parent_rt, "_lumen_f_global_get(0, 'mark').kind"), "absent");
    }

    /// BUG-1682: регистрация нового фрейма хоронит биндинг хоста, вынутого из
    /// документа, на его месте — `bid` соседей не сдвигается, `window.length`
    /// считает только живые, под-документ вынутого отпущен.
    #[test]
    fn registering_a_new_frame_buries_bindings_of_removed_hosts() {
        use crate::frame_bridge::{DETACHED_HOST, upsert_binding};
        let doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<body><iframe></iframe><iframe></iframe><iframe></iframe></body>",
        )));
        let hosts: Vec<u32> =
            lumen_dom::collect_iframes(&doc.lock().unwrap()).iter().map(|i| i.node.index() as u32).collect();
        assert_eq!(hosts.len(), 3);
        let mut slots = FrameDocSlots { self_doc: Some(Arc::clone(&doc)), ..FrameDocSlots::default() };
        let binding = |host_nid: u32, child: &Arc<Mutex<lumen_dom::Document>>| FrameDocBinding {
            host_nid,
            doc: Arc::clone(child),
            url: "about:blank".to_owned(),
            name: None,
            accessible: true,
            opaque_id: None,
            peer: None,
        };
        let first = Arc::new(Mutex::new(lumen_dom::Document::new()));
        let second = Arc::new(Mutex::new(lumen_dom::Document::new()));
        assert_eq!(upsert_binding(&mut slots, binding(hosts[0], &first)), 0);
        assert_eq!(upsert_binding(&mut slots, binding(hosts[1], &second)), 1);

        doc.lock().unwrap().detach(lumen_dom::NodeId::from_index(hosts[0] as usize));
        // Повторная регистрация живого хоста (навигация фрейма) не хоронит.
        assert_eq!(upsert_binding(&mut slots, binding(hosts[1], &second)), 1);
        assert_eq!(slots.frames[0].host_nid, hosts[0]);

        let third = Arc::new(Mutex::new(lumen_dom::Document::new()));
        assert_eq!(upsert_binding(&mut slots, binding(hosts[2], &third)), 2, "индексы не сдвинулись");
        assert_eq!(slots.frames[0].host_nid, DETACHED_HOST);
        assert!(slots.frames[0].peer.is_none());
        assert_eq!(Arc::strong_count(&first), 1, "под-документ вынутого фрейма отпущен");
        assert_eq!(slots.frames[1].host_nid, hosts[1]);
        assert_eq!(slots.frames.iter().filter(|b| b.host_nid != DETACHED_HOST).count(), 2);
    }
}
