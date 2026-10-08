//! BUG-1148 — `srcset`/`sizes` assignment and `<picture><source>` mutation must
//! queue the image through [`lumen_core::ext::ImageLoadHook`] immediately, with
//! the URL the `<picture>`/`srcset` picker selects (not the literal `src`).

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::ext::ImageLoadHook;

#[derive(Default)]
struct RecordingHook {
    queued: Mutex<Vec<String>>,
}

impl ImageLoadHook for RecordingHook {
    fn queue_image_load(&self, _nid: u32, raw_src: &str) {
        self.queued.lock().unwrap_or_else(|e| e.into_inner()).push(raw_src.to_string());
    }
}

fn runtime_with_hook(doc: Arc<Mutex<Document>>) -> (V8JsRuntime, Arc<RecordingHook>) {
    let hook = Arc::new(RecordingHook::default());
    let rt = V8JsRuntime::new()
        .unwrap()
        .with_image_load_hook(Arc::clone(&hook) as Arc<dyn ImageLoadHook>);
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    (rt, hook)
}

#[test]
fn srcset_assignment_queues_picked_candidate() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval(
        "var i = document.createElement('img'); \
         i.srcset = '/only.png 1x'; \
         document.getElementById('main').appendChild(i);",
    )
    .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    assert!(queued.iter().all(|u| u == "/only.png"), "{queued:?}");
    assert!(!queued.is_empty());
}

#[test]
fn picture_source_srcset_queues_img_child() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval(
        "var p = document.createElement('picture'); \
         var s = document.createElement('source'); \
         var i = document.createElement('img'); \
         p.appendChild(s); p.appendChild(i); \
         document.getElementById('main').appendChild(p); \
         s.setAttribute('srcset', '/pic.png');",
    )
    .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    assert_eq!(queued.last().map(String::as_str), Some("/pic.png"), "{queued:?}");
}

#[test]
fn inner_html_srcset_only_img_queues() {
    let (rt, hook) = runtime_with_hook(make_doc());
    rt.eval("document.getElementById('main').innerHTML = '<img srcset=\"/a.png 1x, /b.png 2x\">';")
        .unwrap();
    let queued = hook.queued.lock().unwrap().clone();
    assert_eq!(queued, vec!["/a.png".to_string()]);
}
