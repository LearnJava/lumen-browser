//! Тесты `v8_event_classes`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

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

#[test]
fn uievent_instanceof_event() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new UIEvent('focus'); \
                 (e instanceof UIEvent) && (e instanceof Event)"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn mouseevent_instanceof_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new MouseEvent('click', {clientX: 10, clientY: 20, button: 0, buttons: 1}); \
                 (e instanceof MouseEvent) && (e instanceof UIEvent) && (e instanceof Event) && \
                 e.clientX === 10 && e.clientY === 20 && e.button === 0 && e.buttons === 1"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn mouseevent_modifier_keys() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new MouseEvent('click', {ctrlKey: true, shiftKey: false, altKey: true}); \
                 e.ctrlKey && !e.shiftKey && e.altKey && \
                 e.getModifierState('Control') && e.getModifierState('Alt') && \
                 !e.getModifierState('Shift')"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn mouseevent_page_coords_default_to_client() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new MouseEvent('mousemove', {clientX: 42, clientY: 7}); \
                 e.pageX === 42 && e.pageY === 7"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn keyboardevent_instanceof_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new KeyboardEvent('keydown', {key: 'Enter', code: 'Enter', keyCode: 13}); \
                 (e instanceof KeyboardEvent) && (e instanceof UIEvent) && (e instanceof Event) && \
                 e.key === 'Enter' && e.code === 'Enter' && e.keyCode === 13"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn keyboardevent_location_constants() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "KeyboardEvent.DOM_KEY_LOCATION_STANDARD === 0 && \
                 KeyboardEvent.DOM_KEY_LOCATION_LEFT     === 1 && \
                 KeyboardEvent.DOM_KEY_LOCATION_RIGHT    === 2 && \
                 KeyboardEvent.DOM_KEY_LOCATION_NUMPAD   === 3"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn keyboardevent_repeat_and_composing() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new KeyboardEvent('keydown', {repeat: true, isComposing: false}); \
                 e.repeat === true && e.isComposing === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn inputevent_instanceof_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new InputEvent('input', {data: 'a', inputType: 'insertText'}); \
                 (e instanceof InputEvent) && (e instanceof UIEvent) && \
                 e.data === 'a' && e.inputType === 'insertText' && \
                 Array.isArray(e.getTargetRanges())"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn focusevent_instanceof_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new FocusEvent('focus', {relatedTarget: null}); \
                 (e instanceof FocusEvent) && (e instanceof UIEvent) && \
                 e.relatedTarget === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn wheelevent_instanceof_chain_and_deltas() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new WheelEvent('wheel', {deltaX: 0, deltaY: 100, deltaMode: 0}); \
                 (e instanceof WheelEvent) && (e instanceof MouseEvent) && \
                 e.deltaY === 100 && e.deltaMode === WheelEvent.DOM_DELTA_PIXEL"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn wheelevent_delta_constants() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "WheelEvent.DOM_DELTA_PIXEL === 0 && \
                 WheelEvent.DOM_DELTA_LINE  === 1 && \
                 WheelEvent.DOM_DELTA_PAGE  === 2"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn pointerevent_instanceof_chain_and_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new PointerEvent('pointerdown', {pointerId: 1, pointerType: 'mouse', isPrimary: true}); \
                 (e instanceof PointerEvent) && (e instanceof MouseEvent) && \
                 e.pointerId === 1 && e.pointerType === 'mouse' && e.isPrimary === true && \
                 Array.isArray(e.getCoalescedEvents()) && Array.isArray(e.getPredictedEvents())"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_pointer_event_delivers_to_element() {
    // _lumen_dispatch_pointer_event must fire a PointerEvent on the target node
    // with pointerId=1, pointerType='mouse', isPrimary=true per Pointer Events L2.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        "var div = document.createElement('div'); document.body.appendChild(div); \
                 var got = null; \
                 div.addEventListener('pointerdown', function(e) { got = e; }); \
                 _lumen_dispatch_pointer_event(div.__nid__, 'pointerdown', 10, 20, 0, 1, 0); \
                 got !== null && got instanceof PointerEvent && \
                 got.type === 'pointerdown' && \
                 got.clientX === 10 && got.clientY === 20 && \
                 got.pointerId === 1 && got.pointerType === 'mouse' && got.isPrimary === true && \
                 got.pressure === 0.5"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_pointer_event_bubbles_for_bubbling_types() {
    // pointerdown / pointermove / pointerup must bubble through ancestor chain.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        "var parent = document.createElement('div'); document.body.appendChild(parent); \
                 var child = document.createElement('span'); parent.appendChild(child); \
                 var bubbled = false; \
                 parent.addEventListener('pointerdown', function(e) { bubbled = e.bubbles; }); \
                 _lumen_dispatch_pointer_event(child.__nid__, 'pointerdown', 0, 0, 0, 1, 0); \
                 bubbled"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_pointer_event_no_bubble_for_enter_leave() {
    // pointerenter / pointerleave must NOT bubble (bubbles:false per spec).
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        "var parent = document.createElement('div'); document.body.appendChild(parent); \
                 var child = document.createElement('span'); parent.appendChild(child); \
                 var bubbled_to_parent = false; \
                 parent.addEventListener('pointerenter', function(e) { bubbled_to_parent = true; }); \
                 _lumen_dispatch_pointer_event(child.__nid__, 'pointerenter', 0, 0, 0, 0, 0); \
                 !bubbled_to_parent"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_pointer_event_mouseover_and_mouseenter_both_exist() {
    // Both mouseover (bubbles) and mouseenter (no bubble) should be dispatchable.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        "var el = document.createElement('div'); document.body.appendChild(el); \
                 var over = false; var enter = false; \
                 el.addEventListener('mouseover',  function() { over = true; }); \
                 el.addEventListener('mouseenter', function() { enter = true; }); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'mouseover',  5, 5, 0, 0, 0); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'mouseenter', 5, 5, 0, 0, 0); \
                 over && enter"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_pointer_event_mousedown_mouseup_sequence() {
    // mousedown and mouseup must deliver with correct button/buttons values.
    let doc = make_doc();
    let rt = v8_runtime_with_dom(doc);
    let r = rt.eval(
        "var el = document.createElement('button'); document.body.appendChild(el); \
                 var downBtns = -1; var upBtns = -1; \
                 el.addEventListener('mousedown', function(e) { downBtns = e.buttons; }); \
                 el.addEventListener('mouseup',   function(e) { upBtns   = e.buttons; }); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'mousedown', 0, 0, 0, 1, 0); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'mouseup',   0, 0, 0, 0, 0); \
                 downBtns === 1 && upBtns === 0"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn animationevent_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new AnimationEvent('animationend', {animationName: 'fade', elapsedTime: 0.5}); \
                 (e instanceof AnimationEvent) && (e instanceof Event) && \
                 e.animationName === 'fade' && e.elapsedTime === 0.5 && e.pseudoElement === ''"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn transitionevent_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new TransitionEvent('transitionend', {propertyName: 'opacity', elapsedTime: 0.3}); \
                 (e instanceof TransitionEvent) && e.propertyName === 'opacity' && e.elapsedTime === 0.3"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new StorageEvent('storage', {key: 'x', oldValue: 'a', newValue: 'b', url: 'http://ex.com/'}); \
                 e.key === 'x' && e.oldValue === 'a' && e.newValue === 'b' && e.url === 'http://ex.com/'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

// ── BUG-774: WebIDL coercion of StorageEvent's two entry points ───────
// Mirrors `tests/wpt/webstorage/event_constructor.window.js` and
// `event_initstorageevent.window.js` subtest for subtest.

#[test]
fn storageevent_ctor_arity_and_new_required() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var threwNoNew = false, threwNoArgs = false; \
                     try { StorageEvent(''); } catch (e) { threwNoNew = e instanceof TypeError; } \
                     try { new StorageEvent(); } catch (e) { threwNoArgs = e instanceof TypeError; } \
                     threwNoNew && threwNoArgs && StorageEvent.length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_ctor_defaults_for_absent_members() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('type'); \
                     e.type === 'type' && e.key === null && e.oldValue === null && \
                     e.newValue === null && e.url === '' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_ctor_null_arguments_stringify_type_and_url() {
    // `type` is a required DOMString and `url` a non-nullable USVString,
    // so an explicit `null` becomes the string \"null\" on both; the three
    // nullable members keep the null.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent(null, {key: null, oldValue: null, newValue: null, \
                                                     url: null, storageArea: null}); \
                     e.type === 'null' && e.key === null && e.oldValue === null && \
                     e.newValue === null && e.url === 'null' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_ctor_undefined_members_take_declared_defaults() {
    // The mirror image of the test above: an explicitly-`undefined`
    // dictionary member takes its default, so `url` is '' and NOT
    // 'undefined' — while the required `type` argument has no default
    // and does stringify.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent(undefined, {key: undefined, oldValue: undefined, \
                                                          newValue: undefined, url: undefined, \
                                                          storageArea: undefined}); \
                     e.type === 'undefined' && e.key === null && e.oldValue === null && \
                     e.newValue === null && e.url === '' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_arity_and_zero_arguments_throw() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'), threw = false; \
                     try { e.initStorageEvent(); } catch (err) { threw = err instanceof TypeError; } \
                     threw && e.initStorageEvent.length === 1",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_one_argument_defaults_rest() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'); e.initStorageEvent('type'); \
                     e.type === 'type' && e.bubbles === false && e.cancelable === false && \
                     e.key === null && e.oldValue === null && e.newValue === null && \
                     e.url === '' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_sensible_arguments_pass_through() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'); \
                     e.initStorageEvent('type', true, true, 'key', 'oldValue', 'newValue', 'url', localStorage); \
                     e.type === 'type' && e.bubbles === true && e.cancelable === true && \
                     e.key === 'key' && e.oldValue === 'oldValue' && e.newValue === 'newValue' && \
                     e.url === 'url' && e.storageArea === localStorage",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_eight_null_arguments() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'); \
                     e.initStorageEvent(null, null, null, null, null, null, null, null); \
                     e.type === 'null' && e.bubbles === false && e.cancelable === false && \
                     e.key === null && e.oldValue === null && e.newValue === null && \
                     e.url === 'null' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_eight_undefined_arguments() {
    // An explicitly-passed `undefined` is indistinguishable from an
    // absent argument for every parameter that HAS a default — only
    // the required `type` stringifies it.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'); \
                     e.initStorageEvent(undefined, undefined, undefined, undefined, undefined, \
                                        undefined, undefined, undefined); \
                     e.type === 'undefined' && e.bubbles === false && e.cancelable === false && \
                     e.key === null && e.oldValue === null && e.newValue === null && \
                     e.url === '' && e.storageArea === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn storageevent_init_stringifies_non_string_values() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var e = new StorageEvent('storage'); \
                     e.initStorageEvent(1, 0, 0, 2, 3, 4, 5); \
                     e.type === '1' && e.key === '2' && e.oldValue === '3' && \
                     e.newValue === '4' && e.url === '5'",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn popstateevent_state() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new PopStateEvent('popstate', {state: {page: 2}}); \
                 e.state && e.state.page === 2"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn hashchangeevent_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new HashChangeEvent('hashchange', {oldURL: 'http://ex.com/#a', newURL: 'http://ex.com/#b'}); \
                 e.oldURL === 'http://ex.com/#a' && e.newURL === 'http://ex.com/#b'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn errorevent_fields() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new ErrorEvent('error', {message: 'oops', filename: 'app.js', lineno: 10, colno: 5}); \
                 e.message === 'oops' && e.filename === 'app.js' && e.lineno === 10 && e.colno === 5"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// [BUG-813] Two WebIDL properties of the `ErrorEvent` interface that
/// only became observable once a worker error reached the page at all:
/// `Object.prototype.toString` must say `[object ErrorEvent]` (what
/// `assert_class_string` reads, and what a plain constructor does not
/// give), and the interface object must not be callable without `new`.
#[test]
fn errorevent_class_string_and_new_required() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var tag = Object.prototype.toString.call(new ErrorEvent('error')); \
                     var threw = false; \
                     try { ErrorEvent('error'); } catch (e) { threw = (e instanceof TypeError); } \
                     tag + '|' + threw",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("[object ErrorEvent]|true".to_string())
    );
}

/// [BUG-813] `Worker` is an `AbstractWorker`, i.e. an `EventTarget`, so
/// the page may dispatch its own event at it. The event object must
/// arrive untouched — including `error`, which a report coming *from*
/// the worker thread can never carry, since it crosses an agent
/// boundary and arrives as JSON. No worker is started here: what is
/// under test is the page-side class, not the thread.
#[test]
fn worker_dispatch_event_runs_the_pages_own_error_listener() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var w = new Worker('data:text/javascript,/*noop*/'); \
                     var seen = ''; var err = new Error('test'); \
                     w.addEventListener('error', function(e) { \
                       seen = [e.type, e.message, e.lineno, e.error === err, e.target === w].join('|'); \
                     }, true); \
                     var ret = w.dispatchEvent(new ErrorEvent('error', \
                       {message: 'Hello Worker', lineno: 5, colno: 6, error: err, cancelable: true})); \
                     seen + '|' + ret",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("error|Hello Worker|5|true|true|true".to_string())
    );
}

/// BUG-702: `PromiseRejectionEvent` must be constructible and carry
/// `promise`/`reason`, and `window` must own the two handler properties.
/// core-js declares V8's native Promise untrustworthy when the constructor is
/// missing and swaps in its own polyfill on every core-js site — on
/// `tbank.ru/auth/login/` that swap spun the engine forever.
#[test]
fn promise_rejection_event_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var p = Promise.resolve(); \
                     var e = new PromiseRejectionEvent('unhandledrejection', {promise: p, reason: 'boom', cancelable: true}); \
                     e.type === 'unhandledrejection' && e.promise === p && e.reason === 'boom' \
                       && (e instanceof Event) \
                       && typeof window.PromiseRejectionEvent === 'function' \
                       && ('onunhandledrejection' in window) && ('onrejectionhandled' in window) \
                       && window.onunhandledrejection === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-918: the report is due at the end of the *task*, i.e. once the
/// microtask queue has drained (HTML LS §8.1.7.3 step 4), so a handler
/// attached one `await` later — the ordinary shape of a `promise_test`
/// — still cancels it. The flush used to be an `enqueue_microtask`,
/// which runs ahead of the `await` continuation and reported all three
/// of these. The read is a second `eval` on purpose: the flush happens
/// after the job that queued the rejection has returned.
#[test]
fn bug918_rejection_handled_later_in_the_same_task_is_not_reported() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var seen = []; \
                 window.addEventListener('unhandledrejection', function(e) { seen.push(e.reason.message); }); \
                 var a = Promise.reject(new TypeError('sync')); a.catch(function() {}); \
                 var b = Promise.reject(new TypeError('one-await')); \
                 (async function() { await Promise.resolve(); b.catch(function() {}); })(); \
                 var c = Promise.reject(new TypeError('microtask')); \
                 queueMicrotask(function() { c.catch(function() {}); });",
    )
    .unwrap();
    let r = rt.eval("seen.join(',')").unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}

/// The complement of the test above: deferring the flush to the end of
/// the task must not lose a rejection nobody ever handles, and a
/// handler attached in a *later* task must still produce the
/// `rejectionhandled` half of HTML LS §8.1.7.5.
#[test]
fn bug918_unhandled_rejection_is_still_reported_at_end_of_task() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var seen = []; \
                 window.addEventListener('unhandledrejection', function(e) { seen.push('u:' + e.reason.message); }); \
                 window.addEventListener('rejectionhandled', function(e) { seen.push('h:' + e.reason.message); }); \
                 var late = Promise.reject(new TypeError('late')); \
                 Promise.reject(new TypeError('never'));",
    )
    .unwrap();
    let r = rt.eval("seen.join(',')").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("u:late,u:never".to_string())
    );
    // A later task attaches the handler: the promise was already
    // reported, so the engine owes `rejectionhandled` for it.
    rt.eval("late.catch(function() {});").unwrap();
    let r = rt.eval("seen.join(',')").unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("u:late,u:never,h:late".to_string())
    );
}

/// BUG-392: on a page where nothing is connected, `getGamepads()` must
/// report an empty list and the two Window event handler IDL attributes
/// must already exist (`'onX' in window`).
#[test]
fn gamepad_surface_clean_without_device() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "navigator.getGamepads().length === 0 \
                     && ('ongamepadconnected' in window) && ('ongamepaddisconnected' in window) \
                     && window.ongamepadconnected === null",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-392: `window.dispatchEvent` must invoke the `on<type>` IDL
/// attribute for a generic event type, not only for `load`/`error` —
/// otherwise `window.ongamepadconnected = fn` is stored where no
/// dispatch path ever looks (same class of defect as BUG-390).
#[test]
fn window_on_handler_fires_for_generic_event_type() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var seen = []; \
                     window.addEventListener('gamepadconnected', function(e) { seen.push('listener:' + e.gamepad.index); }); \
                     window.ongamepadconnected = function(e) { seen.push('onhandler:' + e.gamepad.index); }; \
                     _lumen_gamepad_connect(3, 'Pad', 'standard'); \
                     seen.join(',') === 'listener:3,onhandler:3' && navigator.getGamepads().length === 4",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn submitevent_submitter() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var btn = document.createElement('button'); \
                 var e = new SubmitEvent('submit', {bubbles: true, cancelable: true, submitter: btn}); \
                 e.submitter === btn"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-437: the shell asks the shim whether a form submission may proceed.
/// A page handler calling `preventDefault()` must come back as `false`, and
/// the event must reach listeners as a trusted, bubbling `SubmitEvent`.
#[test]
fn dispatch_submit_event_reports_prevent_default() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var form = document.getElementById('main'); \
                 var seen = null; \
                 form.addEventListener('submit', function(e) { seen = e; e.preventDefault(); }); \
                 var proceed = _lumen_dispatch_submit_event(form.__nid__, -1); \
                 proceed === false && seen !== null && seen.type === 'submit' && \
                 seen.isTrusted === true && seen.bubbles === true && seen.submitter === null"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// BUG-437: without `preventDefault()` the shell must still submit — the
/// dispatch reports `true` and exposes the submitter element.
#[test]
fn dispatch_submit_event_uncancelled_proceeds_with_submitter() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var form = document.getElementById('main'); \
                 var btn = form.querySelector('.highlight'); \
                 var got = null; \
                 form.addEventListener('submit', function(e) { got = e.submitter; }); \
                 var proceed = _lumen_dispatch_submit_event(form.__nid__, btn.__nid__); \
                 proceed === true && got !== null && got.__nid__ === btn.__nid__"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn compositionevent_data() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var e = new CompositionEvent('compositionupdate', {data: 'あ'}); \
                 (e instanceof CompositionEvent) && (e instanceof UIEvent) && e.data === 'あ'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_mouse_event_delivers_coordinates() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var received = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('click', function(e) { received = e; }); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'click', 42, 99, 0, 1, 0); \
                 received !== null && received instanceof MouseEvent && \
                 received.clientX === 42 && received.clientY === 99 && \
                 received.button === 0 && received.buttons === 1 && received.isTrusted === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_key_event_delivers_properties() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var received = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('keydown', function(e) { received = e; }); \
                 _lumen_dispatch_key_event(el.__nid__, 'keydown', 'Enter', 'Enter', 13, 0, 0, false, false); \
                 received !== null && received instanceof KeyboardEvent && \
                 received.key === 'Enter' && received.code === 'Enter' && received.keyCode === 13 && \
                 received.isTrusted === true"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn dispatch_mouse_event_modifier_flags() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "var received = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('click', function(e) { received = e; }); \
                 _lumen_dispatch_mouse_event(el.__nid__, 'click', 0, 0, 0, 1, 3); \
                 received !== null && received.ctrlKey === true && received.shiftKey === true && \
                 received.altKey === false && received.metaKey === false"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn window_exports_all_event_classes() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof window.UIEvent === 'function' && \
                 typeof window.MouseEvent === 'function' && \
                 typeof window.KeyboardEvent === 'function' && \
                 typeof window.InputEvent === 'function' && \
                 typeof window.FocusEvent === 'function' && \
                 typeof window.WheelEvent === 'function' && \
                 typeof window.PointerEvent === 'function' && \
                 typeof window.AnimationEvent === 'function' && \
                 typeof window.TransitionEvent === 'function' && \
                 typeof window.StorageEvent === 'function' && \
                 typeof window.PopStateEvent === 'function' && \
                 typeof window.HashChangeEvent === 'function' && \
                 typeof window.ErrorEvent === 'function' && \
                 typeof window.SubmitEvent === 'function' && \
                 typeof window.DragEvent === 'function' && \
                 typeof window.ClipboardEvent === 'function' && \
                 typeof window.CompositionEvent === 'function'"
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}
