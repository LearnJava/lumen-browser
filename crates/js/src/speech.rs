//! Web Speech API (W3C Web Speech API §3–4).
//!
//! Implements:
//! - `window.speechSynthesis` — `SpeechSynthesis` object with `speak/cancel/pause/resume/getVoices`.
//! - `SpeechSynthesisUtterance` — text + properties + events (`start/end/error/pause/resume`).
//! - `SpeechSynthesisVoice` — one synthetic voice "Lumen Voice" (en-US, localService=true).
//! - `SpeechSynthesisEvent` / `SpeechSynthesisErrorEvent` — constructible `Event` subclasses;
//!   every event an utterance delivers is an instance of one of them (BUG-680).
//! - `window.SpeechRecognition` / `window.webkitSpeechRecognition` — stub that always rejects
//!   with `service-not-allowed` (no ML model bundled in Phase 0).
//!
//! Platform TTS backend (synthesis only):
//! - **Windows 10+**: `System.Speech.Synthesis.SpeechSynthesizer` via PowerShell (SAPI 5).
//!   Text is passed through an env-var to avoid shell-injection.
//! - **Linux**: `espeak`, fallback to `spd-say`. If neither is installed, silently no-ops.
//! - **macOS**: `say` (built-in system command).
//! - **Other**: silent no-op.
//!
//! All TTS calls are fire-and-forget: a background thread is spawned, the JS event loop
//! continues immediately.  Events (`start`, `end`) fire with estimated timing based on
//! text length / utterance rate.

/// Speak `text` using the platform TTS engine, fire-and-forget.
///
/// The background thread is detached — Lumen does not wait for completion.
/// Errors (engine not installed, permission denied, etc.) are silently swallowed.
#[cfg(feature = "v8-backend")]
fn platform_speak_async(text: String) {
    std::thread::Builder::new()
        .name("lumen-tts".into())
        .spawn(move || platform_speak_blocking(&text))
        .ok();
}

/// Blocking platform TTS call — run on a background thread.
#[cfg(all(feature = "v8-backend", target_os = "windows"))]
fn platform_speak_blocking(text: &str) {
    // Pass text through an env-var to avoid any PowerShell quoting/injection issues.
    let _ = std::process::Command::new("powershell")
        .env("LUMEN_TTS_TEXT", text)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Speak($env:LUMEN_TTS_TEXT)",
        ])
        .status();
}

#[cfg(all(feature = "v8-backend", target_os = "linux"))]
fn platform_speak_blocking(text: &str) {
    // Try espeak first; fall back to spd-say if not found.
    let ok = std::process::Command::new("espeak")
        .arg(text)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        let _ = std::process::Command::new("spd-say").arg(text).status();
    }
}

#[cfg(all(feature = "v8-backend", target_os = "macos"))]
fn platform_speak_blocking(text: &str) {
    let _ = std::process::Command::new("say").arg(text).status();
}

#[cfg(all(feature = "v8-backend", not(any(target_os = "windows", target_os = "linux", target_os = "macos"))))]
fn platform_speak_blocking(_text: &str) {}

/// V8 port of the former rquickjs `install_speech_bindings` (Ph3 V8 migration S5-S7
/// batch 2, rquickjs side removed in S12b-B5): the native goes through the compat
/// layer, the JS shim evaluates unchanged.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_speech_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn1;
    use lumen_core::ext::JsRuntime as _;

    let native = into_v8_fn1(move |text: String| {
        platform_speak_async(text);
    });
    rt.register_native("_lumen_speech_speak", native)?;
    rt.eval(SPEECH_SHIM)?;
    Ok(())
}

/// JavaScript shim: Web Speech API (W3C Web Speech §3–4).
#[cfg(feature = "v8-backend")]
const SPEECH_SHIM: &str = r#"(function() {
'use strict';

// ── SpeechSynthesisVoice ──────────────────────────────────────────────────────
function SpeechSynthesisVoice(name, lang, localService) {
    this.voiceURI     = 'urn:' + name;
    this.name         = name;
    this.lang         = lang;
    this.localService = !!localService;
    this.default      = false;
}

var _voices = [
    new SpeechSynthesisVoice('Lumen Voice', 'en-US', true)
];
_voices[0].default = true;

// ── SpeechSynthesisUtterance ──────────────────────────────────────────────────
function SpeechSynthesisUtterance(text) {
    this.text    = (text !== undefined) ? String(text) : '';
    this.lang    = '';
    this.voice   = null;
    this.volume  = 1;
    this.rate    = 1;
    this.pitch   = 1;
    this.onstart    = null;
    this.onend      = null;
    this.onerror    = null;
    this.onpause    = null;
    this.onresume   = null;
    this.onmark     = null;
    this.onboundary = null;
    this._listeners = {};
}

SpeechSynthesisUtterance.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
SpeechSynthesisUtterance.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
};
SpeechSynthesisUtterance.prototype._fire = function(type, extra) {
    // BUG-680: a delivered event is a real SpeechSynthesis(Error)Event, not a
    // plain literal, so `instanceof` and the Event methods work on it.
    var init = { utterance: this };
    if (extra) {
        for (var k in extra) init[k] = extra[k];
    }
    var ev;
    if (type === 'error') {
        if (init.error === undefined) init.error = 'synthesis-failed';
        ev = new SpeechSynthesisErrorEvent(type, init);
    } else {
        ev = new SpeechSynthesisEvent(type, init);
    }
    ev.isTrusted     = true;
    ev.target        = this;
    ev.currentTarget = this;
    ev.eventPhase    = 2;
    var handler = this['on' + type];
    if (typeof handler === 'function') { try { handler.call(this, ev); } catch (_) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_); } }
    var ls = this._listeners[type];
    if (ls) ls.forEach(function(f) { try { f(ev); } catch (_) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_); } });
    ev.currentTarget = null;
    ev.eventPhase    = 0;
};

// ── SpeechSynthesisEvent / SpeechSynthesisErrorEvent (Web Speech API §4.2.6–4.2.7) ──
// Both dictionaries have required members, so a missing `utterance` (and, for
// the error event, a missing or non-enum `error`) is a WebIDL TypeError.
var _SYNTH_ERROR_CODES = ['canceled', 'interrupted', 'audio-busy', 'audio-hardware',
    'network', 'synthesis-unavailable', 'synthesis-failed', 'language-unavailable',
    'voice-unavailable', 'text-too-long', 'invalid-argument', 'not-allowed'];

function _synthEventInit(ctor, args) {
    if (args.length < 2) {
        throw new TypeError("Failed to construct '" + ctor + "': 2 arguments required, but only " + args.length + ' present.');
    }
    var init = args[1];
    if (init === undefined || init === null) init = {};
    if (typeof init !== 'object' && typeof init !== 'function') {
        throw new TypeError("Failed to construct '" + ctor + "': The provided value is not of type '" + ctor + "Init'.");
    }
    if (!(init.utterance instanceof SpeechSynthesisUtterance)) {
        throw new TypeError("Failed to construct '" + ctor + "': required member utterance is " +
            (init.utterance === undefined ? 'undefined' : "not of type 'SpeechSynthesisUtterance'") + '.');
    }
    var elapsed = init.elapsedTime === undefined ? 0 : Math.fround(Number(init.elapsedTime));
    if (!isFinite(elapsed)) {
        throw new TypeError("Failed to construct '" + ctor + "': The provided float value is non-finite.");
    }
    return {
        utterance:   init.utterance,
        charIndex:   init.charIndex  === undefined ? 0 : Number(init.charIndex)  >>> 0,
        charLength:  init.charLength === undefined ? 0 : Number(init.charLength) >>> 0,
        elapsedTime: elapsed,
        name:        init.name === undefined ? '' : String(init.name),
        raw:         init
    };
}

class SpeechSynthesisEvent extends Event {
    constructor(type, eventInitDict) {
        var d = _synthEventInit(new.target.name, arguments);
        super(type, d.raw);
        Object.defineProperty(this, '_ss', { value: d });
    }
    get utterance()   { return this._ss.utterance; }
    get charIndex()   { return this._ss.charIndex; }
    get charLength()  { return this._ss.charLength; }
    get elapsedTime() { return this._ss.elapsedTime; }
    get name()        { return this._ss.name; }
}

class SpeechSynthesisErrorEvent extends SpeechSynthesisEvent {
    constructor(type, eventInitDict) {
        super(type, eventInitDict);
        var err = this._ss.raw.error;
        if (err === undefined) {
            throw new TypeError("Failed to construct 'SpeechSynthesisErrorEvent': required member error is undefined.");
        }
        err = String(err);
        if (_SYNTH_ERROR_CODES.indexOf(err) < 0) {
            throw new TypeError("Failed to construct 'SpeechSynthesisErrorEvent': The provided value '" + err +
                "' is not a valid enum value of type SpeechSynthesisErrorCode.");
        }
        Object.defineProperty(this, '_ssError', { value: err });
    }
    get error() { return this._ssError; }
}

// ── SpeechSynthesis (singleton) ───────────────────────────────────────────────
var _queue    = [];
var _speaking = false;
var _paused   = false;

function _processNext() {
    if (_queue.length === 0) { _speaking = false; return; }
    _speaking = true;
    var utt  = _queue[0];
    var rate = (typeof utt.rate === 'number' && utt.rate > 0) ? utt.rate : 1;

    // Fire start event synchronously so event-order matches browser behaviour.
    utt._fire('start');

    // Dispatch to the platform TTS engine (fire-and-forget).
    // Only the text is forwarded — rate/pitch are controlled by the utterance
    // timing estimate above; the OS engine uses its own defaults.
    if (typeof _lumen_speech_speak === 'function') {
        try { _lumen_speech_speak(utt.text); } catch(_) {}
    }

    // Estimate speaking duration from text length and rate.
    // ~14 characters/second at rate=1 is a rough SAPI/espeak average.
    var estMs = Math.max(50, Math.round(utt.text.length / 14 / rate * 1000));
    setTimeout(function() {
        if (_queue.length > 0 && _queue[0] === utt) _queue.shift();
        utt._fire('end');
        _processNext();
    }, estMs);
}

var speechSynthesis = {
    get pending()  { return _queue.length > 1 || (_queue.length === 1 && _speaking); },
    get speaking() { return _speaking; },
    get paused()   { return _paused; },

    speak: function(utt) {
        if (!(utt instanceof SpeechSynthesisUtterance)) return;
        _queue.push(utt);
        if (!_speaking) _processNext();
    },

    cancel: function() {
        _queue = [];
        _speaking = false;
        _paused   = false;
    },

    pause:  function() { _paused = true; },
    resume: function() { _paused = false; },

    getVoices: function() { return _voices.slice(); },

    onvoiceschanged: null,

    addEventListener: function(type, fn) {
        if (type === 'voiceschanged') {
            var prev = this.onvoiceschanged;
            this.onvoiceschanged = prev ? function(e) { prev(e); fn(e); } : fn;
        }
    },
    removeEventListener: function() {}
};

// Fire voiceschanged once after the current microtask drains so that
// code calling getVoices() in the handler sees the populated list.
if (typeof setTimeout === 'function') {
    setTimeout(function() {
        if (typeof speechSynthesis.onvoiceschanged === 'function') {
            speechSynthesis.onvoiceschanged({ type: 'voiceschanged' });
        }
    }, 0);
}

// ── SpeechRecognition stub ────────────────────────────────────────────────────
// No ML model is bundled in Phase 0.  All recognition requests immediately
// fail with service-not-allowed, matching the behaviour of browsers with no
// permission or no microphone access.
function SpeechRecognition() {
    this.lang              = '';
    this.continuous        = false;
    this.interimResults    = false;
    this.maxAlternatives   = 1;
    this.serviceURI        = '';
    this.onstart           = null;
    this.onend             = null;
    this.onerror           = null;
    this.onresult          = null;
    this.onnomatch         = null;
    this.onspeechstart     = null;
    this.onspeechend       = null;
    this.onaudiostart      = null;
    this.onaudioend        = null;
    this._listeners        = {};
}

SpeechRecognition.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
SpeechRecognition.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
};
SpeechRecognition.prototype._fire = function(type, extra) {
    var ev = Object.assign({ type: type }, extra || {});
    var handler = this['on' + type];
    if (typeof handler === 'function') { try { handler.call(this, ev); } catch (_) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_); } }
    var ls = this._listeners[type];
    if (ls) ls.forEach(function(f) { try { f(ev); } catch (_) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(_); } });
};
SpeechRecognition.prototype.start = function() {
    var self = this;
    if (typeof setTimeout === 'function') {
        setTimeout(function() {
            self._fire('error', {
                error: 'service-not-allowed',
                message: 'Speech recognition not available in Lumen (Phase 0)'
            });
            self._fire('end');
        }, 0);
    }
};
SpeechRecognition.prototype.stop  = function() {};
SpeechRecognition.prototype.abort = function() {};

// ── Export to globals ─────────────────────────────────────────────────────────
// In Lumen's QuickJS environment `window` is a plain JS object defined by
// the DOM shim (`var window = { ... }` at top-level).  All Web API globals
// live on it (Blob, Worker, etc.).  We follow the same convention.
if (typeof window !== 'undefined') {
    window.SpeechSynthesisUtterance = SpeechSynthesisUtterance;
    window.SpeechSynthesisVoice     = SpeechSynthesisVoice;
    window.SpeechSynthesisEvent      = SpeechSynthesisEvent;
    window.SpeechSynthesisErrorEvent = SpeechSynthesisErrorEvent;
    window.speechSynthesis          = speechSynthesis;
    window.SpeechRecognition        = SpeechRecognition;
    window.webkitSpeechRecognition  = SpeechRecognition;
}
// Also expose on globalThis so Worker contexts and bare-name access work.
globalThis.SpeechSynthesisUtterance = SpeechSynthesisUtterance;
globalThis.SpeechSynthesisVoice     = SpeechSynthesisVoice;
globalThis.SpeechSynthesisEvent      = SpeechSynthesisEvent;
globalThis.SpeechSynthesisErrorEvent = SpeechSynthesisErrorEvent;
globalThis.speechSynthesis          = speechSynthesis;
globalThis.SpeechRecognition        = SpeechRecognition;
globalThis.webkitSpeechRecognition  = SpeechRecognition;

})();
"#;

