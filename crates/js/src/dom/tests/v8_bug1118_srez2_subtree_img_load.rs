//! BUG-1118 срез 2 — `<img src>` built as a ready subtree (not assigned via
//! `_lumen_set_attr`) must still trigger [`lumen_core::ext::ImageLoadHook`]
//! immediately: `innerHTML`, `insertAdjacentHTML`, `appendChild`/
//! `insertBefore` of a `cloneNode(true)` copy that already carried `src`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::ext::ImageLoadHook;

/// Records every `raw_src` the runtime queued, in call order.
#[derive(Default)]
struct RecordingHook {
    queued: Mutex<Vec<String>>,
    /// BUG-1048: the `nid` passed alongside each entry of `queued`.
    nids: Mutex<Vec<u32>>,
}

impl ImageLoadHook for RecordingHook {
    fn queue_image_load(&self, nid: u32, raw_src: &str) {
        self.queued
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(raw_src.to_string());
        self.nids.lock().unwrap_or_else(|e| e.into_inner()).push(nid);
    }
}

fn runtime_with_hook(doc: Arc<Mutex<Document>>) -> (V8JsRuntime, Arc<RecordingHook>) {
    let hook = Arc::new(RecordingHook::default());
    let rt = V8JsRuntime::new()
        .unwrap()
        .with_image_load_hook(Arc::clone(&hook) as Arc<dyn ImageLoadHook>);
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    (rt, hook)
}

/// `innerHTML = '<img src="...">'` parses the subtree straight from markup —
/// `src` is never assigned through `_lumen_set_attr`, so this exercises the
/// `_lumen_set_inner_html` hook point.
#[test]
fn inner_html_img_src_queues_immediately() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval("document.getElementById('main').innerHTML = '<img src=\"/dyn.png\">';")
        .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    assert_eq!(queued, vec!["/dyn.png".to_string()]);
}

/// `cloneNode(true)` of an element that already had `src` set, then
/// `appendChild` — no `setAttribute`/`.src =` call happens on the clone.
#[test]
fn append_child_of_cloned_img_queues_immediately() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval(
        "var i = document.createElement('img'); \
         i.setAttribute('src', '/first.png'); \
         var clone = i.cloneNode(true); \
         document.getElementById('main').appendChild(clone);",
    )
    .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    // `/first.png` fires once from the original `setAttribute` (срез 1) and
    // the clone's `appendChild` queues it again (срез 2) — both hit the same
    // shared dedup set in the real `DynamicImgFetchHook`, so a duplicate
    // here is expected and harmless.
    assert_eq!(queued, vec!["/first.png".to_string(), "/first.png".to_string()]);
}

/// `insertBefore` of a subtree containing an `<img src>` must queue too.
#[test]
fn insert_before_of_img_subtree_queues_immediately() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval(
        "var wrap = document.createElement('div'); \
         wrap.innerHTML = '<img src=\"/nested.png\">'; \
         var main = document.getElementById('main'); \
         main.insertBefore(wrap, main.firstChild);",
    )
    .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    // `wrap.innerHTML` queues `/nested.png` once (its own `_lumen_set_inner_html`
    // hook point, `wrap` is detached at that moment) and `insertBefore` walks
    // the subtree again and queues it a second time — same
    // fires-twice-relies-on-the-real-dedup-set shape as
    // `append_child_of_cloned_img_queues_immediately` above.
    assert_eq!(
        queued,
        vec!["/nested.png".to_string(), "/nested.png".to_string()]
    );
}

/// An `<img>` with no `src` attribute must not queue a load at all.
#[test]
fn subtree_img_without_src_does_not_queue() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval("document.getElementById('main').innerHTML = '<img alt=\"no src\">';")
        .unwrap();
    assert!(hook.queued.lock().unwrap().is_empty());
}

/// BUG-1048: a `new Image()` that is never inserted is invisible to the shell's
/// DOM walk, so the hook must name the node itself — otherwise its `load`/
/// `error` has nowhere to go and `img.complete` stays `false` forever.
#[test]
fn detached_image_src_queues_with_its_own_nid() {
    let (rt, hook) = runtime_with_hook(make_doc());
    let nid = rt.eval("var i = new Image(); i.src = '/pre.png'; i.__nid__").unwrap();
    let lumen_core::JsValue::Number(nid) = nid else { panic!("__nid__ is not a number: {nid:?}") };
    assert_eq!(hook.queued.lock().unwrap().clone(), vec!["/pre.png".to_string()]);
    assert_eq!(hook.nids.lock().unwrap().clone(), vec![nid as u32]);
}
