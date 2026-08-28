//! Тесты `v8_css_storage_nav_misc`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-2).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// ── CSS.supports() / CSS.escape() ──────────────────────────────────────

#[test]
fn css_object_exists_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.CSS === 'object'"));
}

#[test]
fn css_supports_two_arg_known_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.supports('display', 'grid')"));
}

#[test]
fn css_supports_two_arg_unknown_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!bool_eval(&rt, "CSS.supports('--custom-var', '1')"));
}

#[test]
fn css_supports_one_arg_known_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.supports('(color: red)')"));
}

#[test]
fn css_supports_one_arg_unknown_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(!bool_eval(&rt, "CSS.supports('(unknown-prop: x)')"));
}

#[test]
fn css_supports_one_arg_and_condition() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.supports('(display: grid) and (color: red)')"));
}

#[test]
fn css_supports_one_arg_or_with_unknown() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.supports('(unknown: x) or (color: red)')"));
}

#[test]
fn css_supports_case_insensitive() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.supports('Display', 'block')"));
}

#[test]
fn css_escape_plain_word() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("CSS.escape('hello')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("hello".into()));
}

#[test]
fn css_escape_leading_digit() {
    let rt = v8_runtime_with_dom(make_doc());
    // Leading digit '1' must be hex-escaped.
    let r = rt.eval("CSS.escape('1abc')").unwrap();
    let s = match r { lumen_core::JsValue::String(s) => s, _ => panic!("expected string") };
    assert!(s.starts_with('\\'), "leading digit should be escaped, got: {s}");
}

#[test]
fn css_supports_is_function() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof CSS.supports === 'function'"));
}

#[test]
fn css_escape_is_function() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof CSS.escape === 'function'"));
}

// ── Storage Access API ───────────────────────────────────────────────────

#[test]
fn storage_access_request_storage_access_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.requestStorageAccess === 'function'"));
}

#[test]
fn storage_access_has_storage_access_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.hasStorageAccess === 'function'"));
}

#[test]
fn storage_access_request_storage_access_for_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.requestStorageAccessFor === 'function'"));
}

#[test]
fn storage_access_has_unpartitioned_cookie_access_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof document.hasUnpartitionedCookieAccess === 'function'"));
}

// BUG-067/070: WEB_API_SHIM defined `Event` but no global `EventTarget`, so
// every shim doing `class X extends EventTarget` (WebHID, WebUSB,
// Bluetooth, WebSerial, WebXR, Navigation API) threw "EventTarget is not defined"
// during install_dom and silently failed to install.

#[test]
fn event_target_global_is_constructible() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof EventTarget === 'function' && new EventTarget() instanceof EventTarget"));
}

#[test]
fn event_target_dispatch_invokes_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var t = new EventTarget(); var hit = 0;\
                 t.addEventListener('ping', function() { hit++; });\
                 t.dispatchEvent(new Event('ping'));\
                 t.removeEventListener('ping', function() {});\
                 hit === 1"
    ));
}

// `event_target_dependent_apis_installed` split into one test per API
// (S12b-24 scoping note) for clearer per-API failure attribution.

#[test]
fn event_target_dependent_navigator_hid_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.hid === 'object'"));
}

#[test]
fn event_target_dependent_navigator_usb_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.usb === 'object'"));
}

#[test]
fn event_target_dependent_navigator_bluetooth_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.bluetooth === 'object'"));
}

#[test]
fn event_target_dependent_navigator_serial_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.serial === 'object'"));
}

#[test]
fn serial_get_ports_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.serial.getPorts() instanceof Promise"));
}

#[test]
fn serial_request_port_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.serial.requestPort({filters:[]}) instanceof Promise"));
}

#[test]
fn serial_port_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.SerialPort === 'function'"));
}

#[test]
fn event_target_dependent_navigator_xr_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.xr === 'object'"));
}

#[test]
fn webxr_is_session_supported_returns_promise_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.xr.isSessionSupported('immersive-vr') instanceof Promise"));
}

#[test]
fn webxr_request_session_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.xr.requestSession('immersive-vr') instanceof Promise"));
}

#[test]
fn webxr_stub_classes_exist() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "typeof window.XRSession === 'function' && \
                 typeof window.XRFrame === 'function' && \
                 typeof window.XRReferenceSpace === 'function' && \
                 typeof window.XRView === 'function'"
    ));
}

// ── CSS Scroll Snap L2 events (SnapChangeEvent) ──────────────────────────

#[test]
fn snap_change_event_class_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof SnapChangeEvent === 'function'"));
}

#[test]
fn snap_change_event_constructor_with_props() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "new SnapChangeEvent('snapchanging', { snapTargetBlock: 'center' }) !== undefined"
    ));
}

#[test]
fn lumen_fire_snap_changing_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof globalThis._lumen_fire_snap_changing === 'function'"));
}

#[test]
fn lumen_fire_snap_changed_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof globalThis._lumen_fire_snap_changed === 'function'"));
}

#[test]
fn snap_change_event_with_init_props() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var ev = new SnapChangeEvent('snapchanging', { \
                   snapTargetBlock: 'end', \
                   snapTargetInline: 'start' \
                 }); \
                 ev instanceof SnapChangeEvent"
    ));
}

#[test]
fn event_target_dependent_window_navigation_installed() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.navigation === 'object'"));
}

// ── Navigation API entries / History fallback ───────────────────────────

#[test]
fn navigation_entries_reads_shell_state() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "_lumen_navigation_set_state('{\"entries\":[{\"url\":\"https://a/\",\"key\":\"nav-1\",\"id\":\"id-1\",\"state\":null},{\"url\":\"https://b/\",\"key\":\"nav-2\",\"id\":\"id-2\",\"state\":null}],\"index\":1}');\
                 navigation.entries().length === 2\
                 && navigation.entries()[0].key === 'nav-1'\
                 && navigation.currentEntry.key === 'nav-2'\
                 && navigation.canGoBack() === true\
                 && navigation.canGoForward() === false"
    ));
}

#[test]
fn navigation_traverse_to_queues_numeric_action() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("navigation.traverseTo('nav-1'); true").unwrap();
    let q = rt.take_nav_updates();
    assert!(q.iter().any(|(action, _, key, _)| matches!(action, NavAction::TraverseTo) && key == "nav-1"));
}

#[test]
fn history_go_falls_back_to_shell_state() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_navigation_set_state('{\"entries\":[{\"url\":\"https://a/\",\"key\":\"nav-1\",\"id\":\"id-1\",\"state\":null},{\"url\":\"https://b/\",\"key\":\"nav-2\",\"id\":\"id-2\",\"state\":null}],\"index\":1}')").unwrap();
    rt.eval("history.go(-1); true").unwrap();
    let travs = rt.take_history_traversals();
    assert!(travs.contains(&-1));
}

#[test]
fn history_go_shell_state_out_of_range_not_queued() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_navigation_set_state('{\"entries\":[{\"url\":\"https://a/\",\"key\":\"nav-1\",\"id\":\"id-1\",\"state\":null},{\"url\":\"https://b/\",\"key\":\"nav-2\",\"id\":\"id-2\",\"state\":null}],\"index\":1}')").unwrap();
    rt.eval("history.go(-5); true").unwrap();
    let travs = rt.take_history_traversals();
    assert!(travs.is_empty());
}

#[test]
fn history_length_prefers_shell_entries() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("_lumen_navigation_set_state('{\"entries\":[{\"url\":\"https://a/\",\"key\":\"nav-1\",\"id\":\"id-1\",\"state\":null},{\"url\":\"https://b/\",\"key\":\"nav-2\",\"id\":\"id-2\",\"state\":null}],\"index\":1}')").unwrap();
    assert!(bool_eval(&rt, "history.length === 2"));
}

// ── CSS.registerProperty() ───────────────────────────────────────────────

#[test]
fn css_register_property_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof CSS.registerProperty === 'function'"));
}

#[test]
fn css_register_property_valid() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.registerProperty({ name: '--my-color', syntax: '<color>', inherits: true, initialValue: 'blue' }); true"));
}

#[test]
fn css_register_property_stored() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.registerProperty({ name: '--stored', syntax: '*', inherits: false, initialValue: 'test' }); CSS._getRegisteredProperties()['--stored'] !== undefined"));
}

#[test]
fn css_register_property_requires_name() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "try { CSS.registerProperty({ syntax: '<color>' }); false; } catch (e) { e instanceof TypeError; }"));
}

#[test]
fn css_register_property_requires_dash_prefix() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "try { CSS.registerProperty({ name: 'my-color' }); false; } catch (e) { e instanceof SyntaxError; }"));
}

#[test]
fn css_register_property_default_inherits() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.registerProperty({ name: '--default-inherit', syntax: '*', initialValue: 'val' }); CSS._getRegisteredProperties()['--default-inherit'].inherits"));
}

#[test]
fn css_register_property_default_syntax() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "CSS.registerProperty({ name: '--default-syntax', inherits: true, initialValue: 'val' }); CSS._getRegisteredProperties()['--default-syntax'].syntax === '*'"));
}

// ── PerformanceObserver misc (paint/LCP/layout-shift delivery) ──────────

#[test]
fn perf_observer_take_records() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, r#"
                var po = new PerformanceObserver(function() {});
                po.observe({entryTypes: ['paint']});
                _lumen_deliver_paint_entry('first-paint', 100);
                var records = po.takeRecords();
                records.length === 1 && records[0].entryType === 'paint' && records[0].name === 'first-paint'
                "#));
}

#[test]
fn perf_observer_lcp_entry() {
    let rt = v8_runtime_with_dom(make_doc());
    // NodeId 6 = <div id="main"> in make_doc() (nodes: root=0..text=8, len 9).
    assert!(bool_eval(&rt, r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['largest-contentful-paint']});
                _lumen_deliver_lcp_entry(6, 1024, 200.5, 210.5);
                got.length === 1 && got[0].entryType === 'largest-contentful-paint' && got[0].size === 1024 && got[0].element !== null && Math.abs(got[0].duration - 10) < 0.1
                "#));
}

#[test]
fn perf_observer_layout_shift() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, r#"
                var got = [];
                var po = new PerformanceObserver(function(list) { got = list.getEntries(); });
                po.observe({entryTypes: ['layout-shift']});
                _lumen_deliver_layout_shift(0.15, 0, false);
                got.length === 1 && got[0].entryType === 'layout-shift' && got[0].value === 0.15 && got[0].hadRecentInput === false
                "#));
}

#[test]
fn perf_observer_buffered() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, r#"
                var po1 = new PerformanceObserver(function() {});
                po1.observe({entryTypes: ['layout-shift']});
                _lumen_deliver_layout_shift(0.1, 0, false);
                var po2 = new PerformanceObserver(function() {});
                po2.observe({entryTypes: ['layout-shift'], buffered: true});
                var buffered = po2.takeRecords();
                buffered.length === 1 && buffered[0].value === 0.1
                "#));
}

#[test]
fn perf_observer_disconnect() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, r#"
                var count = 0;
                var po = new PerformanceObserver(function() { count++; });
                po.observe({entryTypes: ['layout-shift']});
                _lumen_deliver_layout_shift(0.1, 0, false);
                po.disconnect();
                _lumen_deliver_layout_shift(0.2, 0, false);
                count === 1
                "#));
}
