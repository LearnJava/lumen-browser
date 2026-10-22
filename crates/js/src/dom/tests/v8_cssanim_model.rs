//! CSSANIM-MODEL — `CSSAnimation`/`CSSTransition` as script-visible objects (`_css_*` in
//! `web_api_shim_tail_b.js`): the interface, the play-pending state and `ready`, the clock a
//! seek moves, `getAnimations()` relevance and composite order, cancellation by style and by
//! script. The computed style and the clock are replaced, as in `v8_bug1293_motion_overlay`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

/// `__vals` — computed-style snapshot of node 1, `__now` — `performance.now()`, `__kf` — the
/// `@keyframes` of the document. `__sync()` is one «style recalc» of the element.
const PRELUDE: &str = "
var __vals = {}, __now = 0, __kf = {};
_lumen_computed_property = function(nid, name) { return __vals[name] || ''; };
_wa_gap_keyframes_json = function(name) { return __kf[name] === undefined ? null : JSON.stringify(__kf[name]); };
var __el = { __nid__: 1 };
_lumen_make_element = function(nid) { return __el; };
performance.now = function() { return __now; };
function __tick(ms) { __now = ms; _wa_gap_tr_clock_ms = null; }
function __sync() { _wa_gap_tr_clock_ms = null; return _wa_get_animations_for(__el); }
function __css(name, extra) {
  __vals['animation-name'] = name; __vals['animation-duration'] = '10s'; __vals['animation-delay'] = '0s';
  __vals['animation-timing-function'] = 'linear'; __vals['animation-iteration-count'] = '1';
  __vals['animation-direction'] = 'normal'; __vals['animation-fill-mode'] = 'none';
  __vals['animation-play-state'] = 'running';
  for (var k in (extra || {})) __vals['animation-' + k] = extra[k];
}
__kf['k'] = [{ offset: 0, decls: [['margin-left', '10px']] }, { offset: 1, decls: [['margin-left', '30px']] }];
__kf['j'] = [{ offset: 0, decls: [['margin-left', '1px']] }, { offset: 1, decls: [['margin-left', '2px']] }];
";

#[test]
fn css_animation_is_an_animation_interface_with_an_empty_id() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0];
        [typeof CSSAnimation, typeof CSSTransition, a instanceof CSSAnimation, a instanceof Animation,
         Object.prototype.toString.call(a), JSON.stringify(a.id), a.animationName,
         (function() {{ try {{ new CSSAnimation(); }} catch (e) {{ return e.name; }} }})()].join(' ')"
    );
    assert_eq!(
        eval_str(&rt, &script),
        "function function true true [object CSSAnimation] \"\" k TypeError"
    );
}

#[test]
fn a_new_css_animation_is_play_pending_until_the_next_frame() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0];
        var r = [a.pending, a.playState, a.startTime, a.currentTime];
        var ready = a.ready;
        __tick(16); _lumen_run_raf_callbacks(16);
        r.push(a.pending, a.ready === ready, a.startTime !== null);
        r.join(' ')"
    );
    // Pending, running, no start time, current time 0 → after the frame: not pending, same promise.
    assert_eq!(eval_str(&rt, &script), "true running  0 false true true");
}

#[test]
fn a_dropped_animation_name_cancels_the_animation_and_rejects_its_promises() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], out = [];
        a.ready.then(function() {{ out.push('ready'); }}, function(e) {{ out.push('ready:' + e.name); }});
        a.finished.then(function() {{ out.push('fin'); }}, function(e) {{ out.push('fin:' + e.name); }});
        __css('none');
        var left = __sync().length;
        [left, a.playState, a.currentTime, a.pending].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "0 idle  false");
}

#[test]
fn duplicated_names_get_one_object_each_and_the_list_order_is_the_composite_order() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k, k');
        var two = __sync();
        __tick(100); __css('k, j');
        var kj = __sync();
        var k0 = kj[0];
        __tick(200); __css('j, k');
        var jk = __sync();
        [two.length, two[0] !== two[1], kj.map(function(x) {{ return x.animationName; }}).join(),
         jk.map(function(x) {{ return x.animationName; }}).join(), jk[1] === k0].join(' ')"
    );
    // `k` keeps its object when `j` is put in front of it.
    assert_eq!(eval_str(&rt, &script), "2 true k,j j,k true");
}

#[test]
fn a_finished_animation_is_listed_only_while_it_fills() {
    let rt = rt();
    let plain = format!(
        "{PRELUDE}
        __css('k');
        var r = [__sync().length];
        __tick(11000);
        r.push(__sync().length);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &plain), "1 0");
    let rt = self::rt();
    let filling = format!(
        "{PRELUDE}
        __css('k', {{ 'fill-mode': 'forwards' }});
        var r = [__sync().length];
        __tick(16); _lumen_run_raf_callbacks(16);
        __tick(11000);
        var a = __sync();
        r.push(a.length, a[0].playState);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &filling), "1 1 finished");
}

#[test]
fn a_delayed_animation_is_listed_before_it_starts() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k', {{ delay: '5s' }});
        String(__sync().length)"
    );
    assert_eq!(eval_str(&rt, &script), "1");
}

#[test]
fn script_cancel_unlists_and_play_brings_it_back_without_a_second_object() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], r = [];
        a.cancel();
        r.push(__sync().length, a.playState, a.currentTime);
        a.play();
        var back = __sync();
        r.push(back.length, back[0] === a, a.playState, a.pending);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "0 idle  1 true running true");
}

#[test]
fn seeking_moves_the_clock_the_overlay_reads() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], r = [];
        a.pause();
        a.currentTime = 5000;
        __tick(3000);
        r.push(_wa_gap_an_value(1, 'margin-left', '0px', '0px'), a.currentTime, a.playState);
        a.play();
        __tick(4000);
        r.push(a.currentTime, a.playState);
        r.join(' ')"
    );
    // Paused at 5 s → 20px; resumed, a second later it is at 6 s.
    assert_eq!(eval_str(&rt, &script), "20px 5000 paused 6000 running");
}

#[test]
fn a_script_change_of_the_play_state_stands_against_the_style() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], r = [];
        a.pause();
        __css('k', {{ 'play-state': 'paused' }}); __sync();
        __css('k', {{ 'play-state': 'running' }}); __sync();
        r.push(a.playState);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "paused");
}

#[test]
fn setting_current_time_to_null_throws_and_a_finished_animation_holds_its_end() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], r = [];
        try {{ a.currentTime = null; r.push('no throw'); }} catch (e) {{ r.push(e.name); }}
        __tick(16); _lumen_run_raf_callbacks(16);
        __tick(50000);
        r.push(a.currentTime, a.playState);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "TypeError 10000 finished");
}

#[test]
fn a_transition_is_a_css_transition_with_its_property_and_leaves_the_list_when_over() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, 'margin-left', '30px');
        var t = __sync(), r = [t.length];
        r.push(t[0] instanceof CSSTransition, t[0].transitionProperty, t[0].pending, t[0].id === '');
        r.push(t[0].effect.getKeyframes().map(function(k) {{ return k.marginLeft; }}).join('>'));
        __tick(20000);
        r.push(__sync().length);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "1 true margin-left true true 10px>30px 0");
}

#[test]
fn a_transition_is_cancelled_when_the_style_no_longer_asks_for_it() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, 'margin-left', '30px');
        var t = __sync()[0], r = [t.playState];
        __vals['transition-property'] = 'none';
        _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, 'margin-left', '30px');
        r.push(t.playState, __sync().length);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "running idle 0");
}
