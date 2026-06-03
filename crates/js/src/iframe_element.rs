//! HTMLIFrameElement JS stubs (Phase 0).
//!
//! Installs `HTMLIFrameElement`-compatible properties and methods on `<iframe>`
//! DOM elements so that pages can interact with them without JS errors.
//!
//! Phase 0 scope — no sub-document navigation:
//! - `src` getter/setter (reflects `src` attribute)
//! - `name` getter/setter (reflects `name` attribute)
//! - `srcdoc` getter/setter (reflects `srcdoc` attribute)
//! - `width` getter/setter (reflects `width` attribute)
//! - `height` getter/setter (reflects `height` attribute)
//! - `sandbox` getter/setter (reflects `sandbox` attribute)
//! - `allow` getter/setter (reflects `allow` attribute)
//! - `referrerPolicy` getter/setter (reflects `referrerpolicy` attribute)
//! - `loading` getter/setter (reflects `loading` attribute)
//! - `contentDocument` getter → `null` (no sub-document in Phase 0)
//! - `contentWindow` getter → `null` (no sub-document in Phase 0)

use rquickjs::Ctx;

/// Install HTMLIFrameElement stubs into the JS context.
///
/// Patches existing `<iframe>` elements and intercepts `document.createElement('iframe')`
/// so that pages can read/write iframe properties without throwing.
///
/// `contentDocument` and `contentWindow` always return `null` (Phase 0 — no
/// nested document navigation). This matches spec behaviour for cross-origin iframes.
///
/// Must be called **after** `dom::install_dom_api`.
pub fn install_iframe_element_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(IFRAME_ELEMENT_SHIM)?;
    Ok(())
}

/// JavaScript shim: HTMLIFrameElement stub properties.
const IFRAME_ELEMENT_SHIM: &str = r#"(function() {
  'use strict';

  function patchIframeElement(el) {
    if (el.__lumen_iframe_patched) return;
    el.__lumen_iframe_patched = true;

    // Reflect string attributes — getter reads attribute, setter updates it.
    function reflectAttr(prop, attr) {
      Object.defineProperty(el, prop, {
        get: function() {
          return (el.getAttribute && el.getAttribute(attr)) || '';
        },
        set: function(v) {
          if (el.setAttribute) el.setAttribute(attr, String(v == null ? '' : v));
        },
        configurable: true,
        enumerable: true,
      });
    }

    reflectAttr('src',            'src');
    reflectAttr('name',           'name');
    reflectAttr('srcdoc',         'srcdoc');
    reflectAttr('width',          'width');
    reflectAttr('height',         'height');
    reflectAttr('sandbox',        'sandbox');
    reflectAttr('allow',          'allow');
    reflectAttr('referrerPolicy', 'referrerpolicy');
    reflectAttr('loading',        'loading');

    // Phase 0: no sub-document. contentDocument and contentWindow are null.
    // Spec: cross-origin iframes may also expose null for security reasons.
    Object.defineProperty(el, 'contentDocument', {
      get: function() { return null; },
      configurable: true,
    });
    Object.defineProperty(el, 'contentWindow', {
      get: function() { return null; },
      configurable: true,
    });

    // getSVGDocument() → null (no sub-document).
    el.getSVGDocument = function() { return null; };
  }

  // Patch any <iframe> elements already in the document.
  if (typeof document !== 'undefined' && document.querySelectorAll) {
    try {
      var iframes = document.querySelectorAll('iframe');
      for (var i = 0; i < iframes.length; i++) {
        patchIframeElement(iframes[i]);
      }
    } catch(e) {}
  }

  // Intercept future document.createElement('iframe') calls.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag) {
      var el = _origCreate(tag);
      if (typeof tag === 'string' && tag.toLowerCase() === 'iframe') {
        patchIframeElement(el);
      }
      return el;
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

    /// Minimal DOM stubs for testing without the full DOM bridge.
    fn install_minimal_dom(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
var document = {
  querySelectorAll: function() { return []; },
  createElement: function(tag) {
    var attrs = {};
    return {
      getAttribute: function(k){ return attrs[k] || null; },
      setAttribute: function(k,v){ attrs[k]=v; },
      hasAttribute: function(k){ return k in attrs; },
      removeAttribute: function(k){ delete attrs[k]; },
      dispatchEvent: function(){}
    };
  }
};
"#,
        )
        .unwrap();
    }

    #[test]
    fn install_succeeds_without_document() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_iframe_element_bindings(&ctx)
                .expect("install should succeed without document");
        });
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx)
                .expect("install should succeed with minimal dom");
        });
    }

    #[test]
    fn src_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.src = 'https://example.com';
el.src === 'https://example.com'
"#,
                )
                .unwrap();
            assert!(result, "src getter/setter should reflect attribute");
        });
    }

    #[test]
    fn content_document_is_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.contentDocument === null
"#,
                )
                .unwrap();
            assert!(result, "contentDocument should be null in Phase 0");
        });
    }

    #[test]
    fn content_window_is_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.contentWindow === null
"#,
                )
                .unwrap();
            assert!(result, "contentWindow should be null in Phase 0");
        });
    }

    #[test]
    fn name_getter_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.name = 'myframe';
el.name === 'myframe'
"#,
                )
                .unwrap();
            assert!(result, "name getter/setter should reflect attribute");
        });
    }

    #[test]
    fn width_height_reflect_attributes() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.width = '600';
el.height = '400';
el.width === '600' && el.height === '400'
"#,
                )
                .unwrap();
            assert!(result, "width/height should reflect attributes");
        });
    }

    #[test]
    fn sandbox_reflects_attribute() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.sandbox = 'allow-scripts allow-same-origin';
el.sandbox === 'allow-scripts allow-same-origin'
"#,
                )
                .unwrap();
            assert!(result, "sandbox should reflect attribute");
        });
    }

    #[test]
    fn get_svg_document_returns_null() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.getSVGDocument() === null
"#,
                )
                .unwrap();
            assert!(result, "getSVGDocument() should return null in Phase 0");
        });
    }

    #[test]
    fn src_default_is_empty_string() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_minimal_dom(&ctx);
            install_iframe_element_bindings(&ctx).unwrap();
            let result: bool = ctx
                .eval(
                    r#"
var el = document.createElement('iframe');
el.src === ''
"#,
                )
                .unwrap();
            assert!(result, "src should default to empty string");
        });
    }
}
