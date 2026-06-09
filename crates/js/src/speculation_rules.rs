//! Speculation Rules API Phase 0 (W3C Speculation Rules §3).
//!
//! Exposes:
//! - `document.prerendering` — boolean, `false` in Phase 0 (no prerendering support).
//! - `prerenderingchange` event on document — fired (never in Phase 0).
//! - `document.getSpeculationRules()` — returns `[]` Phase 0.
//!
//! The `<script type="speculationrules">` JSON block is not consumed by the engine
//! in Phase 0 (no background prefetch/prerender). The shell may hook into the
//! `_lumen_deliver_speculation_rules(json)` native binding for Phase 1 to implement
//! resource hints.

use rquickjs::Ctx;

/// Install the Speculation Rules API stubs into the JS context.
///
/// Call after the DOM shim so that `document` and `EventTarget` are already defined.
pub fn install_speculation_rules_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SPECULATION_RULES_SHIM)?;
    Ok(())
}

const SPECULATION_RULES_SHIM: &str = r#"(function() {
  'use strict';

  // document.prerendering (Speculation Rules §2.3):
  // true if the current document is being prerendered (not yet activated).
  // Phase 0: always false — Lumen does not yet prerender pages.
  if (typeof document !== 'undefined') {
    if (!Object.getOwnPropertyDescriptor(document, 'prerendering')) {
      Object.defineProperty(document, 'prerendering', {
        configurable: true,
        enumerable: true,
        get: function() { return false; }
      });
    }

    // document.getSpeculationRules() → [] Phase 0.
    // Phase 1: return parsed speculation rules from <script type="speculationrules">.
    if (typeof document.getSpeculationRules !== 'function') {
      document.getSpeculationRules = function() { return []; };
    }

    // Expose onprerenderingchange as a document event handler (§2.3.4).
    if (!Object.getOwnPropertyDescriptor(document, 'onprerenderingchange')) {
      Object.defineProperty(document, 'onprerenderingchange', {
        configurable: true,
        enumerable: true,
        get: function() { return document._onprerenderingchange || null; },
        set: function(fn) { document._onprerenderingchange = typeof fn === 'function' ? fn : null; }
      });
    }
  }

  // _lumen_deliver_speculation_rules(rulesJson):
  // Shell Phase 1 hook — called after HTML parsing when a
  // <script type="speculationrules"> block is encountered.
  // Phase 0: no-op. Phase 1: parse JSON, schedule prefetch/prerender hints.
  if (typeof globalThis._lumen_deliver_speculation_rules === 'undefined') {
    globalThis._lumen_deliver_speculation_rules = function(_rulesJson) {
      // Phase 0 no-op
    };
  }

})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_prereqs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
            if (typeof document === 'undefined') {
                var document = { _listeners: {} };
                document.addEventListener = function(t, fn) {
                    (this._listeners[t] = this._listeners[t] || []).push(fn);
                };
            }
            "#,
        )
        .unwrap();
        install_speculation_rules_api(ctx).unwrap();
    }

    #[test]
    fn document_prerendering_is_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx.eval("document.prerendering === false").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn document_get_speculation_rules_returns_empty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof document.getSpeculationRules === 'function'
                      && Array.isArray(document.getSpeculationRules())
                      && document.getSpeculationRules().length === 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deliver_speculation_rules_is_noop() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof globalThis._lumen_deliver_speculation_rules === 'function'
                      && (globalThis._lumen_deliver_speculation_rules('{}'), true)
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn onprerenderingchange_property() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    document.onprerenderingchange === null
                      && (document.onprerenderingchange = function(){}, true)
                      && typeof document.onprerenderingchange === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
