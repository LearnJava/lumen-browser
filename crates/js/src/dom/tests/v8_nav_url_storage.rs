//! V8 port of the Location/NavigateRequest, Web Storage and
//! URLSearchParams/URL test families (slice S12b-24-nav-url-storage).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of the deleted `runtime_with_dom`: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}
// ── location / NavigateRequest tests ─────────────────────────────────────

fn v8_runtime_with_url(url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), url, None, None, None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn location_href_initialised_from_page_url() {
    let rt = v8_runtime_with_url("https://example.com/path?q=1#top");
    let r = rt.eval("location.href").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/path?q=1#top".into()));
}

#[test]
fn location_fields_parsed_correctly() {
    let rt = v8_runtime_with_url("https://example.com:8080/path/to?q=hello#sec");
    let proto    = rt.eval("location.protocol").unwrap();
    let hostname = rt.eval("location.hostname").unwrap();
    let host     = rt.eval("location.host").unwrap();
    let port     = rt.eval("location.port").unwrap();
    let pathname = rt.eval("location.pathname").unwrap();
    let search   = rt.eval("location.search").unwrap();
    let hash     = rt.eval("location.hash").unwrap();
    let origin   = rt.eval("location.origin").unwrap();
    assert_eq!(proto,    lumen_core::JsValue::String("https:".into()));
    assert_eq!(hostname, lumen_core::JsValue::String("example.com".into()));
    assert_eq!(host,     lumen_core::JsValue::String("example.com:8080".into()));
    assert_eq!(port,     lumen_core::JsValue::String("8080".into()));
    assert_eq!(pathname, lumen_core::JsValue::String("/path/to".into()));
    assert_eq!(search,   lumen_core::JsValue::String("?q=hello".into()));
    assert_eq!(hash,     lumen_core::JsValue::String("#sec".into()));
    assert_eq!(origin,   lumen_core::JsValue::String("https://example.com:8080".into()));
}

#[test]
fn location_href_empty_when_no_url() {
    let rt = v8_runtime_with_url("");
    let r = rt.eval("location.href").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("".into()));
}

#[test]
fn location_assign_sets_navigate_push() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("location.assign('https://target.example/page')").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://target.example/page"));
}

#[test]
fn location_href_setter_sets_navigate_push() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("location.href = 'https://other.example/'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://other.example/"));
}

#[test]
fn location_replace_sets_navigate_replace() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("location.replace('https://new.example/')").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Replace(u)) if u == "https://new.example/"));
}

#[test]
fn location_reload_sets_navigate_reload() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("location.reload()").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Reload)));
}

// BUG-359: cross-document `location.href =`/`assign()`/`replace()` must
// resolve a relative target against the current document URL before
// queuing the navigation request — previously the resolved URL was
// computed only to decide fragment-vs-cross-document and then thrown
// away, leaving the raw relative string to reach the shell/network layer.
#[test]
fn location_assign_resolves_relative_url() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html");
    rt.eval("location.assign('support/x.html')").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com/dir/support/x.html"));
}

#[test]
fn location_href_setter_resolves_relative_url() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html");
    rt.eval("location.href = 'support/x.html'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com/dir/support/x.html"));
}

#[test]
fn location_replace_resolves_relative_url() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html");
    rt.eval("location.replace('support/x.html')").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Replace(u)) if u == "https://example.com/dir/support/x.html"));
}

#[test]
fn no_navigate_request_when_no_navigation() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("1 + 1").unwrap();
    assert!(rt.take_navigate_request().is_none());
}

#[test]
fn location_hash_setter_updates_hash() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.hash = 'sec';").unwrap();
    assert_eq!(rt.eval("location.hash").unwrap(), lumen_core::JsValue::String("#sec".into()));
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#sec".into())
    );
}

#[test]
fn location_hash_setter_strips_leading_hash() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.hash = '#top';").unwrap();
    assert_eq!(rt.eval("location.hash").unwrap(), lumen_core::JsValue::String("#top".into()));
}

#[test]
fn location_hash_setter_fires_hashchange() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("var fired=null; window.onhashchange=function(e){ fired=e.newURL; }; location.hash='x';")
        .unwrap();
    // BUG-832: the dispatch is a queued task now, so the event loop has
    // to turn once before the handler has been called.
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("fired").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#x".into())
    );
}

#[test]
fn location_hash_setter_fires_addeventlistener() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("var n=0; window.addEventListener('hashchange', function(){ n++; }); location.hash='a';")
        .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(rt.eval("n").unwrap(), lumen_core::JsValue::Number(1.0));
}

/// BUG-832: `hashchange` must reach a listener registered on the line
/// AFTER the assignment that caused it — §7.10.6 queues the dispatch as
/// a task, and this is the ordering all four residual `scroll-to-fragid`
/// tests depend on. Before the fix the setter ran the listener list inline,
/// so this page heard nothing at all.
#[test]
fn bug832_hashchange_reaches_listener_registered_after_the_assignment() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval(
        "var fired=null; location.hash='late'; \
                 window.addEventListener('hashchange', function(e){ fired=e.newURL; });",
    )
    .unwrap();
    assert_eq!(rt.eval("fired").unwrap(), lumen_core::JsValue::Null);
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("fired").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#late".into())
    );
}

/// BUG-832: the event object is built at queueing time, so two writes in
/// one turn deliver two events carrying their own URL pair, in order —
/// not two copies of whatever `location` settled on.
#[test]
fn bug832_two_hash_writes_in_one_turn_deliver_both_url_pairs_in_order() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval(
        "var log=[]; window.addEventListener('hashchange', function(e){ \
                     log.push(e.oldURL + '>' + e.newURL); }); \
                 location.hash='a'; location.hash='b';",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("log.join('|')").unwrap(),
        lumen_core::JsValue::String(
            "https://example.com/page>https://example.com/page#a|\
                     https://example.com/page#a>https://example.com/page#b"
                .into()
        )
    );
}

/// BUG-832: the queued dispatch keeps BUG-591's reporting — a listener
/// that throws does not take the listeners after it down with it.
#[test]
fn bug832_throwing_hashchange_listener_does_not_stop_the_next_one() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval(
        "var n=0; \
                 window.addEventListener('hashchange', function(){ throw new Error('boom'); }); \
                 window.addEventListener('hashchange', function(){ n++; }); \
                 location.hash='x';",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(rt.eval("n").unwrap(), lumen_core::JsValue::Number(1.0));
}

#[test]
fn location_hash_setter_same_value_noop() {
    let rt = v8_runtime_with_url("https://example.com/page#sec");
    rt.eval("var n=0; window.addEventListener('hashchange', function(){ n++; }); location.hash='sec';")
        .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(rt.eval("n").unwrap(), lumen_core::JsValue::Number(0.0));
}

#[test]
fn location_hash_setter_no_navigate_request() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.hash='b';").unwrap();
    assert!(rt.take_navigate_request().is_none());
}

#[test]
fn location_hash_setter_enqueues_history_push() {
    let rt = v8_runtime_with_url("https://example.com/p");
    rt.eval("location.hash='c';").unwrap();
    let updates = rt.take_history_url_updates();
    assert_eq!(updates.len(), 1);
    assert!(matches!(&updates[0], HistoryUrlUpdate::Push { url, .. } if url == "https://example.com/p#c"));
}

#[test]
fn location_hash_setter_increments_history_length() {
    let rt = v8_runtime_with_url("https://example.com/page");
    let delta = rt
        .eval("var before = history.length; location.hash='d'; history.length - before;")
        .unwrap();
    assert_eq!(delta, lumen_core::JsValue::Number(1.0));
}

// ── BUG-376: `window.location =` and the component setters ───────────
// §1 — the single most common navigation idiom on the web. `location`
// used to be a writable `var` binding, so this assignment replaced the
// Location object with a bare string: no navigation happened AND every
// later `location.*` access in the page was broken beyond repair
// (`configurable:false` made it unrestorable).
#[test]
fn window_location_assignment_navigates() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("window.location = 'https://target.example/page'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://target.example/page"));
}

#[test]
fn window_location_assignment_keeps_location_object() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("window.location = 'https://target.example/'").unwrap();
    assert_eq!(rt.eval("typeof window.location").unwrap(), lumen_core::JsValue::String("object".into()));
    assert_eq!(rt.eval("typeof window.location.assign").unwrap(), lumen_core::JsValue::String("function".into()));
}

// The bare `location = url` form must behave identically: the accessor
// lives on the global object, so an unqualified assignment reaches it.
#[test]
fn bare_location_assignment_navigates() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("location = 'https://target.example/bare'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://target.example/bare"));
}

// `Document.location` is `[PutForwards=href]` too.
#[test]
fn document_location_assignment_navigates() {
    let rt = v8_runtime_with_url("https://start.example/");
    rt.eval("document.location = 'https://target.example/doc'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://target.example/doc"));
}

// The global `location` property is `[LegacyUnforgeable]`: an accessor
// that cannot be redefined or shadowed by a plain value.
#[test]
fn window_location_is_unforgeable_accessor() {
    let rt = v8_runtime_with_url("https://example.com/");
    let d = rt
        .eval(
            "var d = Object.getOwnPropertyDescriptor(globalThis, 'location'); \
                     (typeof d.get) + '/' + (typeof d.set) + '/' + d.configurable",
        )
        .unwrap();
    assert_eq!(d, lumen_core::JsValue::String("function/function/false".into()));
}

// §2 — component setters navigate instead of silently mutating a field
// and leaving `href`/`toString()` describing the old URL.
#[test]
fn location_pathname_setter_navigates() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html?q=1");
    rt.eval("location.pathname = '/hijacked'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com/hijacked?q=1"));
}

#[test]
fn location_search_setter_navigates() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.search = '?injected=1'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com/page?injected=1"));
}

#[test]
fn location_protocol_setter_navigates() {
    let rt = v8_runtime_with_url("http://example.com/page");
    rt.eval("location.protocol = 'https'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com/page"));
}

#[test]
fn location_hostname_setter_navigates_keeping_port() {
    let rt = v8_runtime_with_url("https://example.com:8080/page");
    rt.eval("location.hostname = 'other.example'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://other.example:8080/page"));
}

#[test]
fn location_host_setter_navigates() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.host = 'other.example:9000'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://other.example:9000/page"));
}

#[test]
fn location_port_setter_navigates() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.port = '8443'").unwrap();
    let req = rt.take_navigate_request();
    assert!(matches!(req, Some(NavigateRequest::Push(u)) if u == "https://example.com:8443/page"));
}

// A write the URL Standard ignores must navigate nowhere rather than
// half-applying and desynchronising the object.
#[test]
fn location_port_setter_ignores_non_numeric() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.port = 'abc'").unwrap();
    assert!(rt.take_navigate_request().is_none());
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/page".into())
    );
}

// The object never lies about itself: until the navigation is committed
// by the engine, every component still describes the current URL.
#[test]
fn location_component_setter_keeps_object_consistent() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.search = '?injected=1'").unwrap();
    let s = rt
        .eval("location.href + '|' + location.search + '|' + location.toString()")
        .unwrap();
    assert_eq!(
        s,
        // `search` is still empty — the page URL carries no query until
        // the engine commits the navigation the setter requested.
        lumen_core::JsValue::String(
            "https://example.com/page||https://example.com/page".into()
        )
    );
}

// §3 — `Location` exists as an interface.
#[test]
fn location_interface_shape() {
    let rt = v8_runtime_with_url("https://example.com/");
    assert_eq!(rt.eval("typeof Location").unwrap(), lumen_core::JsValue::String("function".into()));
    assert_eq!(rt.eval("location instanceof Location").unwrap(), lumen_core::JsValue::Bool(true));
    assert_eq!(
        rt.eval("Object.prototype.toString.call(location)").unwrap(),
        lumen_core::JsValue::String("[object Location]".into())
    );
    assert_eq!(
        rt.eval("location.constructor.name").unwrap(),
        lumen_core::JsValue::String("Location".into())
    );
}

// Unforgeable members: a page cannot delete `assign` and break the
// scripts that run after it.
#[test]
fn location_members_are_not_deletable() {
    let rt = v8_runtime_with_url("https://example.com/");
    assert_eq!(rt.eval("delete location.assign").unwrap(), lumen_core::JsValue::Bool(false));
    assert_eq!(rt.eval("typeof location.assign").unwrap(), lumen_core::JsValue::String("function".into()));
    assert_eq!(rt.eval("delete location.href").unwrap(), lumen_core::JsValue::Bool(false));
}

#[test]
fn location_href_fragment_no_navigate_request() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.href = '#sec';").unwrap();
    assert!(rt.take_navigate_request().is_none());
}

#[test]
fn location_href_fragment_updates_hash() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.href = '#sec';").unwrap();
    assert_eq!(rt.eval("location.hash").unwrap(), lumen_core::JsValue::String("#sec".into()));
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#sec".into())
    );
}

#[test]
fn location_href_fragment_fires_hashchange() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("var fired=null; window.addEventListener('hashchange', function(e){ fired=e.newURL; }); location.href='#x';")
        .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert_eq!(
        rt.eval("fired").unwrap(),
        lumen_core::JsValue::String("https://example.com/page#x".into())
    );
}

#[test]
fn location_href_fragment_enqueues_history_push() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.href='#c';").unwrap();
    let u = rt.take_history_url_updates();
    assert_eq!(u.len(), 1);
    assert!(matches!(&u[0], HistoryUrlUpdate::Push { url, .. } if url == "https://example.com/page#c"));
}

#[test]
fn location_assign_fragment_no_reload() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.assign('#a');").unwrap();
    assert!(rt.take_navigate_request().is_none());
    let u = rt.take_history_url_updates();
    assert_eq!(u.len(), 1);
}

#[test]
fn location_replace_fragment_enqueues_replace() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.replace('#b');").unwrap();
    assert!(rt.take_navigate_request().is_none());
    let u = rt.take_history_url_updates();
    assert_eq!(u.len(), 1);
    assert!(matches!(&u[0], HistoryUrlUpdate::Replace { url, .. } if url == "https://example.com/page#b"));
}

#[test]
fn location_href_cross_document_still_navigates() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.href='https://example.com/other';").unwrap();
    assert!(rt.take_navigate_request().is_some());
}

#[test]
fn location_href_different_path_with_fragment_navigates() {
    let rt = v8_runtime_with_url("https://example.com/page");
    rt.eval("location.href='https://example.com/other#x';").unwrap();
    assert!(rt.take_navigate_request().is_some());
}

#[test]
fn push_state_updates_location_href() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("history.pushState(null, '', '/page2')").unwrap();
    // BUG-829: the entry URL is the argument resolved against the document
    // base URL, not the argument itself.
    let r = rt.eval("location.href").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/page2".into()));
}

#[test]
fn replace_state_updates_location_href() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("history.replaceState({x:1}, '', '/replaced')").unwrap();
    let r = rt.eval("location.href").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/replaced".into()));
}

#[test]
fn push_state_does_not_request_navigation() {
    let rt = v8_runtime_with_url("https://example.com/");
    rt.eval("history.pushState(null, '', '/other')").unwrap();
    // pushState changes URL client-side without a network request
    assert!(rt.take_navigate_request().is_none());
}

#[test]
fn push_state_enqueues_history_url_update_push() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("history.pushState({a:1}, '', '/page2')").unwrap();
    let updates = rt.take_history_url_updates();
    assert_eq!(updates.len(), 1, "one push update expected");
    match &updates[0] {
        HistoryUrlUpdate::Push { url, new_state_json } => {
            // The shell is handed the resolved absolute URL (BUG-829), so
            // the address bar and the back-stack entry agree with `location`.
            assert_eq!(url, "https://example.com/page2");
            assert_eq!(new_state_json, r#"{"a":1}"#);
        }
        other => panic!("expected Push, got {other:?}"),
    }
    // Second drain: already consumed
    assert!(rt.take_history_url_updates().is_empty());
}

#[test]
fn replace_state_enqueues_history_url_update_replace() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("history.replaceState({b:2}, '', '/new-page')").unwrap();
    let updates = rt.take_history_url_updates();
    assert_eq!(updates.len(), 1, "one replace update expected");
    match &updates[0] {
        HistoryUrlUpdate::Replace { url, new_state_json } => {
            assert_eq!(url, "https://example.com/new-page");
            assert_eq!(new_state_json, r#"{"b":2}"#);
        }
        other => panic!("expected Replace, got {other:?}"),
    }
}

#[test]
fn push_state_no_url_does_not_enqueue_update() {
    let rt = v8_runtime_with_url("https://example.com/");
    // pushState with null url → no URL update
    rt.eval("history.pushState({x:3}, '')").unwrap();
    assert!(rt.take_history_url_updates().is_empty());
}

#[test]
fn deliver_popstate_fires_onpopstate() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("var fired = null; window.onpopstate = function(e) { fired = e.state; };").unwrap();
    rt.eval("_lumen_deliver_popstate('{\"x\":42}', '/page0')").unwrap();
    let r = rt.eval("fired && fired.x").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(42.0));
}

#[test]
fn deliver_popstate_updates_location() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("_lumen_deliver_popstate('null', '/restored')").unwrap();
    // A delivered entry URL is resolved against the document base URL too
    // (BUG-829), so `location` stays a whole URL through a traversal and
    // `pathname`/`search` keep working.
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/restored".into())
    );
    assert_eq!(
        rt.eval("location.pathname").unwrap(),
        lumen_core::JsValue::String("/restored".into())
    );
}

#[test]
fn deliver_popstate_fires_event_listeners() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("var count = 0; window.addEventListener('popstate', function(e) { count += e.state.n; });").unwrap();
    rt.eval("_lumen_deliver_popstate('{\"n\":5}', '')").unwrap();
    let r = rt.eval("count").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(5.0));
}

#[test]
fn deliver_popstate_fires_hashchange_on_fragment_change() {
    let rt = v8_runtime_with_url("https://example.com/page#a");
    rt.eval("var __h = null; window.onhashchange = function(e) { window.__h = e.newURL; };").unwrap();
    rt.eval("_lumen_deliver_popstate('null', 'https://example.com/page#b')").unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    let r = rt.eval("window.__h").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/page#b".into()));
}

#[test]
fn deliver_popstate_hashchange_addeventlistener() {
    let rt = v8_runtime_with_url("https://example.com/p#a");
    rt.eval("var n = 0; window.addEventListener('hashchange', function() { n++; });").unwrap();
    rt.eval("_lumen_deliver_popstate('null', 'https://example.com/p#z')").unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    let r = rt.eval("n").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn deliver_popstate_no_hashchange_same_fragment() {
    let rt = v8_runtime_with_url("https://example.com/a#sec");
    rt.eval("var n = 0; window.addEventListener('hashchange', function() { n++; });").unwrap();
    rt.eval("_lumen_deliver_popstate('null', 'https://example.com/b#sec')").unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    let r = rt.eval("n").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn deliver_popstate_no_hashchange_empty_url() {
    let rt = v8_runtime_with_url("https://example.com/page#a");
    rt.eval("var n = 0; window.addEventListener('hashchange', function() { n++; });").unwrap();
    rt.eval("_lumen_deliver_popstate('null', '')").unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    let r = rt.eval("n").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn deliver_popstate_updates_history_state() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("_lumen_deliver_popstate('{\"x\":42}', '/page0');").unwrap();
    assert_eq!(rt.eval("history.state.x").unwrap(), lumen_core::JsValue::Number(42.0));
}

#[test]
fn deliver_popstate_empty_url_keeps_url_updates_state() {
    let rt = v8_runtime_with_url("https://example.com/page1");
    rt.eval("_lumen_deliver_popstate('{\"n\":9}', '');").unwrap();
    assert_eq!(rt.eval("history.state.n").unwrap(), lumen_core::JsValue::Number(9.0));
    assert_eq!(
        rt.eval("location.href").unwrap(),
        lumen_core::JsValue::String("https://example.com/page1".into())
    );
}

#[test]
fn location_file_url_parsed() {
    let rt = v8_runtime_with_url("file:///home/user/page.html");
    let r = rt.eval("location.protocol").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("file:".into()));
}

// ── Web Storage tests ─────────────────────────────────────────────────────

fn v8_runtime_with_storage(ls: Option<Arc<Mutex<lumen_core::WebStorage>>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, ls, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn local_storage_set_get() {
    let rt = v8_runtime_with_storage(None);
    rt.eval("localStorage.setItem('k', 'v')").unwrap();
    let r = rt.eval("localStorage.getItem('k')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("v".into()));
}

#[test]
fn local_storage_missing_key_returns_null() {
    let rt = v8_runtime_with_storage(None);
    let r = rt.eval("localStorage.getItem('nope')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Null);
}

#[test]
fn local_storage_length_and_key() {
    let rt = v8_runtime_with_storage(None);
    rt.eval("localStorage.setItem('a', '1'); localStorage.setItem('b', '2')").unwrap();
    let len = rt.eval("localStorage.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(2.0));
    // key(0) == 'a' (insertion order)
    let k0 = rt.eval("localStorage.key(0)").unwrap();
    assert_eq!(k0, lumen_core::JsValue::String("a".into()));
}

#[test]
fn local_storage_remove_item() {
    let rt = v8_runtime_with_storage(None);
    rt.eval("localStorage.setItem('x', '42'); localStorage.removeItem('x')").unwrap();
    let r = rt.eval("localStorage.getItem('x')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Null);
}

#[test]
fn local_storage_clear() {
    let rt = v8_runtime_with_storage(None);
    rt.eval("localStorage.setItem('a', '1'); localStorage.setItem('b', '2'); localStorage.clear()").unwrap();
    let len = rt.eval("localStorage.length").unwrap();
    assert_eq!(len, lumen_core::JsValue::Number(0.0));
}

#[test]
fn local_storage_persists_across_runtimes() {
    // Shared Arc<Mutex<WebStorage>> simulates the same origin across page reloads.
    let shared = Arc::new(Mutex::new(lumen_core::WebStorage::default()));
    {
        let rt = v8_runtime_with_storage(Some(Arc::clone(&shared)));
        rt.eval("localStorage.setItem('persist', 'yes')").unwrap();
    }
    let rt2 = v8_runtime_with_storage(Some(Arc::clone(&shared)));
    let r = rt2.eval("localStorage.getItem('persist')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("yes".into()));
}

#[test]
fn session_storage_fresh_per_runtime_without_owner() {
    // BUG-836: isolation is what a runtime with NO attached store gets —
    // the case of two different browsing contexts. Two documents of the
    // *same* tab share a store instead; see the test below.
    let rt1 = v8_runtime_with_storage(None);
    rt1.eval("sessionStorage.setItem('s', 'hello')").unwrap();
    let rt2 = v8_runtime_with_storage(None);
    let r = rt2.eval("sessionStorage.getItem('s')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Null);
}

/// Same as [`v8_runtime_with_storage`] but attaches a tab-owned
/// `sessionStorage` partition (BUG-836), the way the shell does.
fn v8_runtime_with_session_storage(
    ss: Arc<Mutex<lumen_core::WebStorage>>,
) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap().with_session_storage(ss);
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, None, None, None, None, false).unwrap();
    rt
}

#[test]
fn session_storage_persists_across_documents_of_a_tab() {
    // BUG-836: HTML LS §12.2 binds session storage to the browsing
    // context, so the second document of the same tab must see it.
    let tab = Arc::new(Mutex::new(lumen_core::WebStorage::default()));
    {
        let rt = v8_runtime_with_session_storage(Arc::clone(&tab));
        rt.eval("sessionStorage.setItem('s', 'hello')").unwrap();
    }
    let rt2 = v8_runtime_with_session_storage(Arc::clone(&tab));
    let r = rt2.eval("sessionStorage.getItem('s')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".into()));
}

#[test]
fn local_storage_on_window() {
    let rt = v8_runtime_with_storage(None);
    rt.eval("window.localStorage.setItem('w', 'win')").unwrap();
    let r = rt.eval("localStorage.getItem('w')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("win".into()));
}

// ── BUG-773: `Storage` as a WebIDL legacy platform object ─────────────────

/// Evaluates `src` and asserts the completion value is the given string.
fn assert_storage_str(rt: &V8JsRuntime, src: &str, want: &str) {
    let r = rt.eval(src).unwrap();
    assert_eq!(r, lumen_core::JsValue::String(want.into()), "{src}");
}

#[test]
fn storage_property_write_reaches_the_backend() {
    // The defect itself: `storage.foo = 'x'` used to create a plain JS
    // property on the wrapper — invisible to `getItem`/`length` and lost
    // on the next page load.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.foo = 'bar'; localStorage['baz'] = 'quux'; \
                 [localStorage.getItem('foo'), localStorage.getItem('baz'), localStorage.length].join(',')",
        "bar,quux,2",
    );
}

#[test]
fn storage_property_read_comes_from_the_backend() {
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.setItem('k', 'v'); \
                 [localStorage.k, localStorage['k'], localStorage.missing === undefined].join(',')",
        "v,v,true",
    );
}

#[test]
fn storage_enumeration_lists_only_keys() {
    // `storage_enumerate.window.js`: method names used to leak into
    // `Object.keys` while the real keys were absent from it.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.setItem('foo', 'bar'); localStorage.baz = 'quux'; \
                 localStorage.setItem(0, 'alpha'); localStorage[42] = 'beta'; \
                 Object.keys(localStorage).sort().join(',')",
        "0,42,baz,foo",
    );
    assert_storage_str(&rt, "Object.values(localStorage).sort().join(',')", "alpha,bar,beta,quux");
}

#[test]
fn storage_named_property_descriptor_shape() {
    // Same test: every enumerated key must be a configurable, enumerable,
    // writable data property.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.setItem('k', 'v'); \
                 var d = Object.getOwnPropertyDescriptor(localStorage, 'k'); \
                 [d.value, d.writable, d.enumerable, d.configurable].join(',')",
        "v,true,true,true",
    );
}

#[test]
fn storage_in_operator_and_delete_route_to_the_backend() {
    // `storage_in.window.js` / `storage_removeitem.window.js`.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "var out = []; \
                 out.push('name' in localStorage); \
                 localStorage['name'] = 'user1'; \
                 out.push('name' in localStorage); \
                 out.push(delete localStorage['name']); \
                 out.push(delete localStorage['unknown']); \
                 out.push('name' in localStorage); \
                 out.push(localStorage.getItem('name') === null); \
                 out.join(',')",
        "false,true,true,true,false,true",
    );
}

#[test]
fn storage_builtins_are_not_hidden_by_same_named_keys() {
    // `storage_functions_not_overwritten.window.js` — an item called
    // `clear` must not make the object unusable, because a name the
    // prototype already answers hides the named property (`Storage` has
    // no [LegacyOverrideBuiltIns]).
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "['key', 'getItem', 'setItem', 'removeItem', 'clear', 'length'] \
                     .forEach(function(b) { localStorage.setItem(b, b); }); \
                 [typeof localStorage.getItem, typeof localStorage.clear, \
                  localStorage.length, localStorage.getItem('length')].join(',')",
        "function,function,6,length",
    );
}

#[test]
fn storage_members_live_on_a_shared_prototype() {
    // `symbol-props.window.js` and `storage_builtins.window.js` both
    // reference `Storage.prototype` directly; it did not exist at all.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "[typeof Storage, \
                  localStorage.hasOwnProperty('getItem'), \
                  Object.getPrototypeOf(localStorage) === Storage.prototype, \
                  Object.getPrototypeOf(sessionStorage) === Storage.prototype, \
                  localStorage.getItem === Storage.prototype.getItem].join(',')",
        "function,false,true,true,true",
    );
}

#[test]
fn storage_prototype_property_hides_the_named_property() {
    // `set.window.js`: without [LegacyOverrideBuiltIns] a prototype data
    // property wins the *read*, while the write still reaches the store.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "Storage.prototype.x = 'proto'; localStorage.x = 'value'; \
                 [localStorage.x, localStorage.getItem('x'), \
                  Object.getOwnPropertyDescriptor(localStorage, 'x') === undefined].join(',')",
        "proto,value,true",
    );
}

#[test]
fn storage_stores_only_strings() {
    // `storage_string_conversion.window.js`: the named setter is a
    // WebIDL DOMString sink, so `null` is stored as the text `null`.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.a = null; \
                 localStorage.b = { toString: function() { return 'obj'; } }; \
                 [typeof localStorage.a, localStorage.a, localStorage.b].join(',')",
        "string,null,obj",
    );
}

#[test]
fn storage_define_property_routes_to_set_item() {
    // `defineProperty.window.js` — a data descriptor is the named setter
    // spelled a third way.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "Object.defineProperty(localStorage, 'x', { value: 'value' }); \
                 Object.defineProperty(localStorage, 9, \
                     { value: { toString: function() { return 'nine'; } } }); \
                 [localStorage.getItem('x'), localStorage.getItem('9'), localStorage.length].join(',')",
        "value,nine,2",
    );
}

#[test]
fn storage_missing_arguments_throw_type_error() {
    // `missing_arguments.window.js`: five calls, five TypeErrors — the
    // old shim silently read/wrote a key spelled `undefined` instead.
    let rt = v8_runtime_with_storage(None);
    let r = rt
        .eval(
            "var n = 0; \
                     [function() { localStorage.key(); }, \
                      function() { localStorage.getItem(); }, \
                      function() { localStorage.setItem(); }, \
                      function() { localStorage.setItem('a'); }, \
                      function() { localStorage.removeItem(); }] \
                         .forEach(function(f) { \
                             try { f(); } catch (e) { if (e instanceof TypeError) n++; } \
                         }); \
                     n",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(5.0));
}

#[test]
fn storage_symbol_properties_stay_ordinary() {
    // `symbol-props.window.js`: only *string* names go through the
    // backend, a symbol stays a plain own property of the object.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "var s = Symbol(); localStorage[s] = 'test'; \
                 var got = [localStorage[s], localStorage.length]; \
                 got.push(delete localStorage[s]); \
                 got.push(localStorage[s] === undefined); \
                 got.join(',')",
        "test,0,true,true",
    );
}

#[test]
fn storage_key_index_converts_like_unsigned_long() {
    // `storage_key.window.js` runs every index twice, once + 2^32.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.setItem('a', '1'); \
                 [localStorage.key(0), localStorage.key(0x100000000), \
                  localStorage.key(-1) === null, localStorage.key(1) === null].join(',')",
        "a,a,true,true",
    );
}

#[test]
fn storage_two_areas_are_independent_objects() {
    // One shared `Storage.prototype`, two distinct backends — a write
    // through the named setter must not cross over.
    let rt = v8_runtime_with_storage(None);
    assert_storage_str(
        &rt,
        "localStorage.k = 'local'; sessionStorage.k = 'session'; \
                 [localStorage.getItem('k'), sessionStorage.getItem('k'), \
                  localStorage !== sessionStorage].join(',')",
        "local,session,true",
    );
}

// ── URLSearchParams tests ─────────────────────────────────────────────────

#[test]
fn usp_parse_query_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams('a=1&b=2'); p.get('a') + ',' + p.get('b')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
}

#[test]
fn usp_parse_leading_question_mark() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URLSearchParams('?x=hello').get('x')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".into()));
}

#[test]
fn usp_append_and_getall() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams(); p.append('k','1'); p.append('k','2'); p.getAll('k').join(',')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
}

#[test]
fn usp_set_replaces_first() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams('a=1&a=2'); p.set('a','9'); p.toString()").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a=9".into()));
}

#[test]
fn usp_delete() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams('x=1&y=2'); p.delete('x'); p.toString()").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("y=2".into()));
}

#[test]
fn usp_has() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams('k=v'); p.has('k') && !p.has('z')").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn usp_plus_as_space() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URLSearchParams('q=hello+world').get('q')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello world".into()));
}

#[test]
fn usp_size_property() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URLSearchParams('a=1&b=2&c=3').size").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(3.0));
}

#[test]
fn usp_from_object() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var p = new URLSearchParams({foo:'bar'}); p.get('foo')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("bar".into()));
}

#[test]
fn usp_empty_string() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URLSearchParams('').size").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

// ── URL tests ─────────────────────────────────────────────────────────────

#[test]
fn url_absolute_parse() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var u = new URL('https://example.com:8080/path?q=1#top'); u.hostname + ':' + u.port").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("example.com:8080".into()));
}

#[test]
fn url_pathname_and_search() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("var u = new URL('https://x.com/a/b?c=d'); u.pathname + u.search").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("/a/b?c=d".into()));
}

#[test]
fn url_hash() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URL('https://x.com/page#section').hash").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("#section".into()));
}

#[test]
fn url_origin() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URL('https://api.example.com/data').origin").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://api.example.com".into()));
}

#[test]
fn url_resolve_relative_path() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html");
    let r = rt.eval("new URL('../other.html', location.href).pathname").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("/other.html".into()));
}

#[test]
fn url_resolve_root_relative() {
    let rt = v8_runtime_with_url("https://example.com/dir/page.html");
    let r = rt.eval("new URL('/top.html', location.href).pathname").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("/top.html".into()));
}

#[test]
fn url_tostring() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URL('https://example.com/').toString()").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://example.com/".into()));
}

#[test]
fn url_searchparams_from_url() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("new URL('https://example.com/?a=1&b=2').searchParams.get('b')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("2".into()));
}

#[test]
fn url_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.URL === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-375: every URL component is settable, not just `href` ───────────────

/// All nine writable components must take an assignment and be visible
/// in the re-serialized `href` — they used to hit an empty setter.
#[test]
fn url_component_setters_rewrite_href() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var out = [];\
                     function mk() { return new URL('https://a.example/orig?old=1#o'); }\
                     var u = mk(); u.protocol = 'ftp:';        out.push(u.protocol);\
                     u = mk(); u.hostname = 'b.example';       out.push(u.hostname);\
                     u = mk(); u.host = 'b.example:99';        out.push(u.host);\
                     u = mk(); u.port = '99';                  out.push(u.port);\
                     u = mk(); u.pathname = '/changed';        out.push(u.pathname);\
                     u = mk(); u.search = '?uuid=1';           out.push(u.search);\
                     u = mk(); u.hash = '#frag';               out.push(u.hash);\
                     u = mk(); u.username = 'usr';             out.push(u.username);\
                     u = mk(); u.password = 'pw';              out.push(u.password);\
                     out.join('|');",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "ftp:|b.example|b.example:99|99|/changed|?uuid=1|#frag|usr|pw".into()
        )
    );
}

/// The whole point of the setters: `href` must carry the change, so the
/// URL the page finally uses is the one it built.
#[test]
fn url_search_setter_reaches_href() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/track');\
                     u.search = '?uuid=1&dispatch=track';\
                     u.href;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://a.example/track?uuid=1&dispatch=track".into())
    );
}

/// A setter change must be re-derived across components, not patched in
/// place: writing `port` has to move `host` and `origin` with it.
#[test]
fn url_port_setter_updates_host_and_origin() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/p');\
                     u.port = '8443';\
                     u.host + '|' + u.origin + '|' + u.href;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "a.example:8443|https://a.example:8443|https://a.example:8443/p".into()
        )
    );
}

/// Credentials live in the parsed URL and must be reported, not faked
/// as the empty string.
#[test]
fn url_credentials_come_from_the_parse() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://user:pw@a.example/x');\
                     u.username + '|' + u.password + '|' + u.host;",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("user|pw|a.example".into()));
}

/// A username may not smuggle a host past the serializer.
#[test]
fn url_username_setter_percent_encodes_delimiters() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/x');\
                     u.username = 'e@vil/y';\
                     u.host + '|' + u.username;",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("a.example|e%40vil%2Fy".into()));
}

/// `searchParams` mutations must flow back into `href` — the object and
/// the URL used to drift apart permanently after the first `set`.
#[test]
fn url_search_params_mutation_updates_href() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/x?a=1');\
                     u.searchParams.set('a', '2');\
                     u.searchParams.append('b', '3');\
                     u.href + '|' + u.search;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://a.example/x?a=2&b=3|?a=2&b=3".into())
    );
}

/// …and the link is two-way: writing `search` must be seen by the very
/// same `searchParams` object the page is already holding.
#[test]
fn url_search_setter_refreshes_search_params() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/x?a=1');\
                     var sp = u.searchParams;\
                     u.search = '?b=9';\
                     (sp === u.searchParams) + '|' + sp.get('b') + '|' + sp.get('a');",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true|9|null".into()));
}

/// A readonly attribute must stay getter-only so strict mode reports the
/// write instead of swallowing it.
#[test]
fn url_readonly_attributes_throw_in_strict_mode() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "'use strict';\
                     var u = new URL('https://a.example/x');\
                     var got = [];\
                     try { u.origin = 'https://evil.example'; got.push('no-throw'); }\
                     catch (e) { got.push(e instanceof TypeError); }\
                     try { u.searchParams = null; got.push('no-throw'); }\
                     catch (e) { got.push(e instanceof TypeError); }\
                     got.join('|') + '|' + u.origin;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("true|true|https://a.example".into())
    );
}

/// Implementation slots must not be enumerable own properties.
#[test]
fn url_internals_are_not_web_visible() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/x?a=1');\
                     u.searchParams;\
                     Object.keys(u).length + '|' + Object.keys(u.searchParams).length;",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("0|0".into()));
}

/// An opaque-path URL has no authority: host-ish setters are no-ops
/// there rather than turning `mailto:` into a hostful URL.
#[test]
fn url_opaque_path_ignores_host_setters() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('mailto:a@b.example');\
                     u.hostname = 'evil.example';\
                     u.pathname = '/x';\
                     u.href;",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("mailto:a@b.example".into()));
}

/// An invalid value is ignored per spec — and must not corrupt the URL.
#[test]
fn url_invalid_setter_values_are_ignored() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var u = new URL('https://a.example/x');\
                     u.protocol = '1nvalid:';\
                     u.port = 'abc';\
                     u.hostname = '';\
                     u.href;",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("https://a.example/x".into()));
}

// ── BUG-356: HTMLHyperlinkElementUtils on <a>/<area> ────────────────────────

#[test]
fn anchor_href_reflects_content_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     a.setAttribute('href', 'https://example.com/p?q=1#h');\
                     document.body.appendChild(a);\
                     a.href;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://example.com/p?q=1#h".into())
    );
}

#[test]
fn anchor_url_decomposition_getters() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     a.setAttribute('href', 'https://example.com:8080/p/q.html?x=1#frag');\
                     document.body.appendChild(a);\
                     [a.protocol, a.hostname, a.host, a.port, a.pathname, a.search, a.hash, a.origin].join('|');",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "https:|example.com|example.com:8080|8080|/p/q.html|?x=1|#frag|https://example.com:8080".into()
        )
    );
}

#[test]
fn anchor_search_substr_matches_wpt_encoder_idiom() {
    // encoding/resources/encode-href-common.js: a.href = base + '?' + input;
    // a.search.substr(1) — the exact idiom behind BUG-356's 451 subtest failures.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     a.href = 'https://example.com/?' + 'foo=bar';\
                     a.search.substr(1);",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("foo=bar".into()));
}

#[test]
fn anchor_without_href_decomposition_is_empty() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     document.body.appendChild(a);\
                     a.protocol === '' && a.search === '' && a.host === '';",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn anchor_decomposition_setters_rewrite_href() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     a.setAttribute('href', 'https://example.com/old?a=1#x');\
                     document.body.appendChild(a);\
                     a.pathname = '/new'; a.search = '?b=2'; a.hash = '#y';\
                     a.href;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https://example.com/new?b=2#y".into())
    );
}

#[test]
fn anchor_host_setter_updates_hostname_and_port() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var a = document.createElement('a');\
                     a.setAttribute('href', 'https://example.com:8080/p');\
                     document.body.appendChild(a);\
                     a.host = 'foo.com:9090';\
                     a.hostname + '|' + a.port + '|' + a.href;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("foo.com|9090|https://foo.com:9090/p".into())
    );
}

#[test]
fn area_href_and_decomposition() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var ar = document.createElement('area');\
                     ar.setAttribute('href', 'https://example.com/map?a=1');\
                     document.body.appendChild(ar);\
                     ar.protocol + '|' + ar.search + '|' + ar.origin;",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("https:|?a=1|https://example.com".into())
    );
}

#[test]
fn link_does_not_get_url_decomposition_mixin() {
    // HTMLLinkElement reflects `href` but is not part of the
    // HTMLHyperlinkElementUtils mixin — only <a>/<area> get it.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var l = document.createElement('link');\
                     l.setAttribute('href', 'https://example.com/style.css');\
                     document.body.appendChild(l);\
                     l.href + '|' + (typeof l.protocol);",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "https://example.com/style.css|undefined".into()
        )
    );
}
