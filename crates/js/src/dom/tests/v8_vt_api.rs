//! VT-API — the script-visible half of CSS View Transitions: `ViewTransition`,
//! `ViewTransitionTypeSet`, `document.activeViewTransition`, the §6.3 state machine
//! (async update callback, skip, duplicate names), `PageRevealEvent`/`PageSwapEvent`.

use super::*;
use crate::v8_runtime::V8JsRuntime;
use crate::ViewTransitionEvent;

fn rt_with_dom() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Runs the event loop until the transition's chain of tasks has drained: every pending
/// timer is made due, a tick runs it, and the next link (callback → activation → finish)
/// queues its own.
fn settle(rt: &V8JsRuntime) {
    for _ in 0..6 {
        rt.eval(
            "for (var i = 0; i < _lumen_timers.length; i++) \
               if (_lumen_timers[i].deadline - _lumen_now_ms() < 1000) _lumen_timers[i].deadline = 0; \
             _lumen_tick_timers();",
        )
        .unwrap();
    }
}

/// [`settle`] for a scoped transition, which ends with the animations of its pseudo-element tree
/// (the UA group animation runs 0.25 s of wall-clock time).
fn settle_animations(rt: &V8JsRuntime) {
    for _ in 0..4 {
        settle(rt);
        rt.eval("_lumen_run_raf_callbacks(16); null").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    settle(rt);
}

fn log(rt: &V8JsRuntime) -> String {
    match rt.eval("log.join(',')").unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("log is not a string: {other:?}"),
    }
}

#[test]
fn interfaces_are_branded_and_not_constructible() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "typeof ViewTransition === 'function' && typeof ViewTransitionTypeSet === 'function'"));
    assert!(is_true(&rt, "typeof PageRevealEvent === 'function' && typeof PageSwapEvent === 'function'"));
    assert!(is_true(&rt, "(function() { try { new ViewTransition(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(ViewTransition, 'prototype').writable === false"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(globalThis, 'ViewTransition').enumerable === false"));
    assert!(is_true(&rt, "var vt = document.startViewTransition(); Object.prototype.toString.call(vt) === '[object ViewTransition]'"));
    assert!(is_true(&rt, "vt instanceof ViewTransition && vt.constructor === ViewTransition && Object.getOwnPropertyNames(vt).length === 0"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(ViewTransition.prototype, 'ready').enumerable === true"));
    assert!(is_true(&rt, "Object.getOwnPropertyDescriptor(ViewTransition.prototype, 'ready').get.name === 'get ready'"));
    assert!(is_true(&rt, "ViewTransition.prototype.skipTransition.length === 0 && ViewTransition.prototype.waitUntil.length === 1"));
    assert!(is_true(&rt, "(function() { try { ViewTransition.prototype.skipTransition.call({}); return false; } catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn start_view_transition_lives_on_document_prototype() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "Document.prototype.hasOwnProperty('startViewTransition') && !document.hasOwnProperty('startViewTransition')"));
    assert!(is_true(&rt, "Document.prototype.hasOwnProperty('activeViewTransition') && document.activeViewTransition === null"));
}

#[test]
fn update_callback_runs_in_a_later_task_and_events_follow_the_callback() {
    let rt = rt_with_dom();
    rt.eval("var log = []; var vt = document.startViewTransition(function() { log.push('cb'); }); log.push('after');")
        .unwrap();
    assert_eq!(log(&rt), "after", "the callback must not run synchronously");
    assert!(rt.take_view_transition_events().is_empty(), "nothing is captured before the callback");
    settle(&rt);
    rt.eval("vt.updateCallbackDone.then(function() { log.push('done'); }); vt.ready.then(function() { log.push('ready'); }); \
             vt.finished.then(function() { log.push('finished'); });")
        .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "after,cb,done,ready,finished");
    let events = rt.take_view_transition_events();
    assert!(
        matches!(events.as_slice(), [ViewTransitionEvent::Begin, ViewTransitionEvent::End]),
        "got {events:?}"
    );
}

#[test]
fn active_view_transition_follows_the_lifecycle() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var t = document.startViewTransition(function() {}); \
         log.push(document.activeViewTransition === t); \
         t.finished.then(function() { log.push(document.activeViewTransition === null); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "true,true");
}

#[test]
fn skip_transition_rejects_ready_clears_active_and_still_runs_the_callback() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var t = document.startViewTransition(function() { log.push('cb'); }); \
         t.skipTransition(); log.push(document.activeViewTransition === null ? 'cleared' : 'still'); \
         t.ready.catch(function(e) { log.push('ready:' + e.name); }); \
         t.finished.then(function() { log.push('finished'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "cleared,ready:AbortError,cb,finished");
    assert!(rt.take_view_transition_events().is_empty(), "skipped before capture: no snapshot to drop");
}

#[test]
fn skipping_after_the_snapshot_cancels_it() {
    let rt = rt_with_dom();
    rt.eval("var log = []; var t = document.startViewTransition(function() { return new Promise(function() {}); });").unwrap();
    settle(&rt);
    rt.eval("t.skipTransition();").unwrap();
    settle(&rt);
    let events = rt.take_view_transition_events();
    assert!(
        matches!(events.as_slice(), [ViewTransitionEvent::Begin, ViewTransitionEvent::Cancel]),
        "got {events:?}"
    );
}

#[test]
fn a_new_transition_aborts_the_running_one() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var a = document.startViewTransition(function() {}); \
         a.ready.catch(function(e) { log.push('a:' + e.name); }); \
         var b = document.startViewTransition(function() {}); \
         log.push(document.activeViewTransition === b);",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "true,a:AbortError");
}

#[test]
fn a_failing_callback_rejects_every_promise_and_cancels() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var t = document.startViewTransition(function() { throw new Error('boom'); }); \
         t.updateCallbackDone.catch(function(e) { log.push('done:' + e.message); }); \
         t.ready.catch(function(e) { log.push('ready:' + e.message); }); \
         t.finished.catch(function(e) { log.push('finished:' + e.message); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "done:boom,ready:boom,finished:boom");
    assert!(is_true(&rt, "document.activeViewTransition === null"));
    let events = rt.take_view_transition_events();
    assert!(
        matches!(events.as_slice(), [ViewTransitionEvent::Begin, ViewTransitionEvent::Cancel]),
        "got {events:?}"
    );
}

#[test]
fn an_async_callback_delays_activation() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var release; \
         var t = document.startViewTransition(function() { return new Promise(function(r) { release = r; }); }); \
         t.ready.then(function() { log.push('ready'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "", "ready waits for the callback's promise");
    let events = rt.take_view_transition_events();
    assert!(matches!(events.as_slice(), [ViewTransitionEvent::Begin]), "got {events:?}");
    rt.eval("release();").unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "ready");
    assert!(matches!(rt.take_view_transition_events().as_slice(), [ViewTransitionEvent::End]));
}

#[test]
fn options_dictionary_supplies_update_and_types() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var t = document.startViewTransition({ update: function() { log.push('cb'); }, types: ['slide', 'back', 'slide'] });",
    )
    .unwrap();
    assert!(is_true(&rt, "t.types instanceof ViewTransitionTypeSet && t.types === t.types"));
    assert!(is_true(&rt, "t.types.size === 2 && t.types.has('slide') && t.types.has('back') && !t.types.has('x')"));
    assert!(is_true(&rt, "Array.from(t.types).join() === 'slide,back' && [...t.types.keys()].join() === 'slide,back'"));
    assert!(is_true(&rt, "t.types.add('fade') === t.types && t.types.delete('slide') && t.types.size === 2"));
    assert!(is_true(&rt, "t.types.clear() === undefined && t.types.size === 0"));
    settle(&rt);
    assert_eq!(log(&rt), "cb");
    assert!(is_true(&rt, "document.startViewTransition().types.size === 0"));
}

#[test]
fn bad_arguments_throw_synchronously() {
    let rt = rt_with_dom();
    for call in [
        "document.startViewTransition(5)",
        "document.startViewTransition({ update: 5 })",
        "document.startViewTransition({ types: 5 })",
    ] {
        assert!(
            is_true(&rt, &format!("(function() {{ try {{ {call}; return false; }} catch (e) {{ return e instanceof TypeError; }} }})()")),
            "{call} must throw TypeError"
        );
    }
    assert!(is_true(&rt, "document.startViewTransition(null) instanceof ViewTransition && document.startViewTransition({}) instanceof ViewTransition"));
}

#[test]
fn transition_root_is_the_document_element() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "document.startViewTransition().transitionRoot === document.documentElement"));
}

#[test]
fn wait_until_needs_an_argument_and_delays_finished() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "(function() { try { document.startViewTransition().waitUntil(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    rt.eval(
        "var log = []; var go; var t = document.startViewTransition(); \
         t.waitUntil(new Promise(function(r) { go = r; })); \
         t.finished.then(function() { log.push('finished'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "");
    rt.eval("go();").unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "finished");
}

fn name_two_nodes(rt: &V8JsRuntime, first: &str, second: &str) {
    let mut styles = std::collections::HashMap::new();
    for (nid, name) in [(1u32, first), (2, second)] {
        let mut m = std::collections::HashMap::new();
        m.insert("view-transition-name".to_string(), name.to_string());
        styles.insert(nid, m);
    }
    rt.update_computed_styles(styles);
}

#[test]
fn duplicate_names_skip_the_transition_before_the_callback() {
    let rt = rt_with_dom();
    name_two_nodes(&rt, "target", "target");
    rt.eval(
        "var log = []; var t = document.startViewTransition(function() { log.push('cb'); }); \
         t.ready.catch(function(e) { log.push('ready:' + e.name); }); \
         t.updateCallbackDone.then(function() { log.push('done'); }); \
         t.finished.then(function() { log.push('finished'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "ready:InvalidStateError,cb,done,finished");
    assert!(rt.take_view_transition_events().is_empty());
}

#[test]
fn distinct_names_and_generated_names_are_fine() {
    let rt = rt_with_dom();
    name_two_nodes(&rt, "a", "b");
    rt.eval("var log = []; var t = document.startViewTransition(); t.ready.then(function() { log.push('ready'); });").unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "ready");
    name_two_nodes(&rt, "match-element", "match-element");
    rt.eval("var log = []; var t = document.startViewTransition(); t.ready.then(function() { log.push('ready'); });").unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "ready");
}

#[test]
fn page_events_carry_their_view_transition() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "'onpagereveal' in window && 'onpageswap' in window && window.onpagereveal === null"));
    assert!(is_true(&rt, "var e = new PageRevealEvent('pagereveal'); e instanceof Event && e.viewTransition === null && e.type === 'pagereveal'"));
    assert!(is_true(&rt, "var vt = document.startViewTransition(); new PageRevealEvent('pagereveal', { viewTransition: vt }).viewTransition === vt"));
    assert!(is_true(&rt, "var s = new PageSwapEvent('pageswap', { viewTransition: vt }); s.viewTransition === vt && s.activation === null"));
    assert!(is_true(&rt, "(function() { try { new PageRevealEvent(); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { new PageSwapEvent('x', { viewTransition: {} }); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { PageRevealEvent('x'); return false; } catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn view_transition_at_rule_is_a_css_view_transition_rule() {
    let doc_arc = make_doc();
    let style_nid = {
        let mut doc = doc_arc.lock().unwrap();
        let head = super::super::find_element_by_tag(&doc, "head").unwrap();
        let style = doc.create_element(QualName::html("style"));
        doc.append_child(head, style);
        style.index() as u32
    };
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc_arc, "", None, None, None, None, None, None, None, None, None, false, None).unwrap();
    rt.update_stylesheet_nodes(vec![lumen_css_parser::StylesheetNodeEntry {
        node: style_nid,
        sheet: Arc::new(lumen_css_parser::parse(
            "@view-transition { navigation: auto; types: slide back; } @view-transition { }",
        )),
        disabled: false,
    }]);
    assert!(is_true(&rt, "var rules = document.styleSheets[0].cssRules; rules.length === 2 && rules[0] instanceof CSSViewTransitionRule"));
    assert!(is_true(&rt, "rules[0] instanceof CSSRule && Object.prototype.toString.call(rules[0]) === '[object CSSViewTransitionRule]'"));
    assert!(is_true(&rt, "rules[0].navigation === 'auto' && rules[0].types.join() === 'slide,back'"));
    assert!(is_true(&rt, "rules[0].types === rules[0].types && Object.isFrozen(rules[0].types)"));
    assert!(is_true(&rt, "rules[1].navigation === 'none' && rules[1].types.length === 0"));
}

#[test]
fn computed_style_reports_the_view_transition_name() {
    let rt = rt_with_dom();
    let nid = match rt.eval("document.body.__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("nid: {other:?}"),
    };
    let mut m = std::collections::HashMap::new();
    m.insert("view-transition-name".to_string(), "hero".to_string());
    rt.update_computed_styles(std::collections::HashMap::from([(nid, m)]));
    assert!(is_true(&rt, "getComputedStyle(document.body).viewTransitionName === 'hero'"));
    assert!(is_true(&rt, "getComputedStyle(document.body).getPropertyValue('view-transition-name') === 'hero'"));
}

#[test]
fn class_and_group_properties_are_supported_and_reported() {
    let rt = rt_with_dom();
    assert!(is_true(&rt, "CSS.supports('view-transition-class', 'b')"));
    assert!(is_true(&rt, "CSS.supports('view-transition-group', 'normal')"));
    let nid = match rt.eval("document.body.__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("nid: {other:?}"),
    };
    let mut m = std::collections::HashMap::new();
    m.insert("view-transition-class".to_string(), "a b".to_string());
    m.insert("view-transition-group".to_string(), "contain".to_string());
    rt.update_computed_styles(std::collections::HashMap::from([(nid, m)]));
    assert!(is_true(&rt, "getComputedStyle(document.body).viewTransitionClass === 'a b'"));
    assert!(is_true(&rt, "getComputedStyle(document.body).getPropertyValue('view-transition-group') === 'contain'"));
}

#[test]
fn a_callback_that_never_settles_times_out() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var t = document.startViewTransition(function() { return new Promise(function() {}); }); \
         t.ready.catch(function(e) { log.push('ready:' + e.name); }); \
         t.finished.then(function() { log.push('finished'); }, function() { log.push('finished-rejected'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "", "still within the timeout");
    rt.eval("for (var i = 0; i < _lumen_timers.length; i++) _lumen_timers[i].deadline = 0; _lumen_tick_timers();")
        .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "ready:TimeoutError", "neither updateCallbackDone nor finished settles");
    assert!(is_true(&rt, "document.activeViewTransition === null"));
}

#[test]
fn wait_until_ignores_rejections() {
    let rt = rt_with_dom();
    rt.eval(
        "var log = []; var no; var t = document.startViewTransition(); \
         t.waitUntil(new Promise(function(_, rej) { no = rej; })); \
         t.finished.then(function() { log.push('finished'); });",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "");
    rt.eval("no(new Error('x'));").unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "finished");
}

/// BUG-1509 — `getComputedStyle(html, '::view-transition…')` answers the UA sheet under the
/// page's rules while a transition is active, and nothing outside of it.
#[test]
fn pseudo_tree_computed_style_during_a_transition() {
    let rt = rt_with_dom();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "#main { display: block; width: 10px; height: 10px; view-transition-name: target; \
                 mix-blend-mode: multiply; } \
         ::view-transition-image-pair(target) { position: fixed; }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        "var log = [];
         function pos(sel) { return getComputedStyle(document.documentElement, sel).position; }
         function blend(sel) { return getComputedStyle(document.documentElement, sel).mixBlendMode; }
         log.push('before:' + JSON.stringify(pos('::view-transition')));
         var vt = document.startViewTransition(function () {
           log.push('cb:' + pos('::view-transition') + '/' + pos('::view-transition-group(target)') + '/' +
                    pos('::view-transition-image-pair(target)') + '/' + pos('::view-transition-old(target)') + '/' +
                    pos('::view-transition-group(root)') + '/' + blend('::view-transition-group(target)'));
         });
         vt.ready.then(function () {
           log.push('ready:' + blend('::view-transition-group(target)') + '/' + blend('::view-transition-group(root)') + '/' +
                    pos('::view-transition-image-pair(target)') + '/' + pos('::view-transition-image-pair(root)') + '/' +
                    JSON.stringify(pos('::view-transition-group(nope)')) + '/' + blend('::view-transition-old(target)'));
         });
         vt.finished.then(function () { log.push('after:' + JSON.stringify(pos('::view-transition'))); });
         null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(
        log(&rt),
        "before:\"\",\
         cb:absolute/absolute/fixed/absolute/absolute/normal,\
         ready:multiply/normal/fixed/absolute/\"\"/plus-lighter,\
         after:\"\""
    );
}

#[test]
fn pseudo_tree_computed_style_rejects_non_root_owner_and_malformed_pseudos() {
    let rt = rt_with_dom();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("")));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval("var vt = document.startViewTransition(function () {}); null").unwrap();
    settle(&rt);
    // The transition is over; start one and probe inside its callback.
    rt.eval(
        "var log = [];
         document.startViewTransition(function () {
           var html = document.documentElement;
           log.push(JSON.stringify([
             getComputedStyle(document.body, '::view-transition').position,
             getComputedStyle(html, '::view-transition-group').position,
             getComputedStyle(html, '::view-transition-group(*)').position,
             getComputedStyle(html, '::view-transition(x)').position,
             getComputedStyle(html, '::view-transition-group(root)').position]));
         }); null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "[\"\",\"\",\"\",\"\",\"absolute\"]");
}

/// BUG-1509 — a CSS animation the page puts on the pseudo-tree is listed by `getAnimations()` while
/// the transition runs, and is gone with it.
#[test]
fn pseudo_tree_css_animation_is_listed_by_get_animations() {
    let rt = rt_with_dom();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "@keyframes k { from { margin-left: 100px; } to { margin-left: 100px; } } \
         :root::view-transition-group(root), :root::view-transition { animation: k 10s; }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        "var log = [];
         _wa_gap_keyframes_json = function (n) {
           return n === 'k' ? JSON.stringify([{ offset: 0, decls: [['margin-left', '100px']] },
                                              { offset: 1, decls: [['margin-left', '100px']] }]) : null;
         };
         _lumen_has_keyframes = function () { return true; };
         var html = document.documentElement;
         log.push(html.getAnimations({ subtree: true }).length);
         var vt = document.startViewTransition(function () {});
         vt.ready.then(function () {
           var anims = html.getAnimations({ subtree: true });
           log.push(anims.length, anims.map(function (a) { return a.animationName + '@' + a.effect.pseudoElement; }).join('+'));
           log.push(document.getAnimations().length, html.getAnimations().length);
         });
         vt.finished.then(function () { log.push(html.getAnimations({ subtree: true }).length); });
         null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "0,2,k@::view-transition+k@::view-transition-group(root),2,0,0");
}

/// BUG-1509 — with no author rules the UA sheet's own animations are on the tree: the group of
/// every capture, the old and new images of the names present in that state.
#[test]
fn pseudo_tree_ua_animations_are_listed_by_get_animations() {
    let rt = rt_with_dom();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("#main { display: block; view-transition-name: hero; }")));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        "var log = [];
         var html = document.documentElement;
         var vt = document.startViewTransition(function () {});
         vt.ready.then(function () {
           var names = function (list) {
             return list.map(function (a) { return a.animationName.replace('-ua-view-transition-', '') + '@' + a.effect.pseudoElement; }).join(' ');
           };
           log.push(names(html.getAnimations({ subtree: true })));
           log.push(document.getAnimations().length);
         });
         vt.finished.then(function () { log.push(html.getAnimations({ subtree: true }).length + '/' + document.getAnimations().length); });
         null",
    )
    .unwrap();
    settle(&rt);
    let got = log(&rt);
    // Tree order: the groups first (root, then `hero`), per name group → image pair → old → new.
    assert_eq!(
        got,
        "group-anim-root@::view-transition-group(root) fade-out@::view-transition-old(root) fade-in@::view-transition-new(root) \
         group-anim-hero@::view-transition-group(hero) fade-out@::view-transition-old(hero) fade-in@::view-transition-new(hero),6,0/0"
    );
}

// ── BUG-1511: scoped transitions (`Element.prototype.startViewTransition`) ───────────────────

/// `main` (`div#main`, the outer named element) and `inner` (`span.highlight`, its child).
fn scoped_rt(css: &str) -> V8JsRuntime {
    let rt = rt_with_dom();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(css)));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        "var log = []; var main = document.getElementById('main'); \
         var inner = document.querySelector('.highlight'); var html = document.documentElement; null",
    )
    .unwrap();
    rt
}

const NAMES_CSS: &str = "#main { display: block; view-transition-name: outer; } \
                         .highlight { display: block; view-transition-name: inner; }";

/// The groups of the pseudo-element tree of `owner`'s transition, probed from its update callback.
fn groups_in_callback(rt: &V8JsRuntime, owner: &str, start_on: &str) -> String {
    rt.eval(&format!(
        "log = []; var vt = {start_on}.startViewTransition(function () {{ \
           log.push(_lumen_vt_pseudo_list({owner}.__nid__).filter(function (k) {{ return k.indexOf('-group(') > 0; }}) \
             .map(function (k) {{ return k.slice(k.indexOf('(') + 1, -1); }}).sort().join('+')); }}); \
         vt.ready.catch(function (e) {{ log.push(e.name); }}); null",
    ))
    .unwrap();
    settle(rt);
    let out = log(rt);
    rt.eval(&format!("if ({start_on}.activeViewTransition) {start_on}.activeViewTransition.skipTransition(); null")).unwrap();
    settle(rt);
    out
}

#[test]
fn element_start_view_transition_is_scoped_to_its_element() {
    let rt = scoped_rt("");
    assert!(is_true(&rt, "Element.prototype.hasOwnProperty('startViewTransition') && Element.prototype.hasOwnProperty('activeViewTransition')"));
    assert!(is_true(&rt, "Element.prototype.startViewTransition.length === 0 && Element.prototype.startViewTransition.name === 'startViewTransition'"));
    assert!(is_true(&rt, "main.activeViewTransition === null"));
    rt.eval("var t = main.startViewTransition(function () {}); null").unwrap();
    assert!(is_true(&rt, "t instanceof ViewTransition && t.transitionRoot === main"));
    assert!(is_true(&rt, "main.activeViewTransition === t && document.activeViewTransition === null && inner.activeViewTransition === null"));
    // The document element shares its scope with the document.
    assert!(is_true(&rt, "var d = html.startViewTransition(); document.activeViewTransition === d && html.activeViewTransition === d && d.transitionRoot === html"));
    assert!(is_true(&rt, "(function() { try { Element.prototype.startViewTransition.call({}); return false; } catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { main.startViewTransition(5); return false; } catch (e) { return /on 'Element'/.test(e.message); } })()"));
    settle(&rt);
    assert!(is_true(&rt, "main.activeViewTransition === null && document.activeViewTransition === null"));
}

#[test]
fn skipping_a_scoped_transition_clears_only_its_scope() {
    let rt = scoped_rt("");
    rt.eval(
        "var t = main.startViewTransition(); var u = inner.startViewTransition(); \
         t.skipTransition(); t.ready.catch(function (e) { log.push('t:' + e.name); }); \
         u.finished.then(function () { log.push('u done'); }); null",
    )
    .unwrap();
    assert!(is_true(&rt, "main.activeViewTransition === null && inner.activeViewTransition === u"));
    settle(&rt);
    assert_eq!(log(&rt), "t:AbortError,u done");
}

#[test]
fn restarting_a_scope_aborts_its_running_transition_and_document_element_shares_the_document_scope() {
    let rt = scoped_rt("");
    rt.eval(
        "var a = document.startViewTransition(function () { log.push('cb-a'); }); \
         var b = html.startViewTransition(function () { log.push('cb-b'); }); \
         a.ready.catch(function (e) { log.push('a:' + e.name); }); b.ready.then(function () { log.push('b ready'); }); null",
    )
    .unwrap();
    assert!(is_true(&rt, "document.activeViewTransition === b && b.transitionRoot === html"));
    settle(&rt);
    assert_eq!(log(&rt), "a:AbortError,cb-a,cb-b,b ready");
}

#[test]
fn scoped_transitions_run_in_parallel_and_call_back_in_creation_order() {
    let rt = scoped_rt("");
    rt.eval(
        "var vt1 = document.startViewTransition(function () { log.push('vt1'); }); \
         var vt2 = inner.startViewTransition(function () { log.push('vt2'); }); \
         var vt3 = main.startViewTransition(function () { log.push('vt3'); }); \
         Promise.all([vt1.ready, vt2.ready, vt3.ready]).then(function () { log.push('all ready'); }); null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "vt1,vt2,vt3,all ready");
}

#[test]
fn scoped_transitions_leave_the_shell_alone() {
    let rt = scoped_rt(NAMES_CSS);
    rt.eval("main.startViewTransition(function () {}); null").unwrap();
    settle(&rt);
    assert!(rt.take_view_transition_events().is_empty());
}

#[test]
fn name_discovery_stops_at_scope_all_elements() {
    // No scopes: the document sees everything.
    let rt = scoped_rt(NAMES_CSS);
    assert_eq!(groups_in_callback(&rt, "html", "document"), "inner+outer+root");
    assert_eq!(groups_in_callback(&rt, "main", "main"), "inner+outer");
    assert_eq!(groups_in_callback(&rt, "inner", "inner"), "inner");

    // `inner` is a scope: the document and `main` no longer see it, it still sees itself.
    let rt = scoped_rt(&format!("{NAMES_CSS} .highlight {{ view-transition-scope: all; }}"));
    assert_eq!(groups_in_callback(&rt, "html", "document"), "outer+root");
    assert_eq!(groups_in_callback(&rt, "main", "main"), "outer");
    assert_eq!(groups_in_callback(&rt, "inner", "inner"), "inner");

    // `main` is a scope: the document sees none of its subtree, `main` sees itself and below.
    let rt = scoped_rt(&format!("{NAMES_CSS} #main {{ view-transition-scope: all; }}"));
    assert_eq!(groups_in_callback(&rt, "html", "document"), "root");
    assert_eq!(groups_in_callback(&rt, "main", "main"), "inner+outer");
    assert_eq!(groups_in_callback(&rt, "inner", "inner"), "inner");

    // A scope on the document element does not hide its subtree from the document.
    let rt = scoped_rt(&format!("{NAMES_CSS} html {{ view-transition-scope: all; }}"));
    assert_eq!(groups_in_callback(&rt, "html", "document"), "inner+outer+root");
}

#[test]
fn an_element_captured_by_a_running_transition_cannot_join_another_one() {
    let rt = scoped_rt(NAMES_CSS);
    rt.eval(
        "var t1 = main.startViewTransition(function () {}); \
         t1.ready.then(function () { \
           log.push('t1 ready'); \
           var t2 = inner.startViewTransition(function () {}); \
           t2.ready.catch(function (e) { log.push('t2:' + e.name); }); }); null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(log(&rt), "t1 ready,t2:InvalidStateError");
}

#[test]
fn the_scope_element_computes_to_all_while_its_transition_runs() {
    let rt = scoped_rt(NAMES_CSS);
    rt.eval(
        "var scope = function () { return getComputedStyle(main).viewTransitionScope; }; \
         log.push(scope()); var t = main.startViewTransition(); log.push(scope()); \
         log.push(getComputedStyle(inner).viewTransitionScope); \
         t.finished.then(function () { log.push(scope()); }); null",
    )
    .unwrap();
    settle_animations(&rt);
    assert_eq!(log(&rt), "none,all,none,none");
    // A skipped transition resets it at once.
    rt.eval("log = []; var u = main.startViewTransition(); u.skipTransition(); log.push(scope()); null").unwrap();
    assert_eq!(log(&rt), "none");
}

#[test]
fn scoped_pseudo_tree_is_answered_for_its_scope_element_only() {
    let rt = scoped_rt(NAMES_CSS);
    rt.eval(
        "var probe = function (el, sel) { return JSON.stringify(getComputedStyle(el, sel).position); }; \
         var t = main.startViewTransition(function () { \
           log.push(probe(main, '::view-transition') + probe(main, '::view-transition-group(outer)') + \
                    probe(main, '::view-transition-group(root)') + probe(html, '::view-transition') + \
                    probe(inner, '::view-transition')); }); \
         t.finished.then(function () { log.push(probe(main, '::view-transition')); }); null",
    )
    .unwrap();
    settle_animations(&rt);
    // `main` answers for `::view-transition` and its `outer` group; `root`, the document element and `inner` do not.
    assert_eq!(log(&rt), ["\"absolute\"", "\"absolute\"", "\"\"", "\"\"", "\"\""].concat() + ",\"\"");
}

#[test]
fn scoped_pseudo_animations_target_the_scope_element() {
    let rt = scoped_rt(NAMES_CSS);
    rt.eval(
        "var t = main.startViewTransition(); \
         t.ready.then(function () { \
           log.push(document.getAnimations().map(function (a) { \
             return a.effect.target.id + a.effect.pseudoElement; }).sort().join(' ')); \
           log.push(main.getAnimations({ subtree: true }).length + '/' + html.getAnimations().length); }); \
         t.finished.then(function () { log.push(document.getAnimations().length); }); null",
    )
    .unwrap();
    settle_animations(&rt);
    let got = log(&rt);
    assert!(got.starts_with("main::view-transition-group(inner) main::view-transition-group(outer)"), "{got}");
    assert!(got.ends_with(",0"), "{got}");
    assert!(got.contains("main::view-transition-old(outer)") && !got.contains("(root)"), "{got}");
}

/// `animation: unset` on the group and image pair leaves only the fade animations of the old and
/// new images (WPT `scoped/capture.html`).
#[test]
fn page_animation_unset_removes_the_ua_group_animation_of_a_scoped_tree() {
    let rt = scoped_rt(&format!(
        "{NAMES_CSS} ::view-transition-group(*), ::view-transition-image-pair(*) {{ animation: unset; }}          ::view-transition-old(*) {{ animation: -ua-view-transition-fade-out 1s paused; }}          ::view-transition-new(*) {{ animation: -ua-view-transition-fade-in 1s paused; }}"
    ));
    rt.eval(
        "main.startViewTransition().ready.then(function () {            log.push(main.getAnimations({ subtree: true }).map(function (a) {              return a.animationName.replace('-ua-view-transition-', '') + '@' + a.effect.pseudoElement; }).join(' ')); }); null",
    )
    .unwrap();
    settle(&rt);
    assert_eq!(
        log(&rt),
        "fade-out@::view-transition-old(outer) fade-in@::view-transition-new(outer) fade-out@::view-transition-old(inner) fade-in@::view-transition-new(inner)"
    );
}
