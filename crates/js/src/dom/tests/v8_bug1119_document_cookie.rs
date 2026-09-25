//! BUG-1119 — `document.cookie` over the tab's cookie jar.
//!
//! The runtime used to hard-wire `cookie_jar = None`, so every write was a
//! no-op and every read `""`; login.microsoftonline.com took its "cookies
//! disabled" branch, the AWS WAF token on imdb/espn/amazon was lost. The jar
//! here is the same `CookieJarProvider` the shell hands both to the page's
//! `HttpClient` and, now, to `V8JsRuntime::with_cookie_jar`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use lumen_core::ext::CookieProvider as _;
use lumen_storage::{Cookie, CookieJar, CookieJarProvider, SameSite};

fn runtime_at(url: &str, jar: Option<Arc<CookieJar>>) -> V8JsRuntime {
    let mut rt = V8JsRuntime::new().unwrap();
    if let Some(jar) = jar {
        rt = rt.with_cookie_jar(Arc::new(CookieJarProvider::new(jar)));
    }
    rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn str_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

fn jar() -> Arc<CookieJar> {
    Arc::new(CookieJar::open_in_memory().unwrap())
}

/// The bug report's repro: the seven forms Chrome keeps are kept, the
/// `SameSite=None` one without `Secure` is not (RFC 6265bis §5.5 step 21).
#[test]
fn repro_forms_are_stored_and_read_back() {
    let rt = runtime_at("http://127.0.0.1:8000/g3/cookie.html", Some(jar()));
    let all = str_of(
        &rt,
        r#"
        document.cookie = 'c_plain=1';
        document.cookie = 'c_path=1; path=/';
        document.cookie = 'c_nosp=1;path=/';
        document.cookie = 'c_ssn=1; path=/; SameSite=None';
        document.cookie = 'c_exp=1; expires=' + new Date(Date.now() + 86400e3).toUTCString();
        document.cookie = 'c_ma=1; max-age=3600';
        document.cookie = 'CkTst=G1;domain=' + document.domain + ';path=/';
        document.cookie = 'c_dom=1; domain=' + location.hostname;
        document.cookie
        "#,
    );
    let mut names: Vec<&str> =
        all.split("; ").map(|p| p.split('=').next().unwrap_or("")).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["CkTst", "c_dom", "c_exp", "c_ma", "c_nosp", "c_path", "c_plain"],
        "document.cookie = {all:?}"
    );
}

/// A cookie written by script is what the network stack sends next
/// (the bug's second acceptance criterion), and a `Set-Cookie` from the
/// network is readable by script.
#[test]
fn script_and_network_share_one_jar() {
    let jar = jar();
    let provider = CookieJarProvider::new(Arc::clone(&jar));
    provider.process_set_cookie("srv=from-header; Path=/", "example.com", "/", true, None);
    let rt = runtime_at("https://example.com/app/page.html", Some(Arc::clone(&jar)));
    assert_eq!(str_of(&rt, "document.cookie"), "srv=from-header");
    rt.eval("document.cookie = 'aws-waf-token=tok;path=/;domain=.example.com;secure;SameSite=Lax'")
        .unwrap();
    let header = provider.get_for_request("example.com", "/next", true, None, false);
    let mut sent: Vec<&str> = header.split("; ").collect();
    sent.sort_unstable();
    assert_eq!(sent, ["aws-waf-token=tok", "srv=from-header"], "Cookie: {header}");
}

/// RFC 6265 §5.3 step 10 / §5.4 step 1: script neither sees nor creates nor
/// overwrites an `HttpOnly` cookie.
#[test]
fn http_only_is_invisible_and_immutable_from_script() {
    let jar = jar();
    jar.set(
        Cookie {
            domain: "example.com".into(),
            path: "/".into(),
            name: "sid".into(),
            value: "secret".into(),
            expires_at: None,
            secure: false,
            http_only: true,
            same_site: SameSite::Lax,
        },
        None,
    )
    .unwrap();
    let rt = runtime_at("http://example.com/", Some(Arc::clone(&jar)));
    assert_eq!(str_of(&rt, "document.cookie"), "");
    rt.eval("document.cookie = 'sid=stolen; path=/'; document.cookie = 'h=1; HttpOnly'")
        .unwrap();
    assert_eq!(str_of(&rt, "document.cookie"), "");
    let provider = CookieJarProvider::new(jar);
    assert_eq!(provider.get_for_request("example.com", "/", false, None, false), "sid=secret");
}

/// The document path drives both path-match and a write's default-path.
#[test]
fn document_path_scopes_reads_and_default_path() {
    let jar = jar();
    let rt = runtime_at("http://example.com/app/page.html", Some(Arc::clone(&jar)));
    rt.eval("document.cookie = 'here=1'; document.cookie = 'other=1; path=/other'").unwrap();
    assert_eq!(str_of(&rt, "document.cookie"), "here=1");
    let root = runtime_at("http://example.com/index.html", Some(jar));
    assert_eq!(str_of(&root, "document.cookie"), "", "`here` belongs to /app");
}

/// `Secure` from an `http:` document is dropped (RFC 6265bis §5.5 step 13).
#[test]
fn secure_cookie_rejected_on_insecure_document() {
    let rt = runtime_at("http://example.com/", Some(jar()));
    rt.eval("document.cookie = 's=1; secure'").unwrap();
    assert_eq!(str_of(&rt, "document.cookie"), "");
}

/// A document without a network host is cookie-averse (HTML LS §3.1.3), and
/// a runtime with no jar keeps the old empty behaviour.
#[test]
fn cookie_averse_and_jarless_documents_stay_empty() {
    for url in ["about:blank", "data:text/html,x", "file:///C:/page.html"] {
        let rt = runtime_at(url, Some(jar()));
        rt.eval("document.cookie = 'a=1'").unwrap();
        assert_eq!(str_of(&rt, "document.cookie"), "", "{url}");
    }
    let rt = runtime_at("http://example.com/", None);
    rt.eval("document.cookie = 'a=1'").unwrap();
    assert_eq!(str_of(&rt, "document.cookie"), "");
}
