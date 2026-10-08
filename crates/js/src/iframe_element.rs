//! HTMLIFrameElement JS stubs.
//!
//! Installs `HTMLIFrameElement`-compatible properties and methods on `<iframe>`
//! DOM elements so that pages can interact with them without JS errors.
//!
//! `src`/`name`/`srcdoc`/`allow`/`referrerPolicy`/`loading` are reflected on
//! `HTMLIFrameElement.prototype` by `_lumen_install_reflection`
//! (`web_api_shim_tail_b.js`) — that table does URL resolution (`[ReflectURL]`)
//! and enum normalization that a plain own-property reflect here cannot
//! reproduce, so this module must NOT redefine those as own properties
//! (BUG-920: an own property always wins over the prototype accessor, so a
//! duplicate own-property definition silently shadows the correct one).
//!
//! Scope:
//! - `width` getter/setter (reflects `width` attribute; no prototype-level
//!   entry exists for it, so an own property is the only definition)
//! - `height` getter/setter (reflects `height` attribute; same as `width`)
//! - `sandbox` getter/setter (reflects `sandbox` attribute; same as `width`)
//! - `contentDocument` getter → фасад под-документа из [`crate::frame_bridge`]
//!   (BUG-480 срез 2); `null` для фрейма без загруженного под-документа,
//!   cross-origin и opaque-sandbox
//! - `contentWindow` getter → фасад окна из [`crate::frame_bridge`]; `null`
//!   только когда под-документ не загружен вовсе

/// V8 port of the former rquickjs `install_iframe_element_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Patches existing `<iframe>` elements and intercepts `document.createElement('iframe')`
/// so that pages can read/write iframe properties without throwing.
///
/// `contentDocument`/`contentWindow` делегируют в бридж под-документов
/// ([`crate::frame_bridge`], BUG-480 срез 2): реестр биндингов наполняет shell
/// после загрузки каждого фрейма, поэтому до регистрации (динамически созданный
/// фрейм, неудавшийся fetch) оба геттера дают `null`. `typeof`-guard держит шим
/// рабочим и без установленного бриджа (минимальные тестовые DOM).
///
/// Must be called **after** `v8_runtime.rs::install_dom`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_iframe_element_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(IFRAME_ELEMENT_SHIM)?;
    Ok(())
}

/// JavaScript shim: HTMLIFrameElement stub properties.
#[cfg(feature = "v8-backend")]
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

    // src/name/srcdoc/allow/referrerPolicy/loading are already reflected
    // correctly on HTMLIFrameElement.prototype (web_api_shim_tail_b.js) —
    // do NOT redefine them here, see module doc comment (BUG-920).
    reflectAttr('width',   'width');
    reflectAttr('height',  'height');
    reflectAttr('sandbox', 'sandbox');

    // BUG-480 срез 2: доступ к под-документу через бридж (frame_bridge.rs).
    // До регистрации биндинга (фрейм не загружен / cross-origin для
    // contentDocument) — null, как и до среза 2.
    Object.defineProperty(el, 'contentDocument', {
      get: function() {
        var nid = this.__nid__;
        return (typeof _lumen_frame_content_document === 'function' && nid !== undefined)
          ? _lumen_frame_content_document(nid)
          : null;
      },
      configurable: true,
    });
    Object.defineProperty(el, 'contentWindow', {
      get: function() {
        var nid = this.__nid__;
        return (typeof _lumen_frame_content_window === 'function' && nid !== undefined)
          ? _lumen_frame_content_window(nid)
          : null;
      },
      configurable: true,
    });

    // getSVGDocument() — вложенный документ, только если он SVG (OBJECT-1
    // срез 5, то же правило, что у <object>/<embed>).
    el.getSVGDocument = function() {
      var d = this.contentDocument;
      return (d && d.contentType === 'image/svg+xml') ? d : null;
    };
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

  // Intercept future document.createElement('iframe') calls. Forwards every
  // argument (not just `tag`) — GAP-CEREG срез 2 (BUG-890) added a second
  // `options` parameter (`{customElements: registry}`) to the native
  // `createElement`; an arity-1 wrapper here would silently drop it.
  if (typeof document !== 'undefined' && document.createElement) {
    var _origCreate = document.createElement.bind(document);
    document.createElement = function(tag, options) {
      var el = _origCreate(tag, options);
      if (typeof tag === 'string' && tag.toLowerCase() === 'iframe') {
        patchIframeElement(el);
      }
      return el;
    };
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // `expect()` в хелперах тестового модуля: исключение из clippy.toml
    // покрывает только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal DOM stubs for testing without the full DOM bridge.
    fn with_minimal_dom(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
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
        super::install_iframe_element_bindings_v8(&rt)
            .expect("install should succeed with minimal dom");
        f(&rt);
    }

    #[test]
    fn install_succeeds_without_document() {
        let rt = V8JsRuntime::new().unwrap();
        super::install_iframe_element_bindings_v8(&rt)
            .expect("install should succeed without document");
    }

    #[test]
    fn install_succeeds_with_minimal_dom() {
        with_minimal_dom(|_rt| {});
    }

    #[test]
    fn does_not_shadow_prototype_reflected_properties() {
        // BUG-920: src/name/srcdoc/allow/referrerPolicy/loading must be left to
        // `HTMLIFrameElement.prototype`'s `_lumen_install_reflection` table
        // (URL resolution, enum normalization) — patchIframeElement must not
        // define its own competing property for any of them.
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
['src', 'name', 'srcdoc', 'allow', 'referrerPolicy', 'loading'].every(function(p) {
    return Object.getOwnPropertyDescriptor(el, p) === undefined;
})
"#,
                )
                .unwrap();
            assert_eq!(
                result,
                JsValue::Bool(true),
                "patchIframeElement must not shadow prototype-reflected properties"
            );
        });
    }

    #[test]
    fn content_document_is_null() {
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
el.contentDocument === null
"#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "contentDocument should be null without a registered frame binding");
        });
    }

    #[test]
    fn content_window_is_null() {
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
el.contentWindow === null
"#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "contentWindow should be null without a registered frame binding");
        });
    }

    #[test]
    fn width_height_reflect_attributes() {
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
el.width = '600';
el.height = '400';
el.width === '600' && el.height === '400'
"#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "width/height should reflect attributes");
        });
    }

    #[test]
    fn sandbox_reflects_attribute() {
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
el.sandbox = 'allow-scripts allow-same-origin';
el.sandbox === 'allow-scripts allow-same-origin'
"#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "sandbox should reflect attribute");
        });
    }

    #[test]
    fn get_svg_document_returns_null() {
        with_minimal_dom(|rt| {
            let result = rt
                .eval(
                    r#"
var el = document.createElement('iframe');
el.getSVGDocument() === null
"#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true), "getSVGDocument() без под-документа — null");
        });
    }

}
