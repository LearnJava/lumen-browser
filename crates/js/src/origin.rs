//! GAP-ORIGIN — the native half of the `Origin` WebIDL interface (HTML LS
//! §7.1.1 «origin», the `Origin` API of whatwg/html#11846); the class itself
//! lives in `shim/origin_shim.js`, spliced into both the page shim and every
//! worker scope (`[Exposed=*]`).
//!
//! **Why a native.** Two things the JS layer cannot decide on its own:
//! - *tuple vs opaque.* `_lumen_parse_url(...).origin` (the source of
//!   `URL.prototype.origin`) keys on "has a host", so `foo://host` looks like a
//!   tuple origin and `blob:` never looks at its inner URL. The URL Standard's
//!   origin algorithm is in the `url` crate already
//!   ([`lumen_core::url::Url::tuple_origin`]);
//! - *site.* `isSameSite()` compares registrable domains, which needs the
//!   Public Suffix List. `lumen-storage` owns the PSL implementation but
//!   depends on `lumen-js` itself, so the table reaches this crate the same
//!   way the platform audio backend does — a process-global provider the
//!   shell installs at startup ([`set_public_suffix_list`]). Without one the
//!   site of a domain is the domain itself: strictly narrower, never wider,
//!   than the real same-site relation.
//!
//! Opaque origins carry no data at all here — their identity (two opaque
//! origins are same-origin only if they are *the same* origin) is a JS-side
//! counter in the shim.

use std::sync::{Arc, OnceLock, RwLock};

use lumen_core::ext::{JsValue, PublicSuffixList};
use lumen_core::url::Url;

static PSL: OnceLock<RwLock<Option<Arc<dyn PublicSuffixList>>>> = OnceLock::new();

fn psl_lock() -> &'static RwLock<Option<Arc<dyn PublicSuffixList>>> {
    PSL.get_or_init(|| RwLock::new(None))
}

/// Install the Public Suffix List `Origin.prototype.isSameSite()` resolves
/// registrable domains with. Process-global; call once at startup, before
/// any JS context runs. A later call replaces the previous list.
pub fn set_public_suffix_list(psl: Arc<dyn PublicSuffixList>) {
    *psl_lock().write().unwrap_or_else(|e| e.into_inner()) = Some(psl);
}

/// The "site" half of a tuple origin: the registrable domain of a domain host,
/// or the host itself for an IP address, a public suffix, or when no PSL is
/// installed (HTML LS §7.1.1 «obtain a site»).
fn site_of(host: &str, is_domain: bool) -> String {
    if !is_domain {
        return host.to_owned();
    }
    let guard = psl_lock().read().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|psl| psl.registrable_domain(host))
        .unwrap_or(host)
        .to_owned()
}

/// BUG-1208: the ASCII serialization of `url`'s origin (HTML LS §7.1.1
/// «ascii serialization of an origin») — `"null"` for an opaque origin or an
/// unparseable `url`, `scheme://host[:port]` (default port omitted) for a
/// tuple one. What `window.origin`/`self.origin`/`location.origin` must
/// report for a realm whose own URL is `url` — including a `blob:` URL,
/// whose origin is that of the URL in its path ([`Url::origin_serialization`],
/// BUG-1197).
///
/// The one caller outside this file ([`crate::v8_runtime::V8JsRuntime::install_dom`])
/// also uses this for the `about:blank`/`about:srcdoc` inheritance case: pass
/// the PARENT document's URL instead of the child's own `about:` address.
pub(crate) fn origin_serialization_for_url(url: &str) -> String {
    Url::parse(url)
        .map(|u| u.origin_serialization())
        .unwrap_or_else(|_| "null".to_owned())
}

/// `_lumen_url_origin(href)` — `null` when `href` is not an absolute URL,
/// `{opaque: true}` for an opaque origin, otherwise
/// `{opaque: false, scheme, host, port, site}`.
pub(crate) fn url_origin_native(href: &str) -> Option<JsValue> {
    let url = Url::parse(href).ok()?;
    let Some(t) = url.tuple_origin() else {
        return Some(JsValue::object([("opaque".to_string(), JsValue::Bool(true))]));
    };
    let site = site_of(&t.host, t.is_domain);
    Some(JsValue::object([
        ("opaque".to_string(), JsValue::Bool(false)),
        ("scheme".to_string(), JsValue::String(t.scheme)),
        ("host".to_string(), JsValue::String(t.host)),
        ("port".to_string(), JsValue::Number(f64::from(t.port))),
        ("site".to_string(), JsValue::String(site)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(v: &'a JsValue, key: &str) -> &'a JsValue {
        match v {
            JsValue::Object(pairs) => pairs
                .iter()
                .find(|(k, _)| k == key)
                .map_or_else(|| panic!("no field {key}"), |(_, v)| v),
            other => panic!("expected an object, got {other:?}"),
        }
    }

    #[test]
    fn invalid_url_has_no_origin() {
        assert!(url_origin_native("not-valid").is_none());
        assert!(url_origin_native("").is_none());
    }

    #[test]
    fn opaque_and_tuple_origins() {
        let o = url_origin_native("data:text/plain,x").unwrap();
        assert!(matches!(field(&o, "opaque"), JsValue::Bool(true)));
        let t = url_origin_native("https://sub.site.example:123/p").unwrap();
        assert!(matches!(field(&t, "opaque"), JsValue::Bool(false)));
        assert!(matches!(field(&t, "port"), JsValue::Number(p) if *p == 123.0));
        assert!(matches!(field(&t, "host"), JsValue::String(h) if h == "sub.site.example"));
    }

    #[test]
    fn ip_host_is_its_own_site() {
        assert_eq!(site_of("127.0.0.1", false), "127.0.0.1");
    }
}
