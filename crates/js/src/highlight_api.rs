/// CSS Custom Highlight API (CSS Highlight API L1, https://drafts.csswg.org/css-highlight-api-1/)
///
/// Provides `CSS.highlights` registry for custom text highlights with
/// `Highlight` objects containing ranges and priority, styled via `::highlight(name)` pseudo-element.

use rquickjs::Ctx;

/// Install CSS Highlight API bindings into the JavaScript context.
///
/// Exposes `CSS.highlights` `HighlightRegistry` with `set/get/has/delete/clear` methods,
/// `Highlight` constructor for creating highlight ranges with priority.
/// Phase 0: registry and storage; visual styling via `::highlight()` in Phase 1 (render integration).
pub fn install_highlight_api_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(HIGHLIGHT_SHIM)?;
    Ok(())
}

/// JavaScript shim: Install CSS.highlights registry and Highlight class.
const HIGHLIGHT_SHIM: &str = r#"(function() {
    // Highlight class: extends Set, stores ranges and priority
    class Highlight extends Set {
        constructor(...ranges) {
            super(ranges);
            this.priority = 0;  // default priority
        }
    }

    // HighlightRegistry: extends Map, provides named highlight storage
    class HighlightRegistry extends Map {
        // Inherits set/get/has/delete/clear/entries/keys/values from Map
    }

    // Ensure CSS object exists
    if (typeof globalThis.CSS === 'undefined') {
        globalThis.CSS = {};
    }

    // Install CSS.highlights registry and Highlight constructor
    globalThis.CSS.highlights = new HighlightRegistry();
    globalThis.CSS.Highlight = Highlight;
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

    #[test]
    fn highlight_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: String = ctx.eval("typeof CSS.Highlight === 'function' ? 'true' : 'false'").unwrap();
            assert_eq!(result, "true");
        });
    }

    #[test]
    fn highlight_registry_is_map() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "CSS.highlights instanceof Map && CSS.highlights.constructor.name === 'HighlightRegistry'"
            ).unwrap();
            assert_eq!(result, true);
        });
    }

    #[test]
    fn highlight_constructor_with_ranges() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "const h = new CSS.Highlight(); h.add('range1'); h.size === 1"
            ).unwrap();
            assert_eq!(result, true);
        });
    }

    #[test]
    fn highlight_priority_default() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: i32 = ctx.eval(
                "const h = new CSS.Highlight(); h.priority"
            ).unwrap();
            assert_eq!(result, 0);
        });
    }

    #[test]
    fn highlight_priority_settable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: i32 = ctx.eval(
                "const h = new CSS.Highlight(); h.priority = 5; h.priority"
            ).unwrap();
            assert_eq!(result, 5);
        });
    }

    #[test]
    fn registry_set_get() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "const h = new CSS.Highlight('search-match'); \
                 CSS.highlights.set('search', h); \
                 CSS.highlights.get('search') === h"
            ).unwrap();
            assert_eq!(result, true);
        });
    }

    #[test]
    fn registry_has() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "const h = new CSS.Highlight(); \
                 CSS.highlights.set('test', h); \
                 CSS.highlights.has('test')"
            ).unwrap();
            assert_eq!(result, true);
        });
    }

    #[test]
    fn registry_delete() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "const h = new CSS.Highlight(); \
                 CSS.highlights.set('test', h); \
                 CSS.highlights.delete('test'); \
                 !CSS.highlights.has('test')"
            ).unwrap();
            assert_eq!(result, true);
        });
    }

    #[test]
    fn registry_clear() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_highlight_api_bindings(&ctx).unwrap();
            let result: bool = ctx.eval(
                "CSS.highlights.set('a', new CSS.Highlight()); \
                 CSS.highlights.set('b', new CSS.Highlight()); \
                 CSS.highlights.clear(); \
                 CSS.highlights.size === 0"
            ).unwrap();
            assert_eq!(result, true);
        });
    }
}
