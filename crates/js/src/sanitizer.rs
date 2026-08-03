//! Sanitizer API (W3C Sanitizer API §3)
//!
//! Phase 0 stub: `new Sanitizer(config)` creates a sanitizer,
//! `sanitizer.sanitizeFor(element, string)` removes <script> tags and event handlers,
//! `element.setHTML(html, {sanitizer})` sets innerHTML via sanitizer.

/// V8 port of the former rquickjs `install_sanitizer_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B3): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_sanitizer_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SANITIZER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SANITIZER_SHIM: &str = r#"
// Sanitizer API (Phase 0 stub)
// Simple sanitizer that removes <script> tags and event handler attributes

const DANGEROUS_ATTRS = new Set([
  'onload', 'onerror', 'onclick', 'ondblclick', 'onmousedown', 'onmouseup',
  'onmouseover', 'onmouseout', 'onmousemove', 'onmouseenter', 'onmouseleave',
  'onfocus', 'onblur', 'onchange', 'onsubmit', 'oninput', 'onkeydown',
  'onkeyup', 'onkeypress', 'onwheel', 'ondrag', 'ondrop', 'onpaste',
  'oncopy', 'oncut', 'oncontextmenu', 'ontouchstart', 'ontouchend',
  'ontouchcancel', 'ontouchmove',
]);

function removeScriptTags(html) {
  // Remove <script ...>...</script> (case-insensitive)
  return html.replace(/<script[^>]*>[\s\S]*?<\/script>/gi, '');
}

function removeEventHandlers(html) {
  // Remove event handler attributes
  let result = html;

  for (const attr of DANGEROUS_ATTRS) {
    // Match attribute in both " and ' quotes, handle complex values
    const patterns = [
      new RegExp(` ${attr}="[^"]*"`, 'g'),
      new RegExp(` ${attr}='[^']*'`, 'g'),
      new RegExp(` ${attr}=[^ >]*`, 'g'),
    ];

    for (const pattern of patterns) {
      result = result.replace(pattern, '');
    }
  }

  return result;
}

globalThis.Sanitizer = class {
  constructor(config) {
    // Phase 0: config is not used
    this.config = config || {};
  }

  sanitizeFor(element, htmlString) {
    // Validate arguments
    if (!element) {
      throw new TypeError('sanitizeFor: element argument is required');
    }
    if (typeof htmlString !== 'string') {
      throw new TypeError('sanitizeFor: html string argument must be a string');
    }

    // Sanitize by removing dangerous elements and attributes
    let sanitized = removeScriptTags(htmlString);
    sanitized = removeEventHandlers(sanitized);

    // Phase 0: Create a DocumentFragment by setting innerHTML on a temporary container
    // and returning its childNodes
    const temp = document.createElement('div');
    temp.innerHTML = sanitized;

    // Create a proper DocumentFragment
    const frag = document.createDocumentFragment();
    while (temp.firstChild) {
      frag.appendChild(temp.firstChild);
    }
    return frag;
  }
};

// Extend Element.prototype.setHTML
if (typeof Element !== 'undefined' && Element.prototype) {
  if (!Element.prototype.setHTML) {
    Element.prototype.setHTML = function(html, options) {
      options = options || {};
      const sanitizer = options.sanitizer;

      if (sanitizer) {
        const fragment = sanitizer.sanitizeFor(this, html);
        // Clear current content and append sanitized fragment
        this.innerHTML = '';
        this.appendChild(fragment);
      } else {
        // Direct innerHTML if no sanitizer
        this.innerHTML = html;
      }
    };
  }
}

if (typeof window !== 'undefined') {
  window.Sanitizer = globalThis.Sanitizer;
}
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn with_sanitizer(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        // `install_dom` already installs the Sanitizer shim via `install_v8!`
        // (v8_runtime.rs) — calling `install_sanitizer_bindings_v8` again would
        // re-declare the shim's top-level `const`s (not IIFE-wrapped) in the
        // same global scope and fail with "already been declared".
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, false)
            .unwrap();
        f(&rt);
    }

    #[test]
    fn sanitizer_class_exists() {
        with_sanitizer(|rt| {
            let ok = rt.eval("typeof Sanitizer === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizer_can_be_instantiated() {
        with_sanitizer(|rt| {
            let ok = rt.eval("typeof new Sanitizer() === 'object'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizer_has_sanitizefor_method() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval("const s = new Sanitizer(); typeof s.sanitizeFor === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizefor_removes_script_tags() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval(
                    "const s = new Sanitizer(); const div = document.createElement('div'); \
                     const frag = s.sanitizeFor(div, '<p>hello</p><script>alert(\"xss\")</script>'); \
                     const c = document.createElement('div'); c.appendChild(frag); \
                     !c.innerHTML.includes('script')",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizefor_removes_event_handlers() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval(
                    "const s = new Sanitizer(); const div = document.createElement('div'); \
                     const frag = s.sanitizeFor(div, '<button onclick=\"bad()\">click</button>'); \
                     const c = document.createElement('div'); c.appendChild(frag); \
                     !c.innerHTML.includes('onclick')",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizefor_throws_on_missing_element() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval(
                    "const s = new Sanitizer(); \
                     try { s.sanitizeFor(null, '<p>test</p>'); false } \
                     catch (e) { e instanceof TypeError }",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizefor_throws_on_non_string_html() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval(
                    "const s = new Sanitizer(); const div = document.createElement('div'); \
                     try { s.sanitizeFor(div, 123); false } \
                     catch (e) { e instanceof TypeError }",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn sanitizefor_returns_document_fragment() {
        with_sanitizer(|rt| {
            let ok = rt
                .eval(
                    "const s = new Sanitizer(); const div = document.createElement('div'); \
                     const result = s.sanitizeFor(div, '<p>test</p>'); \
                     typeof result === 'object'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
