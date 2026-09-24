//! LIB-11 (BUG-693): a native `_lumen_url_parse` binding onto
//! `lumen_core::url::Url` (the `url` crate — WHATWG URL Standard state
//! machine, ADR-027), replacing `_lumen_parse_url`'s ~35-line hand-rolled
//! string-splitter in `shim/url_parse_shim.js`.
//!
//! **Why a native, not a JS rewrite.** `_lumen_parse_url` is called from a
//! dozen shim files (`URL`, `location`, `<a>`/`<area>`
//! `HTMLHyperlinkElementUtils`, worker `WorkerLocation`) that all expect the
//! same nine-field object shape it has always returned — rewriting the
//! *algorithm* in JS a second time would just relocate the parser, not fix
//! it. Binding `lumen_core::url::Url` gives every one of those call sites
//! IDNA/punycode, percent-encoding, the special-scheme table and userinfo
//! parsing in one place, for the price of dropping in a native that keeps
//! the exact same return shape.
//!
//! **What stays JS-side.** `_lumen_parse_url` used to double as the relative-
//! reference resolver's helper (`_url_resolve` in `url_shim.js` parsed the
//! base to read `bp.protocol`/`bp.host`/`bp.pathname`) and as the component
//! *setters*' re-serializer (`_lumen_url_reserialize` reassembles an href
//! from components, then re-parses it). Both keep working unmodified: this
//! native accepts an optional `base` argument and does the WHATWG "basic URL
//! parser with base" resolution itself (`Url::resolve`), so `url_shim.js`'s
//! `_url_resolve`/`URL` constructor can call it directly with `(href, base)`
//! instead of hand-rolling dot-segment/protocol-relative logic in JS — that
//! hand-rolled resolution algorithm in `url_shim.js` is deleted by this same
//! change, not kept as a second path.
//!
//! **Field mapping.** `has_authority` ↔ [`lumen_core::url::Url::has_authority`]
//! (`inner.cannot_be_a_base()` inverted); `hostname`/`host`/`origin` come from
//! [`lumen_core::url::Url::host_ascii_normalized`] (ASCII/IDNA form — what
//! the URL Standard defines `host`/`hostname` to serialize as), NOT
//! [`lumen_core::url::Url::host`] (that accessor's raw Unicode substring is
//! for the address bar's spoof guard only, per its own module doc — the JS
//! surface is not a display surface).

use lumen_core::ext::JsValue;
use lumen_core::url::Url;

/// Build the same nine-field-plus-`hasAuthority` object
/// `shim/url_parse_shim.js`'s `_lumen_parse_url` used to construct by hand,
/// from a parsed [`Url`]. Kept as a free function so both the page and
/// worker native (below) share one mapping.
fn url_to_js_object(u: &Url) -> JsValue {
    let hostname = u.host_ascii_normalized().to_owned();
    let port = u.port().map(|p| p.to_string()).unwrap_or_default();
    let host = if port.is_empty() {
        hostname.clone()
    } else {
        format!("{hostname}:{port}")
    };
    JsValue::object([
        (
            "href".to_string(),
            JsValue::String(u.href_whatwg().to_owned()),
        ),
        (
            "protocol".to_string(),
            JsValue::String(format!("{}:", u.scheme())),
        ),
        (
            "username".to_string(),
            JsValue::String(u.username().to_owned()),
        ),
        (
            "password".to_string(),
            JsValue::String(u.password().unwrap_or("").to_owned()),
        ),
        ("hostname".to_string(), JsValue::String(hostname)),
        ("host".to_string(), JsValue::String(host)),
        ("port".to_string(), JsValue::String(port)),
        ("pathname".to_string(), JsValue::String(u.path().to_owned())),
        (
            "search".to_string(),
            JsValue::String(match u.query() {
                Some(q) if !q.is_empty() => format!("?{q}"),
                _ => String::new(),
            }),
        ),
        (
            "hash".to_string(),
            JsValue::String(match u.fragment() {
                Some(f) if !f.is_empty() => format!("#{f}"),
                _ => String::new(),
            }),
        ),
        ("origin".to_string(), JsValue::String(u.origin())),
        ("hasAuthority".to_string(), JsValue::Bool(u.has_authority())),
    ])
}

/// `_lumen_url_parse(href, base?)` — parses `href` (resolved against `base`
/// when given, per the WHATWG "basic URL parser with base" algorithm) and
/// returns the field object above, or `null` on a parse failure. `base`
/// itself is parsed with no base of its own; a malformed `base` therefore
/// also yields `null` for a relative `href`, matching `new URL(rel, base)`
/// throwing when `base` doesn't parse.
pub(crate) fn url_parse_native(href: String, base: Option<String>) -> Option<JsValue> {
    let url = match base {
        Some(base) if !base.is_empty() => {
            let base_url = Url::parse(&base).ok()?;
            base_url.resolve(&href).ok()?
        }
        _ => Url::parse(&href).ok()?,
    };
    Some(url_to_js_object(&url))
}

/// Registers `_lumen_url_parse` on `rt` — called from both the page
/// (`V8JsRuntime::install_dom`, via [`crate::v8_runtime::install::install_url_parse`])
/// and every worker flavour (`install_worker_exposed_v8`), the same way
/// `_lumen_text_decode`/`_lumen_cs_*` are shared (WORKER-1's pattern): one
/// native, one registration site per runtime, so a worker gets identical URL
/// semantics to the page.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_url_parse_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn2;
    rt.register_native(
        "_lumen_url_parse",
        into_v8_fn2(|href: String, base: Option<String>| -> Option<JsValue> {
            url_parse_native(href, base)
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(v: &'a JsValue, key: &str) -> &'a JsValue {
        match v {
            JsValue::Object(pairs) => pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .expect("field present"),
            _ => panic!("expected object"),
        }
    }
    fn s(v: &JsValue) -> &str {
        match v {
            JsValue::String(s) => s,
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn parses_absolute_https_url() {
        let obj = url_parse_native(
            "https://user:pass@example.com:8080/a/b?q=1#f".to_string(),
            None,
        )
        .unwrap();
        assert_eq!(s(field(&obj, "protocol")), "https:");
        assert_eq!(s(field(&obj, "username")), "user");
        assert_eq!(s(field(&obj, "password")), "pass");
        assert_eq!(s(field(&obj, "hostname")), "example.com");
        assert_eq!(s(field(&obj, "host")), "example.com:8080");
        assert_eq!(s(field(&obj, "port")), "8080");
        assert_eq!(s(field(&obj, "pathname")), "/a/b");
        assert_eq!(s(field(&obj, "search")), "?q=1");
        assert_eq!(s(field(&obj, "hash")), "#f");
        assert_eq!(s(field(&obj, "origin")), "https://example.com:8080");
        assert!(matches!(field(&obj, "hasAuthority"), JsValue::Bool(true)));
    }

    #[test]
    fn resolves_relative_against_base() {
        let obj = url_parse_native(
            "css/style.css".to_string(),
            Some("https://example.com/dir/page.html".to_string()),
        )
        .unwrap();
        assert_eq!(
            s(field(&obj, "href")),
            "https://example.com/dir/css/style.css"
        );
    }

    #[test]
    fn opaque_path_url_has_no_authority() {
        let obj = url_parse_native("mailto:a@b.com".to_string(), None).unwrap();
        assert!(matches!(field(&obj, "hasAuthority"), JsValue::Bool(false)));
        assert_eq!(s(field(&obj, "origin")), "");
        assert_eq!(s(field(&obj, "hostname")), "");
    }

    #[test]
    fn invalid_url_returns_none() {
        assert!(url_parse_native("not a url".to_string(), None).is_none());
    }

    #[test]
    fn idna_hostname_is_ascii_normalized() {
        let obj = url_parse_native("https://президент.рф/".to_string(), None).unwrap();
        assert_eq!(s(field(&obj, "hostname")), "xn--d1abbgf6aiiy.xn--p1ai");
    }

    #[test]
    fn embedded_tab_and_newline_are_stripped() {
        let obj = url_parse_native("http://exa\tmple.\norg/".to_string(), None).unwrap();
        assert_eq!(s(field(&obj, "hostname")), "example.org");
    }
}
