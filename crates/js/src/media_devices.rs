//! MediaDevices API (W3C Media Capture and Streams §4).
//!
//! Installs `navigator.mediaDevices` and related interfaces so that pages can
//! probe device capability and attempt capture without JS errors. Phase 0: all
//! capture requests reject with `NotAllowedError`; `enumerateDevices` returns
//! an empty list (no label fingerprinting, ADR-007 Layer 4).
//!
//! Installed interfaces:
//! - `navigator.mediaDevices` — `MediaDevicesInfo` EventTarget
//! - `MediaStream` — empty stream class, `window.MediaStream` exported
//! - `MediaStreamTrack` — track class, `window.MediaStreamTrack` exported
//! - `MediaDeviceInfo` — device descriptor, `window.MediaDeviceInfo` exported
//! - `InputDeviceInfo` (subclass) — `window.InputDeviceInfo` exported

use rquickjs::Ctx;

/// Install MediaDevices API shim into the JS context.
///
/// Adds `navigator.mediaDevices` with all W3C Media Capture §4 methods and
/// exports `MediaStream`, `MediaStreamTrack`, `MediaDeviceInfo`, and
/// `InputDeviceInfo` as globals on `window`. All getUserMedia / getDisplayMedia
/// calls reject with `NotAllowedError` (privacy-first).
///
/// Must be called **after** `install_dom_api` so that `navigator`, `Promise`,
/// `DOMException`, `EventTarget`, and `Event` already exist.
pub fn install_media_devices_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(MEDIA_DEVICES_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the MediaDevices API.
const MEDIA_DEVICES_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // ── MediaStreamTrack ────────────────────────────────────────────────────────
  // Represents a single audio or video track within a MediaStream.
  function MediaStreamTrack(kind, label, id) {
    this.kind        = kind || 'audio';
    this.id          = id   || _lumen_random_uuid();
    this.label       = label || '';
    this.enabled     = true;
    this.muted       = false;
    this.readyState  = 'ended';  // tracks from Phase-0 stubs are always ended
    this._listeners  = {};
  }
  MediaStreamTrack.prototype.stop = function() { this.readyState = 'ended'; };
  MediaStreamTrack.prototype.clone = function() {
    var t = new MediaStreamTrack(this.kind, this.label);
    t.enabled = this.enabled;
    return t;
  };
  MediaStreamTrack.prototype.getCapabilities  = function() { return {}; };
  MediaStreamTrack.prototype.getConstraints   = function() { return {}; };
  MediaStreamTrack.prototype.getSettings      = function() { return {}; };
  MediaStreamTrack.prototype.applyConstraints = function() {
    return Promise.resolve();
  };
  MediaStreamTrack.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  MediaStreamTrack.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };
  MediaStreamTrack.prototype.dispatchEvent = function(evt) {
    var fns = this._listeners[evt.type] || [];
    fns.forEach(function(f) { try { f(evt); } catch(e) {} });
    return true;
  };
  globalThis.MediaStreamTrack = MediaStreamTrack;
  if (typeof window !== 'undefined') window.MediaStreamTrack = MediaStreamTrack;

  // ── MediaStream ─────────────────────────────────────────────────────────────
  // Represents a stream of media content (audio and/or video tracks).
  function MediaStream(tracksOrStream) {
    this.id       = _lumen_random_uuid();
    this.active   = false;
    this._tracks  = [];
    this._listeners = {};
    if (Array.isArray(tracksOrStream)) {
      for (var i = 0; i < tracksOrStream.length; i++) {
        this._tracks.push(tracksOrStream[i]);
      }
      this.active = this._tracks.length > 0;
    } else if (tracksOrStream instanceof MediaStream) {
      var src = tracksOrStream;
      for (var j = 0; j < src._tracks.length; j++) {
        this._tracks.push(src._tracks[j].clone());
      }
      this.active = this._tracks.length > 0;
    }
  }
  MediaStream.prototype.getAudioTracks = function() {
    return this._tracks.filter(function(t) { return t.kind === 'audio'; });
  };
  MediaStream.prototype.getVideoTracks = function() {
    return this._tracks.filter(function(t) { return t.kind === 'video'; });
  };
  MediaStream.prototype.getTracks = function() { return this._tracks.slice(); };
  MediaStream.prototype.getTrackById = function(id) {
    for (var i = 0; i < this._tracks.length; i++) {
      if (this._tracks[i].id === id) return this._tracks[i];
    }
    return null;
  };
  MediaStream.prototype.addTrack = function(track) {
    if (this.getTrackById(track.id)) return;
    this._tracks.push(track);
    this.active = true;
  };
  MediaStream.prototype.removeTrack = function(track) {
    this._tracks = this._tracks.filter(function(t) { return t.id !== track.id; });
    this.active  = this._tracks.length > 0;
  };
  MediaStream.prototype.clone = function() {
    return new MediaStream(this);
  };
  MediaStream.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  MediaStream.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };
  MediaStream.prototype.dispatchEvent = function(evt) {
    var fns = this._listeners[evt.type] || [];
    fns.forEach(function(f) { try { f(evt); } catch(e) {} });
    return true;
  };
  globalThis.MediaStream = MediaStream;
  if (typeof window !== 'undefined') window.MediaStream = MediaStream;

  // ── MediaDeviceInfo ─────────────────────────────────────────────────────────
  // Describes a media input or output device.
  function MediaDeviceInfo(deviceId, groupId, kind, label) {
    this.deviceId = deviceId || '';
    this.groupId  = groupId  || '';
    this.kind     = kind     || 'audioinput';
    this.label    = label    || '';
  }
  MediaDeviceInfo.prototype.toJSON = function() {
    return {
      deviceId: this.deviceId,
      groupId:  this.groupId,
      kind:     this.kind,
      label:    this.label,
    };
  };
  globalThis.MediaDeviceInfo = MediaDeviceInfo;
  if (typeof window !== 'undefined') window.MediaDeviceInfo = MediaDeviceInfo;

  // ── InputDeviceInfo ─────────────────────────────────────────────────────────
  // Subclass of MediaDeviceInfo for input devices; adds getCapabilities().
  function InputDeviceInfo(deviceId, groupId, kind, label) {
    MediaDeviceInfo.call(this, deviceId, groupId, kind, label);
  }
  InputDeviceInfo.prototype = Object.create(MediaDeviceInfo.prototype);
  InputDeviceInfo.prototype.constructor = InputDeviceInfo;
  InputDeviceInfo.prototype.getCapabilities = function() { return {}; };
  globalThis.InputDeviceInfo = InputDeviceInfo;
  if (typeof window !== 'undefined') window.InputDeviceInfo = InputDeviceInfo;

  // ── MediaDevices object ─────────────────────────────────────────────────────
  // navigator.mediaDevices is a singleton MediaDevices EventTarget.
  var _md_listeners = {};

  var mediaDevices = {
    // W3C Media Capture §4.3.1 — getSupportedConstraints
    // Returns a map of all recognised constraint names to true.
    getSupportedConstraints: function() {
      return {
        width:            true,
        height:           true,
        aspectRatio:      true,
        frameRate:        true,
        facingMode:       true,
        resizeMode:       true,
        sampleRate:       true,
        sampleSize:       true,
        echoCancellation: true,
        autoGainControl:  true,
        noiseSuppression: true,
        latency:          true,
        channelCount:     true,
        deviceId:         true,
        groupId:          true,
      };
    },

    // W3C Media Capture §4.3.2 — getUserMedia
    // Phase 0: always rejects with NotAllowedError (no camera/mic access).
    getUserMedia: function(constraints) {
      return Promise.reject(
        new DOMException(
          'Permission denied: mediaDevices.getUserMedia is not available in Lumen Phase 0',
          'NotAllowedError'
        )
      );
    },

    // W3C Media Capture §4.3.3 — enumerateDevices
    // Phase 0: returns an empty list (avoids device label fingerprinting, ADR-007).
    enumerateDevices: function() {
      return Promise.resolve([]);
    },

    // Screen Capture API §4.1 — getDisplayMedia
    // Phase 0: always rejects with NotAllowedError (no screen capture).
    getDisplayMedia: function(options) {
      return Promise.reject(
        new DOMException(
          'Permission denied: mediaDevices.getDisplayMedia is not available in Lumen Phase 0',
          'NotAllowedError'
        )
      );
    },

    // EventTarget methods for 'devicechange' event.
    addEventListener: function(type, fn, opts) {
      if (!_md_listeners[type]) _md_listeners[type] = [];
      _md_listeners[type].push(fn);
    },
    removeEventListener: function(type, fn) {
      if (!_md_listeners[type]) return;
      _md_listeners[type] = _md_listeners[type].filter(function(f) { return f !== fn; });
    },
    dispatchEvent: function(evt) {
      var fns = _md_listeners[evt.type] || [];
      fns.forEach(function(f) { try { f(evt); } catch(e) {} });
      return true;
    },

    // ondevicechange handler property.
    get ondevicechange() { return this._ondevicechange || null; },
    set ondevicechange(fn) {
      if (this._ondevicechange) {
        this.removeEventListener('devicechange', this._ondevicechange);
      }
      this._ondevicechange = fn;
      if (fn) this.addEventListener('devicechange', fn);
    },
  };

  // Install on navigator.
  navigator.mediaDevices = mediaDevices;

  // Helper: generate a random UUID for MediaStream/MediaStreamTrack IDs.
  // Falls back to crypto.randomUUID() if available; otherwise uses Math.random.
  function _lumen_random_uuid() {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
    // RFC 4122 v4 fallback.
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
      var r = Math.random() * 16 | 0;
      var v = c === 'x' ? r : (r & 0x3 | 0x8);
      return v.toString(16);
    });
  }
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Set up minimal globals needed before installing the shim.
    ///
    /// Note: `crypto.randomUUID` is deliberately omitted so that `_lumen_random_uuid()`
    /// falls through to the `Math.random`-based fallback — each call produces a distinct
    /// value, enabling clone-ID-inequality tests without a stateful mock.
    fn install_prereqs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            "var navigator = {}; \
             var window = {}; \
             function DOMException(msg, name) { this.message = msg; this.name = name; } \
             DOMException.prototype = Object.create(Error.prototype); \
             var Promise = globalThis.Promise;",
        )
        .unwrap();
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_media_devices_bindings(&ctx).expect("install must succeed");
        });
    }

    #[test]
    fn install_succeeds_with_prereqs() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).expect("install must succeed");
        });
    }

    #[test]
    fn navigator_media_devices_is_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof navigator.mediaDevices").unwrap();
            assert_eq!(ty, "object");
        });
    }

    #[test]
    fn get_user_media_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("typeof navigator.mediaDevices.getUserMedia")
                .unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn get_user_media_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let is_thenable: bool = ctx
                .eval(
                    "(function() { \
                       var p = navigator.mediaDevices.getUserMedia({video:true}); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(is_thenable);
        });
    }

    #[test]
    fn get_display_media_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("typeof navigator.mediaDevices.getDisplayMedia")
                .unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn get_display_media_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let is_thenable: bool = ctx
                .eval(
                    "(function() { \
                       var p = navigator.mediaDevices.getDisplayMedia({video:true}); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(is_thenable);
        });
    }

    #[test]
    fn enumerate_devices_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("typeof navigator.mediaDevices.enumerateDevices")
                .unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn enumerate_devices_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let is_thenable: bool = ctx
                .eval(
                    "(function() { \
                       var p = navigator.mediaDevices.enumerateDevices(); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(is_thenable);
        });
    }

    #[test]
    fn get_supported_constraints_returns_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("typeof navigator.mediaDevices.getSupportedConstraints()")
                .unwrap();
            assert_eq!(ty, "object");
        });
    }

    #[test]
    fn get_supported_constraints_has_common_keys() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let has_width: bool = ctx
                .eval("navigator.mediaDevices.getSupportedConstraints().width === true")
                .unwrap();
            assert!(has_width);
            let has_frame_rate: bool = ctx
                .eval("navigator.mediaDevices.getSupportedConstraints().frameRate === true")
                .unwrap();
            assert!(has_frame_rate);
        });
    }

    #[test]
    fn media_stream_is_class() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof window.MediaStream").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn media_stream_instance_has_id_and_active() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var s = new MediaStream(); \
                       return typeof s.id === 'string' && s.active === false; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_stream_get_tracks_empty() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let len: i32 = ctx
                .eval("(new MediaStream()).getTracks().length")
                .unwrap();
            assert_eq!(len, 0);
        });
    }

    #[test]
    fn media_stream_add_remove_track() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var s = new MediaStream(); \
                       var t = new MediaStreamTrack('video', 'cam'); \
                       s.addTrack(t); \
                       if (s.getTracks().length !== 1) return false; \
                       if (!s.active) return false; \
                       s.removeTrack(t); \
                       return s.getTracks().length === 0 && !s.active; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_stream_clone_is_independent() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var s = new MediaStream([new MediaStreamTrack('audio','mic')]); \
                       var c = s.clone(); \
                       return c.id !== s.id && c.getTracks().length === 1; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_stream_track_is_class() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof window.MediaStreamTrack").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn media_stream_track_properties() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var t = new MediaStreamTrack('video', 'camera'); \
                       return t.kind === 'video' \
                           && t.label === 'camera' \
                           && typeof t.id === 'string' \
                           && t.enabled === true \
                           && t.muted === false \
                           && t.readyState === 'ended'; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_stream_track_clone() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var t = new MediaStreamTrack('audio', 'mic'); \
                       var c = t.clone(); \
                       return c.kind === 'audio' && c.id !== t.id; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_device_info_is_class() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof window.MediaDeviceInfo").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn media_device_info_to_json() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var d = new MediaDeviceInfo('id1','g1','audioinput','Mic'); \
                       var j = d.toJSON(); \
                       return j.deviceId === 'id1' && j.kind === 'audioinput' && j.label === 'Mic'; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn input_device_info_is_subclass() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var d = new InputDeviceInfo('id2','g2','videoinput','Cam'); \
                       return d instanceof MediaDeviceInfo \
                           && typeof d.getCapabilities === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn on_device_change_setter() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var called = false; \
                       navigator.mediaDevices.ondevicechange = function() { called = true; }; \
                       return typeof navigator.mediaDevices.ondevicechange === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn add_remove_event_listener() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            install_media_devices_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var count = 0; \
                       var fn = function() { count++; }; \
                       navigator.mediaDevices.addEventListener('devicechange', fn); \
                       navigator.mediaDevices.removeEventListener('devicechange', fn); \
                       navigator.mediaDevices.dispatchEvent({type:'devicechange'}); \
                       return count === 0; \
                     })()",
                )
                .unwrap();
            assert!(ok, "listener should have been removed");
        });
    }
}
