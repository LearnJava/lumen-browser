//! GAP-P3GCJSDOM срез 2 — forced-GC round-trip test for the wrapper
//! refcount edge wired in срез 1 (`_lumen_wrapper_cache_set`/`_get`,
//! `_lumen_dom_acquire_ref`/`_lumen_dom_release_ref`, docs/tasks/
//! ph3-gc-js-dom.md). Uses [`V8JsRuntime::force_gc_for_testing`] to make
//! V8 actually collect a wrapper `WeakRef` and fire its
//! `FinalizationRegistry` callback — without it, `js_refs` in `lumen-dom`
//! is only exercised by the two direct-call unit tests in
//! `crates/engine/dom/src/lib.rs`, never by a real V8 wrapper lifecycle.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// A detached node with no live JS variable pointing at its wrapper: the
/// wrapper is collected, `_lumen_node_wrapper_finalizer` fires, and
/// `js_ref_count` returns to 0 — `Document::dead_node_ids` can now see it.
#[test]
fn detached_node_with_no_js_reference_is_released_after_gc() {
    let doc = make_doc();
    let main = find_element_by_tag(&doc.lock().unwrap(), "div").expect("fixture div");
    let rt = v8_runtime_with_dom(doc.clone());

    // Build a wrapper for `main`, detach it from the document, then drop the
    // only JS reference to that wrapper.
    rt.eval(
        "(function() { \
             var el = document.getElementById('main'); \
             el.remove(); \
         })();",
    )
    .unwrap();

    assert_eq!(
        doc.lock().unwrap().js_ref_count(main),
        1,
        "acquire_js_ref must have fired exactly once for the single wrapper built above"
    );
    assert!(doc.lock().unwrap().is_detached(main));

    rt.force_gc_for_testing();

    assert_eq!(
        doc.lock().unwrap().js_ref_count(main),
        0,
        "the wrapper's only JS reference went out of scope before GC; the \
         FinalizationRegistry callback must have released it"
    );
    assert!(
        doc.lock().unwrap().dead_node_ids().contains(&main),
        "detached + zero js_refs must make the node collectable"
    );
}

/// The "no false collection" requirement from the task's Definition of
/// Done: a detached node that a live global JS variable still points at
/// must **not** show up as collectable, even after a forced GC pass.
#[test]
fn detached_node_with_live_js_reference_survives_gc() {
    let doc = make_doc();
    let main = find_element_by_tag(&doc.lock().unwrap(), "div").expect("fixture div");
    let rt = v8_runtime_with_dom(doc.clone());

    rt.eval(
        "globalThis._kept = document.getElementById('main'); \
         globalThis._kept.remove();",
    )
    .unwrap();

    assert!(doc.lock().unwrap().is_detached(main));
    rt.force_gc_for_testing();

    assert_eq!(
        doc.lock().unwrap().js_ref_count(main),
        1,
        "globalThis._kept is a live root — the wrapper must survive GC"
    );
    assert!(
        !doc.lock().unwrap().dead_node_ids().contains(&main),
        "a detached node with a reachable JS reference is not collectable"
    );

    // Dropping the last reference and forcing GC again releases it — proves
    // the survival above was really about reachability, not a stuck ref.
    rt.eval("globalThis._kept = null;").unwrap();
    rt.force_gc_for_testing();
    assert_eq!(doc.lock().unwrap().js_ref_count(main), 0);
    assert!(doc.lock().unwrap().dead_node_ids().contains(&main));
}

/// An **attached** node with a live listener must never be collected, even
/// though nothing here keeps a JS variable pointing at its wrapper — the
/// document tree itself is a root via `getElementById`, so a fresh wrapper
/// is minted (and cached) on every access rather than ever going to zero
/// while the node stays attached.
#[test]
fn attached_node_is_never_reported_dead() {
    let doc = make_doc();
    let main = find_element_by_tag(&doc.lock().unwrap(), "div").expect("fixture div");
    let rt = v8_runtime_with_dom(doc.clone());

    rt.eval(
        "document.getElementById('main').addEventListener('click', function() {});",
    )
    .unwrap();
    rt.force_gc_for_testing();

    assert!(!doc.lock().unwrap().is_detached(main));
    assert!(
        !doc.lock().unwrap().dead_node_ids().contains(&main),
        "an attached node must never be in dead_node_ids regardless of js_refs"
    );
}

/// Regression guard for [`V8JsRuntime::force_gc_for_testing`] itself,
/// independent of any DOM wrapper: a `WeakRef` to a locally-scoped object
/// must dereference to `undefined` once nothing else roots the target. If
/// this fails, the DOM-specific tests above cannot be trusted either — the
/// forcing mechanism, not `lumen-dom`'s refcounting, would be the culprit.
#[test]
fn probe_bare_weakref_deref_becomes_undefined_under_force_gc() {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval(
        "(function() { var o = {}; globalThis._wr = new WeakRef(o); })();",
    )
    .unwrap();
    rt.force_gc_for_testing();
    let alive = rt.eval("globalThis._wr.deref() !== undefined").unwrap();
    assert_eq!(
        alive,
        lumen_core::JsValue::Bool(false),
        "bare WeakRef target survived force_gc_for_testing — the target is somehow still reachable"
    );
}

/// Twin of the `WeakRef` probe above, but for `FinalizationRegistry` — the
/// mechanism `_lumen_node_wrapper_finalizer` (`web_api_shim_mid.js`) relies
/// on. `force_gc_for_testing` needed `Platform::pump_message_loop` to make
/// this pass at all: the cleanup callback is a task V8 posts to the
/// embedder's platform queue, not a microtask, and nothing else in this
/// runtime ever pumps that queue.
#[test]
fn probe_bare_finalization_registry_fires_under_force_gc() {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval(
        "globalThis._fired = 0; \
         globalThis._reg = new FinalizationRegistry(function() { globalThis._fired++; }); \
         (function() { var o = {}; globalThis._reg.register(o, 1); })();",
    )
    .unwrap();
    rt.force_gc_for_testing();
    let fired = match rt.eval("globalThis._fired").unwrap() {
        lumen_core::JsValue::Number(n) => n,
        other => panic!("expected a number, got {other:?}"),
    };
    assert_eq!(fired, 1.0, "bare FinalizationRegistry never fired under force_gc_for_testing");
}

/// Sanity check on the fixture: pins down that `document.getElementById`
/// and `document.querySelector` resolve to the same underlying node the
/// other tests in this file look up by Rust-side `NodeId` via
/// [`find_element_by_tag`], so a future change to `super::make_doc` that
/// adds a second `div` (or moves the `id="main"` one) fails loudly here
/// instead of silently invalidating every assertion above.
#[test]
fn fixture_div_has_id_main() {
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc.clone());
    assert_eq!(
        rt.eval("document.getElementById('main') === document.querySelector('div')").unwrap(),
        lumen_core::JsValue::Bool(true)
    );
}
