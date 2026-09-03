//! `document.styleSheets` / `<style>`/`<link>.sheet` / `CSSStyleSheet.cssRules`
//! (CSSOM-1 срез 3, read-only) — natives over the per-node stylesheet
//! registry `V8JsRuntime::stylesheet_nodes`. See
//! `docs/tasks/p1-cssom-1-stylesheets.md` for the architecture this sits on
//! top of (why the registry lives in `crates/shell`/`lumen-css-parser`
//! instead of `Document`) and for the JS-side wiring (`web_api_shim_mid.js`'s
//! `_lumen_make_css_style_sheet`/`_lumen_make_css_rule` and
//! `web_api_shim_tail_b.js`'s `<style>`/`<link>.sheet` getters).

use super::reg;
#[allow(unused_imports)]
use super::super::*;
use lumen_css_parser::{CssomRuleRef, MediaRule, Rule};

/// `CSSStyleRule.selectorText`/`style.cssText` as a JSON object — the shape
/// `_lumen_make_css_rule` (JS) parses to build the wrapper.
///
/// `pub(super)`: also used by `constructed_stylesheets.rs` (CSSOM-5) — a
/// constructed sheet's rules are the same [`Rule`]/[`MediaRule`] shape as an
/// owned one, just addressed through a different registry.
pub(super) fn style_rule_json(r: &Rule) -> serde_json::Value {
    serde_json::json!({
        "kind": "style",
        "selectorText": r.selector_text(),
        "styleCssText": r.style_css_text(),
    })
}

/// `CSSMediaRule.media.mediaText` as a JSON object — nested rules are read
/// through the separate `_lumen_stylesheet_media_child_*` natives below, not
/// embedded here, so this payload stays O(1) regardless of the rule's body.
///
/// `pub(super)`: see [`style_rule_json`]'s doc comment.
pub(super) fn media_rule_json(r: &MediaRule) -> serde_json::Value {
    serde_json::json!({
        "kind": "media",
        "mediaText": r.query.raw.trim(),
    })
}

/// `document.styleSheets`, `<style>`/`<link>.sheet`, `CSSStyleSheet.cssRules`,
/// `CSSStyleRule.selectorText`/`style.cssText`, `CSSMediaRule.media.mediaText`
/// (CSSOM-1 срез 3, read-only) over `stylesheet_nodes`.
pub(crate) fn install_stylesheets(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    stylesheet_nodes: Arc<Mutex<Vec<lumen_css_parser::StylesheetNodeEntry>>>,
) -> JsResult<()> {
    // Owner node ids in document order — the JS side's "sheet index" is this
    // array's index, addressed fresh on every call rather than cached, so a
    // registry rebuild (script touched `<style>`/`<link>`, BUG-443 gate) is
    // visible without rebuilding any JS wrapper.
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_owner_nids", move || -> Vec<u32> {
            s.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.node).collect()
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
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_rule_count", move |idx: u32| -> u32 {
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(idx as usize)
                .map(|e| e.sheet.cssom_rules().len() as u32)
                .unwrap_or(0)
        });
    }
    // One top-level rule (`document.styleSheets[idx].cssRules[rule_idx]`) as
    // JSON — `style_rule_json`/`media_rule_json` shape.
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_rule_json", move |idx: u32, rule_idx: u32| -> Option<String> {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.get(idx as usize)?;
            let rules = entry.sheet.cssom_rules();
            let json = match rules.get(rule_idx as usize)? {
                CssomRuleRef::Style(r) => style_rule_json(r),
                CssomRuleRef::Media(r) => media_rule_json(r),
            };
            Some(json.to_string())
        });
    }
    // Nested style-rule count inside a `@media` block — 0 for a non-media
    // rule or an out-of-range index, never an error (mirrors every other nid
    // native's "out of range answers empty/false" convention).
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_media_child_count", move |idx: u32, rule_idx: u32| -> u32 {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get(idx as usize) else { return 0 };
            let rules = entry.sheet.cssom_rules();
            match rules.get(rule_idx as usize) {
                Some(CssomRuleRef::Media(r)) => r.rules.len() as u32,
                _ => 0,
            }
        });
    }
    // A nested style rule inside a `@media` block, by (sheet, media-rule,
    // child) triple — always `CssomRuleRef::Style`-shaped JSON, `@media`
    // cannot nest another `@media` in this parser's model.
    {
        let s = Arc::clone(&stylesheet_nodes);
        reg!(scope, ctx, store, "_lumen_stylesheet_media_child_json", move |idx: u32, rule_idx: u32, child_idx: u32| -> Option<String> {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.get(idx as usize)?;
            let rules = entry.sheet.cssom_rules();
            match rules.get(rule_idx as usize)? {
                CssomRuleRef::Media(r) => Some(style_rule_json(r.rules.get(child_idx as usize)?).to_string()),
                CssomRuleRef::Style(_) => None,
            }
        });
    }
    Ok(())
}
