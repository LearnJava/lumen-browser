//! MediaRecorder API stub (W3C MediaStream Recording L2).
//!
//! Phase 0: `new MediaRecorder(stream, opts?)`, state machine
//! (inactive/recording/paused), `mimeType` reflection, events
//! `onstart`/`onstop`/`onpause`/`onresume`/`onerror`/`ondataavailable`
//! (fires empty Blob on stop). `BlobEvent` class.
//! `MediaRecorder.isTypeSupported()` → false.

use rquickjs::Ctx;

/// Install the MediaRecorder API stub into the JS context.
pub fn init_media_stream_recording(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(MEDIA_STREAM_RECORDING_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C MediaStream Recording L2 (Phase 0).
const MEDIA_STREAM_RECORDING_SHIM: &str = r#"(function() {
  'use strict';

  // ── BlobEvent ──────────────────────────────────────────────────────────────
  // W3C MediaStream Recording §4.2
  function BlobEvent(type, init) {
    if (typeof Event === 'function') {
      Event.call(this, type, init);
    }
    this.type = String(type || '');
    this.bubbles = !!(init && init.bubbles);
    this.cancelable = !!(init && init.cancelable);
    init = init || {};
    this.data = (init.data instanceof Blob) ? init.data : new Blob([]);
    this.timecode = (typeof init.timecode === 'number') ? init.timecode : 0;
  }
  if (typeof Event === 'function') {
    BlobEvent.prototype = Object.create(Event.prototype);
  }
  BlobEvent.prototype.constructor = BlobEvent;

  // ── MediaRecorder ──────────────────────────────────────────────────────────
  // W3C MediaStream Recording §4.1
  function MediaRecorder(stream, options) {
    if (!stream) {
      throw new TypeError('MediaRecorder: stream argument is required');
    }
    options = options || {};

    // §4.1.1 mimeType
    this.mimeType = (typeof options.mimeType === 'string') ? options.mimeType : '';

    // §4.1.2 state — inactive | recording | paused
    this.state = 'inactive';

    this.stream = stream;
    this._timeslice = undefined;
    this._chunks = [];

    // Event handler attributes
    this.ondataavailable = null;
    this.onerror = null;
    this.onpause = null;
    this.onresume = null;
    this.onstart = null;
    this.onstop = null;

    this._listeners = {};
  }

  // addEventListener / removeEventListener / dispatchEvent (minimal)
  MediaRecorder.prototype.addEventListener = function(type, listener) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(listener);
  };
  MediaRecorder.prototype.removeEventListener = function(type, listener) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(l) { return l !== listener; });
  };
  MediaRecorder.prototype._dispatch = function(evt) {
    var type = evt.type;
    var handler = this['on' + type];
    if (typeof handler === 'function') { try { handler.call(this, evt); } catch(e) {} }
    var listeners = this._listeners[type];
    if (listeners) {
      listeners.forEach(function(l) { try { l(evt); } catch(e) {} });
    }
  };

  // §4.1.4 start(timeslice?)
  MediaRecorder.prototype.start = function(timeslice) {
    if (this.state !== 'inactive') {
      throw new DOMException('MediaRecorder.start: already active', 'InvalidStateError');
    }
    this.state = 'recording';
    this._timeslice = (typeof timeslice === 'number') ? timeslice : undefined;
    this._chunks = [];
    var evt = { type: 'start', target: this };
    this._dispatch(evt);
  };

  // §4.1.5 stop()
  MediaRecorder.prototype.stop = function() {
    if (this.state === 'inactive') {
      throw new DOMException('MediaRecorder.stop: not recording', 'InvalidStateError');
    }
    this.state = 'inactive';
    // Fire ondataavailable with an empty Blob (Phase 0: no real audio/video data)
    var blob = new Blob(this._chunks, { type: this.mimeType || 'application/octet-stream' });
    var dataEvt = new BlobEvent('dataavailable', { data: blob, timecode: Date.now() });
    this._dispatch(dataEvt);
    this._chunks = [];
    var stopEvt = { type: 'stop', target: this };
    this._dispatch(stopEvt);
  };

  // §4.1.6 pause()
  MediaRecorder.prototype.pause = function() {
    if (this.state !== 'recording') {
      throw new DOMException('MediaRecorder.pause: not recording', 'InvalidStateError');
    }
    this.state = 'paused';
    var evt = { type: 'pause', target: this };
    this._dispatch(evt);
  };

  // §4.1.7 resume()
  MediaRecorder.prototype.resume = function() {
    if (this.state !== 'paused') {
      throw new DOMException('MediaRecorder.resume: not paused', 'InvalidStateError');
    }
    this.state = 'recording';
    var evt = { type: 'resume', target: this };
    this._dispatch(evt);
  };

  // §4.1.8 requestData() — flush collected data as ondataavailable
  MediaRecorder.prototype.requestData = function() {
    if (this.state === 'inactive') {
      throw new DOMException('MediaRecorder.requestData: inactive', 'InvalidStateError');
    }
    var blob = new Blob(this._chunks, { type: this.mimeType || 'application/octet-stream' });
    this._chunks = [];
    var dataEvt = new BlobEvent('dataavailable', { data: blob, timecode: Date.now() });
    this._dispatch(dataEvt);
  };

  // §4.1.3 static isTypeSupported(mimeType) → false (Phase 0: no codec support)
  MediaRecorder.isTypeSupported = function(mimeType) {
    return false;
  };

  // Export to global scope
  globalThis.BlobEvent = BlobEvent;
  globalThis.MediaRecorder = MediaRecorder;
})();"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn install_stubs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            "globalThis.Blob = function(parts, opts) { \
               this.parts = parts || []; \
               this.type = (opts && opts.type) || ''; \
               this.size = 0; \
             }; \
             globalThis.DOMException = function(msg, name) { \
               this.message = msg; this.name = name; \
             }; \
             globalThis.Date = { now: function() { return 0; } };",
        )
        .expect("install stubs");
    }

    #[test]
    fn media_recorder_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval("typeof MediaRecorder === 'function' ? 'yes' : 'no'")
                .expect("eval");
            assert_eq!(res, "yes");
        });
    }

    #[test]
    fn blob_event_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval("typeof BlobEvent === 'function' ? 'yes' : 'no'")
                .expect("eval");
            assert_eq!(res, "yes");
        });
    }

    #[test]
    fn is_type_supported_returns_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: bool = ctx
                .eval("MediaRecorder.isTypeSupported('video/webm')")
                .expect("eval");
            assert!(!res);
        });
    }

    #[test]
    fn initial_state_is_inactive() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval("var r = new MediaRecorder({}); r.state")
                .expect("eval");
            assert_eq!(res, "inactive");
        });
    }

    #[test]
    fn start_changes_state_to_recording() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval("var r = new MediaRecorder({}); r.start(); r.state")
                .expect("eval");
            assert_eq!(res, "recording");
        });
    }

    #[test]
    fn stop_fires_dataavailable_and_stop_events() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval(
                    "var r = new MediaRecorder({}); \
                     var got = []; \
                     r.ondataavailable = function(e) { got.push('data:' + (e.data instanceof Blob ? 'blob' : 'bad')); }; \
                     r.onstop = function() { got.push('stop'); }; \
                     r.start(); r.stop(); got.join(',');"
                )
                .expect("eval");
            assert_eq!(res, "data:blob,stop");
        });
    }

    #[test]
    fn pause_and_resume_state_transitions() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval(
                    "var r = new MediaRecorder({}); \
                     r.start(); \
                     r.pause(); \
                     var s1 = r.state; \
                     r.resume(); \
                     var s2 = r.state; \
                     s1 + ',' + s2"
                )
                .expect("eval");
            assert_eq!(res, "paused,recording");
        });
    }

    #[test]
    fn mime_type_reflected_from_options() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_media_stream_recording(&ctx).expect("init");
            let res: String = ctx
                .eval("var r = new MediaRecorder({}, {mimeType: 'video/webm'}); r.mimeType")
                .expect("eval");
            assert_eq!(res, "video/webm");
        });
    }
}
