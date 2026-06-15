/// Topics API stub (Privacy Sandbox Topics API).
///
/// Exposes `document.browsingTopics()` and `DeprecatedTopicsButton` as defined
/// by the Privacy Sandbox Topics API proposal.
///
/// Phase 0 scope (no real topic observation):
/// - `document.browsingTopics([options])` → `Promise<[]>` — empty array; no
///   topics stored or returned (observer isolation Phase 0).
/// - `document.browsingTopics({skipObservation: true})` → `Promise<[]>` —
///   same in Phase 0.
/// - `HTMLButtonElement` with `browsingtopics` attribute (deprecated form —
///   `<button browsingtopics>`) — attribute presence accessible via `hasAttribute`.
///   `DeprecatedTopicsButton` — global alias for `HTMLButtonElement` that exposes
///   a static `browsingTopics()` method returning `Promise<[]>`.
///
/// Phase 1: wire `_lumen_topics_get_topics` native hook to retrieve genuinely
/// observed topics from a privacy-preserving per-origin store.
use rquickjs::Ctx;

/// Install Topics API bindings into the JS context.
///
/// Must run after the DOM shim so that `document`, `Promise`, and
/// `HTMLButtonElement` are available.
pub fn install_topics_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(TOPICS_API_SHIM)?;
    Ok(())
}

const TOPICS_API_SHIM: &str = r#"
(function(global) {
  'use strict';

  // document.browsingTopics([options]) → Promise<TopicsEntry[]>
  // Phase 0: always resolves with an empty array.
  // Real spec: https://patcg-individual-drafts.github.io/topics/
  if (typeof global.document !== 'undefined') {
    global.document.browsingTopics = function browsingTopics(_options) {
      // Phase 1: call _lumen_topics_get_topics(skipObservation) native hook.
      return Promise.resolve([]);
    };
  }

  // DeprecatedTopicsButton — surrogate class for <button browsingtopics>.
  // The "browsingtopics" content attribute signals to the browser that clicking
  // this button should share topics with the surrounding context.
  // Phase 0: class is available; static browsingTopics() → Promise<[]>.
  function DeprecatedTopicsButton() {
    throw new TypeError('DeprecatedTopicsButton cannot be constructed directly. ' +
      'Use <button browsingtopics> in markup.');
  }
  DeprecatedTopicsButton.browsingTopics = function() {
    return Promise.resolve([]);
  };

  global.DeprecatedTopicsButton = DeprecatedTopicsButton;
})(typeof globalThis !== 'undefined' ? globalThis : this);
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

    fn with_topics_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal DOM shim: document object.
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                if (typeof globalThis.document === 'undefined') {
                  globalThis.document = {};
                }
                "#,
            )
            .unwrap();
            install_topics_api(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn browsing_topics_method_exists_on_document() {
        with_topics_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof document.browsingTopics === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn browsing_topics_returns_empty_array() {
        with_topics_api(|ctx| {
            ctx.eval::<(), _>(
                "var __result = null; document.browsingTopics().then(function(v) { __result = v; });",
            )
            .unwrap();
            loop {
                if !ctx.execute_pending_job() {
                    break;
                }
            }
            let ok: bool = ctx
                .eval("Array.isArray(__result) && __result.length === 0")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn browsing_topics_with_skip_observation_resolves_empty() {
        with_topics_api(|ctx| {
            ctx.eval::<(), _>(
                "var __r2 = null; document.browsingTopics({ skipObservation: true }).then(function(v) { __r2 = v; });",
            )
            .unwrap();
            loop {
                if !ctx.execute_pending_job() {
                    break;
                }
            }
            let ok: bool = ctx.eval("Array.isArray(__r2) && __r2.length === 0").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deprecated_topics_button_class_exists() {
        with_topics_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof DeprecatedTopicsButton === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deprecated_topics_button_static_method_returns_empty_array() {
        with_topics_api(|ctx| {
            ctx.eval::<(), _>(
                "var __dtb = null; DeprecatedTopicsButton.browsingTopics().then(function(v) { __dtb = v; });",
            )
            .unwrap();
            loop {
                if !ctx.execute_pending_job() {
                    break;
                }
            }
            let ok: bool = ctx
                .eval("Array.isArray(__dtb) && __dtb.length === 0")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn deprecated_topics_button_constructor_throws() {
        with_topics_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var threw = false;
                    try { new DeprecatedTopicsButton(); } catch(e) { threw = e instanceof TypeError; }
                    threw
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
