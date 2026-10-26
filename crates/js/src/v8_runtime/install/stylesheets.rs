//! `document.styleSheets` / `<style>`/`<link>.sheet` / `CSSStyleSheet.cssRules`
//! (CSSOM-1 срез 3, read-only, plus `insertRule`/`deleteRule` — BUG-518 срез
//! 7) — natives over the per-node stylesheet registry
//! `V8JsRuntime::stylesheet_nodes`. See
//! `docs/tasks/p1-cssom-1-stylesheets.md` for the architecture this sits on
//! top of (why the registry lives in `crates/shell`/`lumen-css-parser`
//! instead of `Document`) and for the JS-side wiring (`web_api_shim_mid.js`'s
//! `_lumen_make_css_style_sheet`/`_lumen_make_css_rule` and
//! `web_api_shim_tail_b.js`'s `<style>`/`<link>.sheet` getters).

use super::reg;
#[allow(unused_imports)]
use super::super::*;
use lumen_css_parser::MixinResultNodeInfo;

/// One node inside a top-level `@mixin`'s `@result` tree (CSSOM-8, вложенные
/// правила) as a JSON object — the shape
/// `_lumen_make_mixin_result_node`/`_lumen_style_get_parsed` (JS) parse to
/// build the wrapper / read `.style`'s current text.
fn mixin_result_node_json(info: &MixinResultNodeInfo) -> serde_json::Value {
    serde_json::json!({
        "kind": info.kind,
        "cssText": info.css_text,
        "styleCssText": info.style_css_text,
        "childCount": info.child_count,
    })
}

/// Media context for the conditions of a `@keyframes` declared inside
/// `@media`/`@supports` (BUG-1686): the page viewport, screen media, default
/// preferences (the JS-side `matchMedia` preference flags are not mirrored here).
fn keyframes_media_context(viewport_size: &Mutex<[f32; 2]>) -> lumen_css_parser::MediaContext {
    let [width, height] = *viewport_size.lock().unwrap_or_else(|e| e.into_inner());
    lumen_css_parser::MediaContext { width, height, ..Default::default() }
}

/// `document.styleSheets`, `<style>`/`<link>.sheet`, `CSSStyleSheet.cssRules`,
/// `CSSStyleRule.selectorText`/`style.cssText`, `CSSMediaRule.media.mediaText`
/// (CSSOM-1 срез 3, read-only) over `stylesheet_nodes`.
pub(crate) fn install_stylesheets(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    sync: super::super::sheet_sync::SheetSync,
    viewport_size: Arc<Mutex<[f32; 2]>>,
) -> JsResult<()> {
    let stylesheet_nodes = Arc::clone(&sync.nodes);
    // Owner node ids in document order — the JS side's "sheet index" is this
    // array's index, addressed fresh on every call rather than cached, so a
    // registry rebuild (script touched `<style>`/`<link>`, BUG-443 gate) is
    // visible without rebuilding any JS wrapper.
    //
    // BUG-493: every read of the list first reconciles the registry with the
    // DOM (`SheetSync::sync`, a no-op while the DOM mutation epoch is
    // unchanged), so a `<style>` a script inserted after load is listed.
    {
        let sync = sync.clone();
        reg!(scope, ctx, store, "_lumen_stylesheet_owner_nids", move || -> Vec<u32> {
            sync.sync();
            sync.nodes.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.node).collect()
        });
    }
    // BUG-493: registry indices of the sheets that belong to the document
    // itself (`document.styleSheets`) — shadow-tree sheets belong to their
    // own `ShadowRoot.styleSheets`.
    {
        let sync = sync.clone();
        reg!(scope, ctx, store, "_lumen_stylesheet_document_indices", move || -> Vec<u32> {
            sync.sync();
            let shadow = sync.shadow_owned.lock().unwrap_or_else(|e| e.into_inner()).clone();
            sync.nodes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .enumerate()
                .filter(|(_, e)| !shadow.contains(&e.node))
                .map(|(i, _)| i as u32)
                .collect()
        });
    }
    // BUG-493: `<style>`/`<link>.sheet` — registry index of `nid`'s own sheet,
    // `-1` for none. See `SheetSync::index_for_owner` for the fast path.
    {
        let sync = sync.clone();
        reg!(scope, ctx, store, "_lumen_stylesheet_index_for", move |nid: u32| -> i32 {
            sync.index_for_owner(nid)
        });
    }
    // CSS Animations L1 §3 — the winning `@keyframes <name>` of the document's
    // sheets (a later sheet / a later rule of one sheet overrides an earlier one)
    // as `[{"offset": f, "decls": [[property, value], ...]}, ...]`, `None` when no
    // enabled sheet defines it. Read by the JS shim to drive `*-rule-*` animations
    // (`_wa_gap_an_value`), the Rust `AnimationScheduler` knowing only five properties.
    {
        let sync = sync.clone();
        let vs = Arc::clone(&viewport_size);
        reg!(scope, ctx, store, "_lumen_keyframes_json", move |name: String| -> Option<String> {
            sync.sync();
            let media = keyframes_media_context(&vs);
            let guard = sync.nodes.lock().unwrap_or_else(|e| e.into_inner());
            let rule = guard
                .iter()
                .filter(|e| !e.disabled)
                .flat_map(|e| e.sheet.keyframes.iter())
                .filter(|k| lumen_css_parser::conditions_hold(&k.conditions, &media))
                .rfind(|k| k.name == name)?;
            let frames: Vec<serde_json::Value> = rule
                .frames
                .iter()
                .map(|f| {
                    let decls: Vec<serde_json::Value> = f
                        .declarations
                        .iter()
                        .map(|d| serde_json::json!([d.property.to_ascii_lowercase(), d.value]))
                        .collect();
                    serde_json::json!({ "offset": f.offset, "decls": decls })
                })
                .collect();
            Some(serde_json::Value::Array(frames).to_string())
        });
    }
    // Whether any enabled sheet defines a `@keyframes` rule. `document.getAnimations()` walks
    // every element for CSS animations only when this is true.
    {
        let sync = sync.clone();
        let vs = Arc::clone(&viewport_size);
        reg!(scope, ctx, store, "_lumen_has_keyframes", move || -> bool {
            sync.sync();
            let media = keyframes_media_context(&vs);
            let guard = sync.nodes.lock().unwrap_or_else(|e| e.into_inner());
            guard.iter().filter(|e| !e.disabled).any(|e| {
                e.sheet.keyframes.iter().any(|k| lumen_css_parser::conditions_hold(&k.conditions, &media))
            })
        });
    }
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_disabled", move |idx: u32| -> bool {
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(idx as usize)
                .map(|e| e.disabled)
                .unwrap_or(false)
        });
    }
    // CSSOM-8, вложенные правила — nested-rule addressing into a top-level
    // `@mixin`'s `@result` tree (`mixin-invalidation.tentative.html`'s first
    // two subtests). `path` is a sequence of `.cssRules` indices, empty
    // meaning "`@result` itself" (see `Stylesheet::mixin_result_child_count`'s
    // doc comment) — JS builds it up one hop at a time
    // (`_lumen_make_mixin_result_node`), so every native here takes it as a
    // plain `Vec<u32>` rather than a JSON-encoded path.
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_mixin_has_result", move |idx: u32, rule_idx: u32| -> bool {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(idx as usize).is_some_and(|e| e.sheet.mixin_has_result(rule_idx as usize))
        });
    }
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_mixin_node_count", move |idx: u32, rule_idx: u32, path: Vec<u32>| -> u32 {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get(idx as usize) else { return 0 };
            let path: Vec<usize> = path.into_iter().map(|p| p as usize).collect();
            entry.sheet.mixin_result_child_count(rule_idx as usize, &path).unwrap_or(0) as u32
        });
    }
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_mixin_node_json", move |idx: u32, rule_idx: u32, path: Vec<u32>| -> Option<String> {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.get(idx as usize)?;
            let path: Vec<usize> = path.into_iter().map(|p| p as usize).collect();
            let info = entry.sheet.mixin_result_node_info(rule_idx as usize, &path)?;
            Some(mixin_result_node_json(&info).to_string())
        });
    }
    {
        let s = Arc::clone(&stylesheet_nodes);
        let sync = sync.clone();
        reg!(scope, ctx, store, "_lumen_stylesheet_mixin_set_node_style", move |idx: u32, rule_idx: u32, path: Vec<u32>, css_text: String| -> bool {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get_mut(idx as usize) else { return false };
            let node = entry.node;
            let path: Vec<usize> = path.into_iter().map(|p| p as usize).collect();
            let ok = std::sync::Arc::make_mut(&mut entry.sheet)
                .set_mixin_result_style(rule_idx as usize, &path, &css_text)
                .is_ok();
            if ok {
                let sheet = std::sync::Arc::make_mut(&mut entry.sheet);
                if let Some(tree_id) = sheet.mixin_tree_id(rule_idx as usize) {
                    sheet.sync_mixin_node(tree_id);
                }
                sync.record(node, lumen_css_parser::CssomOp::TreeEdit);
            }
            ok
        });
    }
    Ok(())
}
