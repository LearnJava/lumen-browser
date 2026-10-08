//! URL Pattern API (WHATWG URLPattern §3).
//!
//! Pure JavaScript implementation of the URLPattern Standard: `new URLPattern(input, base, options)`,
//! `.test()`/`.exec()` (spec-shaped `URLPatternResult`), `hasRegExpGroups`, `generate()` and the
//! static `compareComponent()`.

/// V8 port of the former rquickjs `install_url_pattern_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `globalThis.URLPattern` (constructor, `test`, `exec`, `generate`, `hasRegExpGroups`,
/// per-component pattern getters, static `compareComponent`).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_url_pattern_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(URL_PATTERN_SHIM)?;
    Ok(())
}

/// JavaScript shim: full URLPattern Standard implementation (tokenizer, pattern parser,
/// constructor-string parser, `compareComponent`, `generate`). The file is read verbatim.
#[cfg(feature = "v8-backend")]
pub(crate) const URL_PATTERN_SHIM: &str = include_str!("shim/url_pattern_shim.js");

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_url_pattern(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // The pattern shim canonicalizes through the page's `URL` class.
        crate::js_url::install_url_parse_v8(&rt).unwrap();
        rt.eval(crate::dom::URL_PARSE_SHIM).unwrap();
        rt.eval(crate::dom::URL_SHIM).unwrap();
        super::install_url_pattern_api_v8(&rt).unwrap();
        f(&rt);
    }

    fn eval_str(src: &str) -> String {
        let mut out = String::new();
        with_url_pattern(|rt| match rt.eval(src).unwrap() {
            JsValue::String(s) => out = s,
            other => out = format!("{other:?}"),
        });
        out
    }

    #[test]
    fn test_url_pattern_basic() {
        with_url_pattern(|rt| {
            let result = rt
                .eval("new URLPattern({pathname: '/users/:id'}).test('https://x.test/users/123')")
                .unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_url_pattern_exec_result_shape() {
        assert_eq!(
            eval_str(
                "const r = new URLPattern({pathname: '/users/:id'}).exec('https://x.test/users/456');\
                 r.pathname.groups.id + '|' + r.pathname.input + '|' + r.hostname.groups[0] + '|' + r.inputs.length"
            ),
            "456|/users/456|x.test|1"
        );
    }

    #[test]
    fn test_url_pattern_no_match() {
        with_url_pattern(|rt| {
            let result = rt
                .eval("new URLPattern({pathname: '/users/:id'}).exec('https://x.test/posts/123') === null")
                .unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_url_pattern_component_getters_and_defaults() {
        assert_eq!(
            eval_str(
                "const p = new URLPattern('https://example.com/books/:id(\\\\d+)/?q=*');\
                 [p.protocol, p.hostname, p.port, p.pathname, p.search, p.hash, p.hasRegExpGroups].join('|')"
            ),
            r"https|example.com||/books/:id(\d+)/|q=*|*|true"
        );
    }

    #[test]
    fn test_url_pattern_regexp_group_and_syntax_errors() {
        assert_eq!(
            eval_str(
                "const t = f => { try { f(); return 'ok'; } catch (e) { return e.constructor.name; } };\
                 [t(() => new URLPattern({pathname: '()'})), t(() => new URLPattern({pathname: '(a)'})),\
                  t(() => new URLPattern('(\\\\'))].join(',')"
            ),
            "TypeError,ok,TypeError"
        );
    }

    #[test]
    fn test_url_pattern_generate_and_compare() {
        assert_eq!(
            eval_str(
                "const a = new URLPattern({pathname: '/foo/bar'}), b = new URLPattern({pathname: '/foo/:bar'});\
                 new URLPattern({pathname: '/:foo'}).generate('pathname', {foo: 'x y'}) + '|' +\
                 URLPattern.compareComponent('pathname', a, b)"
            ),
            "/x%20y|1"
        );
    }
}
