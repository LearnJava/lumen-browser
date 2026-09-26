//! HTML Sanitizer API (WICG sanitizer-api, now part of WHATWG HTML).
//!
//! BUG-663: the config-object `Sanitizer` (`get`/`allowElement`/`removeElement`/
//! `replaceElementWithChildren`/`allow|removeAttribute`/`allow|removeProcessingInstruction`/
//! `setComments`/`setDataAttributes`/`removeUnsafe`), the sanitize walk, the safe and
//! unsafe `setHTML*` pair on `Element` and `ShadowRoot`, and the `Document.parseHTML*`
//! statics. The JS lives in `shim/sanitizer_shim.js`; the previous pre-redesign
//! draft (`sanitizeFor()` plus a regex strip of `<script>`/`on*`) is gone.

/// Evaluates the Sanitizer shim. Runs after `dom_parser` (alphabetical
/// `install_v8!` order in `v8_runtime.rs`), whose `Document.parseHTMLUnsafe`
/// the shim wraps.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_sanitizer_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SANITIZER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SANITIZER_SHIM: &str = include_str!("shim/sanitizer_shim.js");

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    /// `install_dom` already evaluates the shim via `install_v8!`.
    fn eval_in_page(script: &str) -> JsValue {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt.eval(script).unwrap()
    }

    fn assert_str(script: &str, expected: &str) {
        assert_eq!(eval_in_page(script), JsValue::String(expected.to_string()), "{script}");
    }

    #[test]
    fn get_returns_canonical_sorted_config() {
        assert_str(
            "JSON.stringify(new Sanitizer({elements: ['p', 'b'], attributes: ['id']}).get())",
            r#"{"attributes":[{"name":"id","namespace":null}],"comments":true,"dataAttributes":true,"elements":[{"name":"b","namespace":"http://www.w3.org/1999/xhtml","removeAttributes":[]},{"name":"p","namespace":"http://www.w3.org/1999/xhtml","removeAttributes":[]}],"removeProcessingInstructions":[]}"#,
        );
    }

    #[test]
    fn modifiers_report_whether_the_config_changed() {
        assert_str(
            "var s = new Sanitizer({elements: ['div']}); \
             [s.allowElement('p'), s.allowElement('p'), s.removeElement('div'), \
              s.removeElement('div'), s.replaceElementWithChildren('b'), \
              s.replaceElementWithChildren('html')].join()",
            "true,false,true,false,true,false",
        );
    }

    #[test]
    fn invalid_config_throws_type_error() {
        assert_eq!(
            eval_in_page("try { new Sanitizer({elements: [], removeElements: []}); false } catch (e) { e instanceof TypeError }"),
            JsValue::Bool(true),
        );
    }

    #[test]
    fn set_html_default_config_strips_script_and_handlers() {
        assert_str(
            "var d = document.createElement('div'); \
             d.setHTML('<p onclick=\"x()\" id=a>hi</p><script>bad()</script><custom-x>t</custom-x>'); \
             d.innerHTML",
            "<p>hi</p>",
        );
    }

    #[test]
    fn set_html_accepts_a_plain_config_object() {
        assert_str(
            "var d = document.createElement('div'); \
             d.setHTML('<div><p>Hello <b>World!</b></p></div>', {sanitizer: {elements: ['div', 'p']}}); \
             d.innerHTML",
            "<div><p>Hello </p></div>",
        );
    }

    #[test]
    fn set_html_unsafe_applies_a_sanitizer_but_keeps_script() {
        assert_str(
            "var d = document.createElement('div'); \
             d.setHTMLUnsafe('<p>a<b>b</b></p><script>x</script>', {sanitizer: {replaceWithChildrenElements: ['b']}}); \
             d.innerHTML",
            "<p>ab</p><script>x</script>",
        );
    }

    #[test]
    fn set_html_drops_javascript_urls_only_in_safe_mode() {
        assert_str(
            "var cfg = {sanitizer: {elements: [{name: 'a', attributes: ['href']}]}}; \
             var a = document.createElement('div'); a.setHTML('<a href=\"javascript:x()\">l</a>', cfg); \
             var b = document.createElement('div'); b.setHTMLUnsafe('<a href=\"javascript:x()\">l</a>', cfg); \
             a.innerHTML + '|' + b.innerHTML",
            "<a>l</a>|<a href=\"javascript:x()\">l</a>",
        );
    }

    #[test]
    fn set_html_on_script_context_is_a_no_op() {
        assert_str(
            "var s = document.createElement('script'); s.setHTML('abc'); \
             var u = document.createElement('script'); u.setHTMLUnsafe('abc'); \
             s.innerHTML + '|' + u.innerHTML",
            "|abc",
        );
    }

    #[test]
    fn shadow_root_has_set_html_pair() {
        assert_str(
            "var host = document.createElement('div'); var sr = host.attachShadow({mode: 'open'}); \
             sr.setHTML('<em>x</em><script>y</script>'); var a = sr.innerHTML; \
             sr.setHTMLUnsafe('<i>z</i>', {sanitizer: {removeElements: ['i']}}); \
             a + '|' + sr.innerHTML",
            "<em>x</em>|",
        );
    }

    #[test]
    fn document_parse_html_sanitizes_and_parse_html_unsafe_does_not() {
        assert_str(
            "var safe = Document.parseHTML('<p>t</p><script>x</script>'); \
             var unsafe = Document.parseHTMLUnsafe('<p>t</p><script>x</script>'); \
             safe.body.getElementsByTagName('script').length + ',' + \
             unsafe.body.getElementsByTagName('script').length + ',' + \
             safe.body.getElementsByTagName('p').length",
            "0,1,1",
        );
    }
}
