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
use lumen_css_parser::{CssomRuleRef, MediaRule, MixinRule, Rule};

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
        "cssText": r.css_text(),
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

/// A top-level `@mixin` rule (CSS Mixins L1 §cssom) as a JSON object —
/// read-only, `name`/`cssText` only (no `.cssRules` navigation into
/// `@result`'s own children — `mixin-cssom.tentative.html`'s non-`insertRule`
/// subtests only ever read the whole rule's `cssText`, never descend into
/// it). `cssText` wraps `MixinRule::css_text`, computed Rust-side rather
/// than reassembled in JS, since its indentation rules (`render_container`)
/// aren't expressible as a simple string join the way `style_rule_json`'s
/// flat declaration list is.
///
/// `pub(super)`: see [`style_rule_json`]'s doc comment — shared with
/// `constructed_stylesheets.rs`.
pub(super) fn mixin_rule_json(r: &MixinRule) -> serde_json::Value {
    serde_json::json!({
        "kind": "mixin",
        "name": r.name,
        "cssText": r.css_text(),
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
    cssom_deltas: super::super::style_flush::CssomDeltaLog,
    cssom_dirty: Arc<std::sync::atomic::AtomicBool>,
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
                CssomRuleRef::Mixin(r) => mixin_rule_json(r),
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
                CssomRuleRef::Style(_) | CssomRuleRef::Mixin(_) => None,
            }
        });
    }
    // `CSSStyleSheet.insertRule`/`.deleteRule` on an OWNED sheet (BUG-518
    // срез 7 — the last gap `constructed_stylesheets.rs`'s own doc comment
    // named: "`document.styleSheets`'s read-only sheets (CSSOM-1) still lack
    // both"). Same sentinel-return convention as the constructed-sheet twin
    // (`_lumen_constructed_insert_rule`/`_lumen_constructed_delete_rule`) —
    // the JS wrapper turns a negative result into the matching
    // `DOMException`. Mutates this entry's own `Arc<Stylesheet>` in place
    // (`Arc::make_mut`, copy-on-write since `cssom_rules()` readers elsewhere
    // hold a `&Stylesheet` only for the duration of one call, never a clone
    // of the `Arc`) — this write alone still does not reach the page cascade:
    // `stylesheet_nodes` is built by
    // `crates/shell/src/stylesheets.rs::build_stylesheet_node_registry`, whose
    // only caller is `build_page_cascade` (`page_pipeline.rs:464`, plus at
    // most one re-run at `:908`), while `cascade.sheet` is an INDEPENDENT
    // parse of one concatenated string (`imports_prefix` + every `<style>`'s
    // text + `linked`, `page_pipeline.rs:421-457`). CSSOM-8 вариант C (this
    // slice) closes that gap without writing anything back into either
    // string: every op below is ALSO appended to `cssom_deltas`
    // (`crates/js/src/v8_runtime/style_flush.rs::CssomDeltaLog`), addressed
    // in this node's own `cssRules` index space. `FlushHandles::maybe_flush`
    // replays the whole log onto a throwaway clone of the cascade sheet on
    // every same-tick flush (`FlushHandles::cssom_patched_sheet`), rebasing
    // each op's index via `Stylesheet::cssom_range_for_source_span` — this
    // node's own parsed text is a literal substring of the cascade's, found
    // by `Stylesheet::locate_embedded_source`. A steady-state (non-same-tick)
    // relayout still lays out against the pristine, unpatched cascade sheet
    // the shell last pushed — only `getComputedStyle`/geometry reads inside
    // the same script turn as the mutation see the patched one; the next full
    // relayout re-parses `<style>`/`<link>` text unchanged by any of this and
    // does not durably absorb the edit either, so a page that reads back
    // computed style only after a later user-driven relayout (not the same
    // tick) still would not see it — no vendored test in this bug's scope
    // needs that. `_lumen_stylesheet_rule_set_style`/`_lumen_stylesheet_
    // media_child_set_style` below record the same way. This does NOT flip
    // `mixin-invalidation.tentative.html`: none of that file's three subtests
    // addresses a top-level or `@media`-child rule — two mutate a rule inside
    // a `@mixin`'s `@result`, and the third calls `CSSStyleRule.insertRule`
    // (nested `@apply` into a style rule's body). All three need nested-rule
    // CSSOM addressing first, which this registry has no address for.
    {
        let s = Arc::clone(&stylesheet_nodes);
        let deltas = Arc::clone(&cssom_deltas);
        let dirty = Arc::clone(&cssom_dirty);
        reg!(scope, ctx, store, "_lumen_stylesheet_insert_rule", move |idx: u32, rule_text: String, index: u32| -> i32 {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get_mut(idx as usize) else { return -1 };
            let node = entry.node;
            match std::sync::Arc::make_mut(&mut entry.sheet).insert_rule(&rule_text, index as usize) {
                Ok(new_index) => {
                    // CSSOM-8 вариант C: recorded in the owning node's own
                    // `cssRules` index space — `replay_cssom_ops` rebases it
                    // onto the cascade sheet at flush time.
                    deltas.lock().unwrap_or_else(|e| e.into_inner()).push((
                        node,
                        lumen_css_parser::CssomOp::InsertRule { index: index as usize, text: rule_text },
                    ));
                    dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    new_index as i32
                }
                Err(lumen_css_parser::CssomRuleMutationError::IndexSize) => -1,
                Err(lumen_css_parser::CssomRuleMutationError::Syntax) => -2,
            }
        });
    }
    {
        let s = Arc::clone(&stylesheet_nodes);
        let deltas = Arc::clone(&cssom_deltas);
        let dirty = Arc::clone(&cssom_dirty);
        reg!(scope, ctx, store, "_lumen_stylesheet_delete_rule", move |idx: u32, index: u32| -> i32 {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get_mut(idx as usize) else { return -1 };
            let node = entry.node;
            match std::sync::Arc::make_mut(&mut entry.sheet).delete_rule(index as usize) {
                Ok(()) => {
                    deltas.lock().unwrap_or_else(|e| e.into_inner()).push((
                        node,
                        lumen_css_parser::CssomOp::DeleteRule { index: index as usize },
                    ));
                    dirty.store(true, std::sync::atomic::Ordering::Relaxed);
                    0
                }
                Err(_) => -1,
            }
        });
    }
    // `CSSStyleRule.style`'s write half (CSSOM-8, BUG-518 срез 9) — a
    // top-level style rule's own declaration block. Same "-1 sentinel on any
    // error, no distinct error kinds" convention as insert/delete above
    // (the JS wrapper never needs to tell `IndexSize` apart from `Syntax`
    // here, since it only ever calls this with an index it just read back
    // from `_lumen_stylesheet_rule_json`). Same not-yet-wired-to-the-cascade
    // caveat as `_lumen_stylesheet_insert_rule` above — see that native's
    // doc comment.
    {
        let s = Arc::clone(&stylesheet_nodes);
        let deltas = Arc::clone(&cssom_deltas);
        let dirty = Arc::clone(&cssom_dirty);
        reg!(scope, ctx, store, "_lumen_stylesheet_rule_set_style", move |idx: u32, rule_idx: u32, css_text: String| -> bool {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get_mut(idx as usize) else { return false };
            let node = entry.node;
            let ok = std::sync::Arc::make_mut(&mut entry.sheet)
                .set_rule_style_text(rule_idx as usize, &css_text)
                .is_ok();
            if ok {
                deltas.lock().unwrap_or_else(|e| e.into_inner()).push((
                    node,
                    lumen_css_parser::CssomOp::SetRuleStyle { index: rule_idx as usize, css_text },
                ));
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            ok
        });
    }
    // Sibling of the native above for a style rule nested inside a
    // top-level `@media` block (`_lumen_stylesheet_media_child_json`'s
    // addressing: `rule_idx` is the `@media` block's own top-level
    // position, `child_idx` its position inside that block).
    {
        let s = Arc::clone(&stylesheet_nodes);
        let deltas = Arc::clone(&cssom_deltas);
        let dirty = Arc::clone(&cssom_dirty);
        reg!(scope, ctx, store, "_lumen_stylesheet_media_child_set_style", move |idx: u32, rule_idx: u32, child_idx: u32, css_text: String| -> bool {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get_mut(idx as usize) else { return false };
            let node = entry.node;
            let ok = std::sync::Arc::make_mut(&mut entry.sheet)
                .set_media_child_style_text(rule_idx as usize, child_idx as usize, &css_text)
                .is_ok();
            if ok {
                deltas.lock().unwrap_or_else(|e| e.into_inner()).push((
                    node,
                    lumen_css_parser::CssomOp::SetMediaChildStyle {
                        media_index: rule_idx as usize,
                        child_index: child_idx as usize,
                        css_text,
                    },
                ));
                dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            ok
        });
    }
    Ok(())
}
