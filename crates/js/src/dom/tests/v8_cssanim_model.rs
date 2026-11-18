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
fn seeking_moves_the_clock_the_overlay_reads() {    let rt = rt();
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

// CSSANIM-MODEL-2 срез S4: the frame loop's scheduler paints `opacity`/`transform`/colours/
// `height` and writes them into the computed-style snapshot, so `cancel()`/`play()`/a seek
// has to reach it — the queue is what the shell drains before its tick.
#[test]
fn cancel_play_and_seek_queue_commands_for_the_frame_loop() {
    use crate::CssMotionCmd;
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0];
        a.cancel();
        a.currentTime = 5000;
        a.cancel();
        a.play();
        'ok'"
    );
    assert_eq!(eval_str(&rt, &script), "ok");
    let cmds = rt.take_css_motion_cmds();
    let shape: Vec<String> = cmds
        .iter()
        .map(|c| match c {
            CssMotionCmd::Cancel { node, name } if *node == 1 && name == "k" => "cancel k".to_string(),
            CssMotionCmd::Restore { node, name } if *node == 1 && name == "k" => "restore k".to_string(),
            CssMotionCmd::Seek { node, name, current_time_ms } if *node == 1 && name == "k" => {
                format!("seek k {current_time_ms}")
            }
            other => format!("unexpected {other:?}"),
        })
        .collect();
    // `cancel` → the seek revives the object (restore) and lands at 5 s → `cancel` again →
    // `play` revives it (restore) and restarts it from 0.
    assert_eq!(
        shape.join(" | "),
        "cancel k | restore k | seek k 5000 | cancel k | restore k | seek k 0"
    );
}

// CSSANIM-MODEL-2 срез S5: a transition the frame loop's `TransitionScheduler` drives is
// cancelled and seeked through the same queue, keyed by its kebab-case property.
#[test]
fn a_scheduler_driven_transition_queues_its_seek_and_cancel() {
    use crate::CssMotionCmd;
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        var t = _css_tr_from_event(1, 'opacity'), r = [t instanceof CSSTransition];
        t.currentTime = 5000;
        t.cancel();
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "true");
    let cmds = rt.take_css_motion_cmds();
    let shape: Vec<String> = cmds
        .iter()
        .map(|c| match c {
            CssMotionCmd::TransitionSeek { node, property, current_time_ms }
                if *node == 1 && property == "opacity" =>
            {
                format!("seek opacity {current_time_ms}")
            }
            CssMotionCmd::TransitionCancel { node, property }
                if *node == 1 && property == "opacity" =>
            {
                "cancel opacity".to_string()
            }
            other => format!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(shape.join(" | "), "seek opacity 5000 | cancel opacity");
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
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '30px');
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
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '30px');
        var t = __sync()[0], r = [t.playState];
        __vals['transition-property'] = 'none';
        _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '30px');
        r.push(t.playState, __sync().length);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "running idle 0");
}

/// CSSANIM-MODEL-2-S6: an element that stops generating a box (`display: none` on it or on an
/// ancestor) runs no transition (`CSSAnimation-canceling`/`CSSTransition-canceling`
/// «Setting display:none …»). The settled value does not change when the box goes away, so only
/// this branch of `_wa_gap_tr_value` can notice: the run is cancelled, the read answers the
/// settled value and the object leaves the list.
#[test]
fn a_transition_is_cancelled_when_the_element_stops_generating_a_box() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '30px');
        var t = __sync()[0], r = [t.playState];
        __tick(1000);
        r.push(_wa_gap_tr_value(1, null, 'margin-left', '30px'));
        _css_boxless = function() {{ return true; }};
        r.push(t.playState, _wa_gap_tr_value(1, null, 'margin-left', '30px'), __sync().length);
        r.join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "running 12px idle 30px 0");
}

/// CSSANIM-MODEL-2-S1: a seek over a phase boundary fires the CSS event the scheduler would have
/// fired, and the scheduler's own event for that animation is dropped (it never saw the seek).
#[test]
fn a_scripted_seek_dispatches_the_animation_events_it_crosses() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], got = [];
        _lumen_dispatch = function(nid, ev) {{ got.push(ev.type + ' ' + ev.animationName + ' ' + ev.elapsedTime); }};
        a.currentTime = 5000;
        __tick(16); _lumen_run_raf_callbacks(16);
        a.currentTime = 10000;
        __tick(32); _lumen_run_raf_callbacks(32);
        _lumen_deliver_animation_events([[1, 'start', 'k', 0], [1, 'end', 'k', 10], [1, 'start', 'j', 0]]);
        got.join('|')"
    );
    // One start into the active interval, one end out of it; the scheduler's `k` events are gone,
    // its `j` (an animation no script drives) is not.
    assert_eq!(
        eval_str(&rt, &script),
        "animationstart k 0|animationend k 10|animationstart j 0"
    );
}

/// The same for a transition: the shell's `TransitionScheduler` drives none of its properties, so
/// before this slice a seek of a transition fired nothing at all.
#[test]
fn a_scripted_seek_dispatches_the_transition_events_it_crosses() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        __vals['margin-left'] = '10px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '10px');
        __vals['margin-left'] = '30px'; _wa_gap_tr_clock_ms = null; _wa_gap_tr_value(1, null, 'margin-left', '30px');
        var t = __sync()[0], got = [];
        _lumen_dispatch = function(nid, ev) {{ got.push(ev.type + ' ' + ev.propertyName + ' ' + ev.elapsedTime + ' ' + ev.cancelable); }};
        t.currentTime = 5000;
        __tick(16); _lumen_run_raf_callbacks(16);
        t.currentTime = 10000;
        __tick(32); _lumen_run_raf_callbacks(32);
        got.join('|')"
    );
    assert_eq!(
        eval_str(&rt, &script),
        "transitionstart margin-left 0 false|transitionend margin-left 10 true"
    );
}

/// CSSANIM-MODEL-2-S2: a replaced effect paints its own keyframes on the animation's clock, and
/// the `@keyframes` its `animation-name` still matches stop applying.
#[test]
fn a_replaced_effect_paints_its_own_keyframes_and_drops_the_css_ones() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], r = [];
        a.pause();
        a.currentTime = 5000;
        a.effect = new KeyframeEffect(__el, {{ left: ['0px', '100px'] }}, 10000);
        r.push(_wa_gap_an_value(1, 'margin-left', '0px', '0px'));
        r.push(_wa_gap_an_value(1, 'left', 'auto', 'auto'));
        r.push(a.playState);
        r.join(' ')"
    );
    // Half-way through the new 10 s effect: left 50px on the target, margin-left back to base.
    assert_eq!(eval_str(&rt, &script), "0px 50px paused");
}

/// CSSANIM-MODEL-2-S2: `effect = null` finishes the animation, the target returns to its settled
/// style and the `animationend` a script now owns is dispatched.
#[test]
fn a_null_effect_finishes_the_animation_and_fires_its_end() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var a = __sync()[0], got = [];
        a.currentTime = 5000;
        __tick(16); _lumen_run_raf_callbacks(16);
        _lumen_dispatch = function(nid, ev) {{ got.push(nid + ' ' + ev.type); }};
        a.effect = null;
        __tick(32); _lumen_run_raf_callbacks(32);
        [a.playState, _wa_gap_an_value(1, 'margin-left', '0px', '0px'), got.join('|')].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "finished 0px 1 animationend");
}

/// CSSANIM-MODEL-2-S2: a play-pending animation whose effect moves to another element still
/// starts at its original element (the event is the CSS animation's, not the effect's).
#[test]
fn a_replaced_effect_starts_at_the_original_element() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var d2 = {{ __nid__: 2 }}, got = [];
        _lumen_make_element = function(nid) {{ return nid === 2 ? d2 : __el; }};
        _lumen_dispatch = function(nid, ev) {{ got.push(nid + ' ' + ev.type); }};
        var a = __sync()[0];
        a.effect = new KeyframeEffect(d2, {{ left: ['0px', '100px'] }}, 10000);
        __tick(16); _lumen_run_raf_callbacks(16);
        // The start is queued on the first frame as a task; the next frame's «update animations
        // and send events» dispatches it, exactly as the shell's scheduler event would arrive.
        __tick(32); _lumen_run_raf_callbacks(32);
        [String(a.effect.target === d2), got.join('|')].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "true 1 animationstart");
}

/// CSSANIM-MODEL-2-S2: `finish()` from before the active interval crosses it in one jump and
/// dispatches both boundaries.
#[test]
fn finish_dispatches_the_boundaries_it_jumps_over() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k');
        var got = [];
        _lumen_dispatch = function(nid, ev) {{ got.push(ev.type + ' ' + ev.elapsedTime); }};
        var a = __sync()[0];
        a.finish();
        __tick(16); _lumen_run_raf_callbacks(16);
        got.join('|')"
    );
    assert_eq!(eval_str(&rt, &script), "animationstart 0|animationend 10");
}

/// CSSANIM-MODEL-2-S2: `animation-timeline: none` detaches the animation — `timeline` is `null`.
#[test]
fn animation_timeline_none_detaches_the_animation() {
    let rt = rt();
    let script = format!(
        "{PRELUDE}
        __css('k', {{ 'timeline': 'none' }});
        var a = __sync()[0];
        [String(a.timeline), a.timeline === null].join(' ')"
    );
    assert_eq!(eval_str(&rt, &script), "null true");
}

/// CSSANIM-MODEL-2: a property the page never read transitions too — the element's first
/// observation snapshots the whole computed style, and the next style recalc compares against
/// that baseline (`_wa_motion_seed`). Real cascade here: only the stubbed harness above keeps its
/// computed style in `__vals`, and the bindings that read it cannot be replaced from a script.
#[test]
fn a_property_nobody_read_transitions_from_the_first_observed_style() {
    let rt = super::v8_bug935_s55_content_journal::runtime(
        super::v8_bug935_s55_content_journal::page(),
    );
    let script = "(function() {
        var d = document.createElement('div');
        document.body.appendChild(d);
        d.style.left = '0px'; d.style.top = '0px';
        getComputedStyle(d).transitionProperty;
        d.style.transition = 'all 10s linear';
        d.style.left = '100px'; d.style.top = '100px';
        var t = d.getAnimations();
        return [t.length, t[0].transitionProperty, t[1].transitionProperty].join(' ');
    })()";
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}")) {
        lumen_core::JsValue::String(s) => assert_eq!(s, "2 left top"),
        other => panic!("expected a string, got {other:?}"),
    }
}

/// CSSANIM-MODEL-3 срез 2: a transition the page never read. Its before-change style lives in
/// the published snapshot only until the next recompute replaces it, so `document.getAnimations()`
/// seeds the model from the entries the flush kept for the nodes whose style names a transition
/// (`_lumen_style_change_candidates`). Without that the child's `left` change in WPT
/// `css-transitions/Document-getAnimations` («CSS Transitions targetting (pseudo-)elements …»)
/// never becomes a transition, even though its `transition-property` asks for one.
#[test]
fn a_transition_on_an_element_nobody_read_starts_at_a_document_style_change_event() {
    let rt = super::v8_bug935_s55_content_journal::runtime(
        super::v8_bug935_s55_content_journal::page(),
    );
    let script = "(function() {
        var parent = document.createElement('div');
        var child = document.createElement('div');
        parent.appendChild(child);
        document.body.appendChild(parent);
        child.style.left = '0px';
        child.style.transition = 'left 100s';
        getComputedStyle(parent).left;
        child.style.left = '100px';
        var a = document.getAnimations();
        return [a.length, a.length ? a[0].transitionProperty : '',
                a.length ? String(a[0].effect.target === child) : ''].join(' ');
    })()";
    assert_eq!(eval_str(&rt, script), "1 left true");
}

/// CSSANIM-MODEL-3: `::marker` is one of the pseudo-elements the animation model looks at, and
/// it sorts before `::before`/`::after` (CSS Pseudo-Elements L4; WPT `Document-getAnimations`
/// «(::marker)» subtests). `document.getAnimations()` ranks a pseudo-element by the animation's
/// *owning* one, not by `effect.target` — a script may move an effect elsewhere and the order
/// must not follow it (WPT «pseudo element with replaced target does not affect animation
/// ordering»).
#[test]
fn a_marker_sorts_between_the_element_and_the_other_pseudo_elements() {
    let rt = rt();
    let script = PRELUDE.to_string() + r#"
        var mk = new Animation(new KeyframeEffect(__el, [], 10000), document.timeline);
        var el = new Animation(new KeyframeEffect(__el, [], 10000), document.timeline);
        var af = new Animation(new KeyframeEffect(__el, [], 10000), document.timeline);
        mk.effect.pseudoElement = '::marker';
        af.effect.pseudoElement = '::after';
        mk.play(); el.play(); af.play();
        [_wa_pseudo_targets(__el).join(','),
         _wa_doc_get_animations().map(function(a) { return a.effect.pseudoElement || 'element'; }).join(',')].join(' ')"#;
    assert_eq!(eval_str(&rt, &script), "::marker,::before,::after element,::marker,::after");
}

/// The two keys a transition can live under: the element's own stays `nid:property` (the shell's
/// event lookups read that shape), a pseudo-element's carries the pseudo.
#[test]
fn a_pseudo_element_transition_has_its_own_key() {
    let rt = rt();
    let script = r#"[
        _wa_tr_key(1, null, 'width'),
        _wa_tr_key(1, '::before', 'width')
    ].join(' ')"#
    .to_string();
    // Distinct records, and the element's key is still the prefix of the pseudo's, so the
    // `nid:` scan that finds an element's live transitions keeps finding them.
    assert_eq!(eval_str(&rt, &script), "1:width 1:::before:width");
}

/// CSS Color L4 §4.4: `border-top-color: currentcolor` follows `color` in the computed style,
/// but its specified value did not change — one transition, on `color`, not nine (WPT
/// `Document-getAnimations` «CSS Transitions targetting (pseudo-)elements …» counts them on a
/// `::marker` with `transition: all`).
#[test]
fn a_property_that_follows_color_does_not_transition_with_it() {
    let rt = rt();
    let script = PRELUDE.to_string() + r#"
        __vals['transition-property'] = 'all'; __vals['transition-duration'] = '10s';
        __vals['transition-delay'] = '0s'; __vals['transition-timing-function'] = 'linear';
        __vals['color'] = 'rgb(0, 128, 0)';
        _wa_gap_tr['1:border-top-color'] = { seen: 'rgb(255, 0, 0)', run: null, boxless: false, cur: true };
        _wa_gap_tr['1:color'] = { seen: 'rgb(255, 0, 0)', run: null, boxless: false, cur: false };
        _wa_gap_tr_clock_ms = null;
        var b = _wa_gap_tr_value(1, null, 'border-top-color', 'rgb(0, 128, 0)');
        _wa_gap_tr_clock_ms = null;
        _wa_gap_tr_value(1, null, 'color', 'rgb(0, 128, 0)');
        [b, !!_css_tr_by_key['1:border-top-color'], !!_css_tr_by_key['1:color']].join(' ')"#;
    assert_eq!(eval_str(&rt, &script), "rgb(0, 128, 0) false true");
}

