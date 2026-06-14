//! Web Speech API integration tests (W3C Web Speech §3–4).
//!
//! Verifies that `speechSynthesis`, `SpeechSynthesisUtterance`, `SpeechSynthesisVoice`,
//! `SpeechRecognition`, and `webkitSpeechRecognition` are properly installed in the
//! full DOM environment.

use lumen_core::JsRuntime as _;
use lumen_dom::Document;
use lumen_js::QuickJsRuntime;
use std::sync::{Arc, Mutex};

fn make_rt() -> QuickJsRuntime {
    let rt = QuickJsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &QuickJsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

fn str_eval(rt: &QuickJsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected string from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

fn num_eval(rt: &QuickJsRuntime, script: &str) -> f64 {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Number(n)) => n,
        Ok(other) => panic!("expected number from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

// ── speechSynthesis object ────────────────────────────────────────────────────

#[test]
fn speech_synthesis_is_defined() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "typeof speechSynthesis === 'object' && speechSynthesis !== null"
    ));
}

#[test]
fn speech_synthesis_get_voices_is_function() {
    let rt = make_rt();
    assert_eq!(
        str_eval(&rt, "typeof speechSynthesis.getVoices"),
        "function"
    );
}

#[test]
fn get_voices_returns_at_least_one() {
    let rt = make_rt();
    let n = num_eval(&rt, "speechSynthesis.getVoices().length");
    assert!(n >= 1.0, "expected at least one voice, got {n}");
}

#[test]
fn first_voice_is_default() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "speechSynthesis.getVoices()[0].default === true"
    ));
}

#[test]
fn first_voice_lang_en_us() {
    let rt = make_rt();
    assert_eq!(
        str_eval(&rt, "speechSynthesis.getVoices()[0].lang"),
        "en-US"
    );
}

#[test]
fn first_voice_local_service() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "speechSynthesis.getVoices()[0].localService === true"
    ));
}

#[test]
fn speaking_starts_false() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "speechSynthesis.speaking === false"));
}

#[test]
fn pending_starts_false() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "speechSynthesis.pending === false"));
}

#[test]
fn paused_starts_false() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "speechSynthesis.paused === false"));
}

#[test]
fn speak_cancel_pause_resume_are_functions() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "typeof speechSynthesis.speak === 'function' && \
         typeof speechSynthesis.cancel === 'function' && \
         typeof speechSynthesis.pause === 'function' && \
         typeof speechSynthesis.resume === 'function'"
    ));
}

// ── SpeechSynthesisUtterance ──────────────────────────────────────────────────

#[test]
fn utterance_constructor_exists() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "typeof SpeechSynthesisUtterance === 'function'"));
}

#[test]
fn utterance_text_property() {
    let rt = make_rt();
    assert_eq!(
        str_eval(&rt, "new SpeechSynthesisUtterance('hello').text"),
        "hello"
    );
}

#[test]
fn utterance_defaults() {
    let rt = make_rt();
    // rate=1, pitch=1, volume=1 by spec default.
    assert!(bool_eval(
        &rt,
        "var u = new SpeechSynthesisUtterance('x'); \
         u.rate === 1 && u.pitch === 1 && u.volume === 1"
    ));
}

#[test]
fn utterance_lang_defaults_to_empty() {
    let rt = make_rt();
    assert_eq!(str_eval(&rt, "new SpeechSynthesisUtterance('x').lang"), "");
}

// ── speak() behaviour ─────────────────────────────────────────────────────────

#[test]
fn speak_fires_start_event_synchronously() {
    let rt = make_rt();
    // The start event fires synchronously when speak() is called (spec §3.3).
    assert!(bool_eval(
        &rt,
        "var fired = false; \
         var u = new SpeechSynthesisUtterance('test'); \
         u.onstart = function() { fired = true; }; \
         speechSynthesis.speak(u); \
         fired"
    ));
}

#[test]
fn speak_sets_speaking_true() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var u = new SpeechSynthesisUtterance('hello'); \
         speechSynthesis.speak(u); \
         speechSynthesis.speaking"
    ));
}

#[test]
fn add_event_listener_start_fires() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var fired = false; \
         var u = new SpeechSynthesisUtterance('hi'); \
         u.addEventListener('start', function() { fired = true; }); \
         speechSynthesis.speak(u); \
         fired"
    ));
}

// ── cancel() ─────────────────────────────────────────────────────────────────

#[test]
fn cancel_clears_queue_and_resets_speaking() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var u = new SpeechSynthesisUtterance('hello'); \
         speechSynthesis.speak(u); \
         speechSynthesis.cancel(); \
         !speechSynthesis.speaking && !speechSynthesis.pending"
    ));
}

// ── SpeechRecognition stub ────────────────────────────────────────────────────

#[test]
fn speech_recognition_is_defined() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "typeof SpeechRecognition === 'function'"));
}

#[test]
fn webkit_speech_recognition_alias() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "SpeechRecognition === webkitSpeechRecognition"
    ));
}

#[test]
fn recognition_has_required_methods() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var r = new SpeechRecognition(); \
         typeof r.start === 'function' && \
         typeof r.stop  === 'function' && \
         typeof r.abort === 'function'"
    ));
}

#[test]
fn recognition_has_event_handlers() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var r = new SpeechRecognition(); \
         'onresult' in r && 'onerror' in r && 'onend' in r && 'onstart' in r"
    ));
}

#[test]
fn recognition_default_properties() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var r = new SpeechRecognition(); \
         r.continuous === false && r.interimResults === false && r.maxAlternatives === 1"
    ));
}

// ── SpeechSynthesisVoice ──────────────────────────────────────────────────────

#[test]
fn voice_has_required_properties() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "var v = speechSynthesis.getVoices()[0]; \
         typeof v.name === 'string' && \
         typeof v.lang === 'string' && \
         typeof v.voiceURI === 'string' && \
         typeof v.localService === 'boolean' && \
         typeof v.default === 'boolean'"
    ));
}

// ── voiceschanged callback ────────────────────────────────────────────────────

#[test]
fn voices_changed_listener_registers() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "speechSynthesis.addEventListener('voiceschanged', function() {}); \
         typeof speechSynthesis.onvoiceschanged === 'function'"
    ));
}
