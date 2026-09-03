//! `new CSSStyleSheet()` / `.replaceSync()`/`.replace()` /
//! `document.adoptedStyleSheets` (CSSOM-5 срез 1, BUG-897) — the "write half"
//! of CSSOM-1's read-only stylesheet registry (`stylesheets.rs`).
//!
//! A constructed sheet has no owning DOM node (`<style>`/`<link>`), so it
//! cannot share `stylesheet_nodes`' index space (that registry is rebuilt
//! from document order — see `crates/shell/src/stylesheets.rs`). It lives in
//! its own registry instead, indexed by construction order; the JS wrapper
//! addresses it via a hidden own property (`_lumenSheetIdx`, see
//! `web_api_shim_mid.js`'s `_lumen_make_constructed_style_sheet`) rather than
//! by document position.
//!
//! `adoptedStyleSheets` (per `document` and per shadow root) is a second,
//! separate registry: an ordered list of constructed-sheet indices keyed by
//! an opaque "scope" `u32` the JS side chooses — `document.
//! adoptedStyleSheets` uses the sentinel `_LUMEN_ADOPTED_DOCUMENT_SCOPE`
//! (`u32::MAX`, `web_api_shim_mid.js`), a shadow root uses its own node id.
//! This module never needs to tell the two apart — it just stores whatever
//! key it is given — so the sentinel exists only on the JS side. Keying by
//! node id (rather than a JS expando on the shadow-root wrapper) is what
//! survives `_lumen_make_shadow_root` rebuilding a fresh wrapper object on
//! every `host.shadowRoot` read (BUG-877) — for this one property only.
//!
//! **Neither registry feeds the layout cascade yet.** The cascade is built
//! exclusively from the DOM tree's own `<style>`/`<link>` text
//! (`crates/shell/src/relayout.rs::refresh_dynamic_css`,
//! `crates/shell/src/page_pipeline.rs::build_page_cascade`) — there is no
//! JS-runtime-to-shell channel a constructed sheet's content or an adopted
//! list could travel over, and building one has to cross the ADR-016 engine
//! thread carefully (a synchronous cross-thread call from the wrong place
//! silently hung the process once already, BUG-976). Assigning
//! `adoptedStyleSheets` or calling `replaceSync` today changes what CSSOM
//! reports back but not what is painted. Follow-up, not this срез.

use super::reg;
#[allow(unused_imports)]
use super::super::*;
use super::stylesheets::{media_rule_json, style_rule_json};
use lumen_css_parser::CssomRuleRef;

/// One `new CSSStyleSheet()` instance (CSSOM §2.1). Indexed by position in
/// [`ConstructedStylesheets`]; that index is the JS wrapper's identity.
#[derive(Debug, Clone)]
pub(crate) struct ConstructedStylesheet {
    /// Last `replaceSync`/`replace` text, parsed. Empty stylesheet until the
    /// first call, per spec (`new CSSStyleSheet()` starts with `cssRules.
    /// length === 0`).
    pub(crate) sheet: Arc<lumen_css_parser::Stylesheet>,
    pub(crate) disabled: bool,
}

impl ConstructedStylesheet {
    fn empty() -> Self {
        Self { sheet: Arc::new(lumen_css_parser::parse("")), disabled: false }
    }
}

/// Registry of constructed stylesheets, shared with
/// [`install_constructed_stylesheets`]'s natives.
pub(crate) type ConstructedStylesheets = Arc<Mutex<Vec<ConstructedStylesheet>>>;

/// `scope id -> ordered list of constructed-sheet indices` — an opaque key
/// the JS side chooses; see the module doc comment.
pub(crate) type AdoptedStylesheets = Arc<Mutex<HashMap<u32, Vec<u32>>>>;

/// `new CSSStyleSheet()`, `.replaceSync()`/`.replace()`, `.cssRules`,
/// `document.adoptedStyleSheets`/`shadowRoot.adoptedStyleSheets` (CSSOM-5
/// срез 1, BUG-897). See the module doc comment for what is and is not wired
/// up yet.
pub(crate) fn install_constructed_stylesheets(
    scope: &mut v8::PinScope<'_, '_>,
    ctx: v8::Local<'_, v8::Context>,
    store: &mut Vec<OwnedNativeFn>,
    sheets: ConstructedStylesheets,
    adopted: AdoptedStylesheets,
) -> JsResult<()> {
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_construct_stylesheet", move || -> u32 {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            guard.push(ConstructedStylesheet::empty());
            (guard.len() - 1) as u32
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_replace_sync", move |idx: u32, text: String| {
            if let Some(entry) = s.lock().unwrap_or_else(|e| e.into_inner()).get_mut(idx as usize) {
                entry.sheet = Arc::new(lumen_css_parser::parse(&text));
            }
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_disabled", move |idx: u32| -> bool {
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(idx as usize)
                .map(|e| e.disabled)
                .unwrap_or(false)
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_set_disabled", move |idx: u32, val: bool| {
            if let Some(entry) = s.lock().unwrap_or_else(|e| e.into_inner()).get_mut(idx as usize) {
                entry.disabled = val;
            }
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_rule_count", move |idx: u32| -> u32 {
            s.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(idx as usize)
                .map(|e| e.sheet.cssom_rules().len() as u32)
                .unwrap_or(0)
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_rule_json", move |idx: u32, rule_idx: u32| -> Option<String> {
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
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_media_child_count", move |idx: u32, rule_idx: u32| -> u32 {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.get(idx as usize) else { return 0 };
            let rules = entry.sheet.cssom_rules();
            match rules.get(rule_idx as usize) {
                Some(CssomRuleRef::Media(r)) => r.rules.len() as u32,
                _ => 0,
            }
        });
    }
    {
        let s = Arc::clone(&sheets);
        reg!(scope, ctx, store, "_lumen_constructed_media_child_json", move |idx: u32, rule_idx: u32, child_idx: u32| -> Option<String> {
            let guard = s.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.get(idx as usize)?;
            let rules = entry.sheet.cssom_rules();
            match rules.get(rule_idx as usize)? {
                CssomRuleRef::Media(r) => Some(style_rule_json(r.rules.get(child_idx as usize)?).to_string()),
                CssomRuleRef::Style(_) => None,
            }
        });
    }
    // `document.adoptedStyleSheets`/`shadowRoot.adoptedStyleSheets` (CSSOM
    // §4.6): an ordered list of constructed-sheet indices, keyed by scope.
    // The JS side (`_lumen_adopted_sheet_ids`) has already checked every
    // element is a `CSSStyleSheet` built by `_lumen_construct_stylesheet`
    // before calling this — an out-of-range index here (only reachable if
    // the registry was reset by a navigation between validation and this
    // call) is silently dropped rather than rejected wholesale.
    {
        let sheets_for_set = Arc::clone(&sheets);
        let a = Arc::clone(&adopted);
        reg!(scope, ctx, store, "_lumen_set_adopted_stylesheets", move |scope_id: u32, ids: Vec<u32>| {
            let len = sheets_for_set.lock().unwrap_or_else(|e| e.into_inner()).len();
            let valid: Vec<u32> = ids.into_iter().filter(|&id| (id as usize) < len).collect();
            a.lock().unwrap_or_else(|e| e.into_inner()).insert(scope_id, valid);
        });
    }
    {
        let a = Arc::clone(&adopted);
        reg!(scope, ctx, store, "_lumen_get_adopted_stylesheets", move |scope_id: u32| -> Vec<u32> {
            a.lock().unwrap_or_else(|e| e.into_inner()).get(&scope_id).cloned().unwrap_or_default()
        });
    }
    Ok(())
}
