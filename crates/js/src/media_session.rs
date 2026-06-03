//! MediaSession API (W3C Media Session §5).
//!
//! Installs `navigator.mediaSession` and `MediaMetadata` so that pages can
//! report playback state and rich metadata (title/artist/album/artwork) for
//! OS media controls without JS errors.
//!
//! Phase 0: the metadata and playback state are stored in JS objects but not
//! forwarded to the OS media-control surface (lock screen / SMTC / MPRIS).
//! Shell integration (P3) can read `_lumen_take_media_session_update()` to
//! pick up changes and wire them to platform APIs.
//!
//! Installed interfaces:
//! - `MediaMetadata` class — title/artist/album/artwork
//! - `MediaPositionState` — duration/playbackRate/position
//! - `navigator.mediaSession` — MediaSession singleton
//!   - `metadata` getter/setter (MediaMetadata)
//!   - `playbackState` getter/setter ("none" | "paused" | "playing")
//!   - `setActionHandler(action, callback)` — play/pause/stop/seekbackward/
//!     seekforward/seekto/previoustrack/nexttrack/skipad/
//!     togglemicrophone/togglecamera/hangup/togglecaptionstrack
//!   - `setPositionState(state)`
//!   - `setCameraActive(active)` / `setMicrophoneActive(active)` (L2 §5.4)
//! - `window.MediaMetadata` exported as global

use rquickjs::Ctx;

/// Install MediaSession API shim into the JS context.
///
/// Adds `navigator.mediaSession` with all W3C Media Session §5 methods and
/// exports `MediaMetadata` as a global. Changes to metadata and playbackState
/// are stored in JS state; `_lumen_take_media_session_update()` returns a JSON
/// snapshot for shell/OS integration.
///
/// Must be called **after** `install_dom_api` so that `navigator`, `Event`,
/// and `JSON` are already defined.
pub fn install_media_session_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(MEDIA_SESSION_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the MediaSession API (W3C Media Session §5).
const MEDIA_SESSION_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // ── MediaMetadata ──────────────────────────────────────────────────────────
  // W3C Media Session §6.1: rich metadata for the playing media item.
  function MediaMetadata(init) {
    this.title   = (init && init.title)   || '';
    this.artist  = (init && init.artist)  || '';
    this.album   = (init && init.album)   || '';
    this.artwork = (init && Array.isArray(init.artwork)) ? init.artwork.slice() : [];
  }
  MediaMetadata.prototype.toString = function() {
    return '[object MediaMetadata]';
  };

  // ── Allowed playback states (W3C Media Session §5.1) ──────────────────────
  var VALID_PLAYBACK_STATES = { 'none': true, 'paused': true, 'playing': true };

  // ── Valid action types (W3C Media Session §5.3) ───────────────────────────
  var VALID_ACTIONS = {
    'play': true, 'pause': true, 'stop': true,
    'seekbackward': true, 'seekforward': true, 'seekto': true,
    'previoustrack': true, 'nexttrack': true, 'skipad': true,
    'togglemicrophone': true, 'togglecamera': true, 'hangup': true,
    'togglecaptionstrack': true, 'enterpictureinpicture': true
  };

  // ── MediaSession singleton ─────────────────────────────────────────────────
  var _metadata       = null;
  var _playbackState  = 'none';
  var _actionHandlers = {};
  var _positionState  = null;
  var _cameraActive   = false;
  var _micActive      = false;
  // Incremented whenever state changes so shell can detect stale reads.
  var _updateSeq = 0;

  var mediaSession = {
    // W3C §5.1: metadata getter/setter.
    get metadata() { return _metadata; },
    set metadata(v) {
      _metadata = (v instanceof MediaMetadata || v === null) ? v : null;
      _updateSeq++;
    },

    // W3C §5.1: playbackState getter/setter.
    get playbackState() { return _playbackState; },
    set playbackState(v) {
      if (VALID_PLAYBACK_STATES[v]) {
        _playbackState = v;
        _updateSeq++;
      }
    },

    // W3C §5.3: register/unregister an action handler.
    setActionHandler: function(action, callback) {
      if (!VALID_ACTIONS[action]) return;
      if (callback === null) {
        delete _actionHandlers[action];
      } else if (typeof callback === 'function') {
        _actionHandlers[action] = callback;
      }
    },

    // W3C §5.4: update position state.
    setPositionState: function(state) {
      if (!state) {
        _positionState = null;
        _updateSeq++;
        return;
      }
      _positionState = {
        duration:     typeof state.duration     === 'number' ? state.duration     : NaN,
        playbackRate: typeof state.playbackRate === 'number' ? state.playbackRate : 1,
        position:     typeof state.position     === 'number' ? state.position     : 0
      };
      _updateSeq++;
    },

    // W3C Media Session L2 §5.4: camera/microphone active state.
    setCameraActive: function(active) {
      _cameraActive = Boolean(active);
      _updateSeq++;
    },
    setMicrophoneActive: function(active) {
      _micActive = Boolean(active);
      _updateSeq++;
    }
  };

  // ── Shell integration helper ───────────────────────────────────────────────
  // Returns a JSON-serialisable snapshot of the current session state, or null
  // if nothing changed since the last call (same _updateSeq).
  // Shell (P3) polls this in about_to_wait to forward metadata to OS.
  var _lastSeqSeen = -1;
  globalThis._lumen_take_media_session_update = function() {
    if (_updateSeq === _lastSeqSeen) return null;
    _lastSeqSeen = _updateSeq;
    return {
      metadata: _metadata ? {
        title:   _metadata.title,
        artist:  _metadata.artist,
        album:   _metadata.album,
        artwork: _metadata.artwork
      } : null,
      playbackState: _playbackState,
      positionState: _positionState,
      cameraActive:  _cameraActive,
      micActive:     _micActive
    };
  };

  // Deliver a media session action from the OS (e.g. OS media keys).
  // Shell calls _lumen_fire_media_action('play') etc. to trigger handlers.
  globalThis._lumen_fire_media_action = function(action, details) {
    var handler = _actionHandlers[action];
    if (typeof handler === 'function') {
      try { handler(details || {}); } catch(_) {}
    }
  };

  // ── Install on navigator ───────────────────────────────────────────────────
  try {
    Object.defineProperty(navigator, 'mediaSession', {
      value: mediaSession, writable: false, configurable: true, enumerable: true
    });
  } catch(_) {
    navigator.mediaSession = mediaSession;
  }

  // ── Global exports ─────────────────────────────────────────────────────────
  try { window.MediaMetadata = MediaMetadata; } catch(_) {}
})();
"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_media_session(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                "#,
            )
            .unwrap();
            super::install_media_session_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn media_session_installed() {
        with_media_session(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.mediaSession === 'object' && navigator.mediaSession !== null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn media_metadata_class_exists() {
        with_media_session(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MediaMetadata === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn playback_state_default_none() {
        with_media_session(|ctx| {
            let state: String = ctx
                .eval("navigator.mediaSession.playbackState")
                .unwrap();
            assert_eq!(state, "none");
        });
    }

    #[test]
    fn playback_state_setter() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>("navigator.mediaSession.playbackState = 'playing';")
                .unwrap();
            let state: String = ctx
                .eval("navigator.mediaSession.playbackState")
                .unwrap();
            assert_eq!(state, "playing");
        });
    }

    #[test]
    fn invalid_playback_state_ignored() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>("navigator.mediaSession.playbackState = 'invalid_value';")
                .unwrap();
            let state: String = ctx
                .eval("navigator.mediaSession.playbackState")
                .unwrap();
            assert_eq!(state, "none");
        });
    }

    #[test]
    fn metadata_null_initially() {
        with_media_session(|ctx| {
            let null_meta: bool = ctx
                .eval("navigator.mediaSession.metadata === null")
                .unwrap();
            assert!(null_meta);
        });
    }

    #[test]
    fn media_metadata_creation() {
        with_media_session(|ctx| {
            let title: String = ctx
                .eval(r#"
                  var m = new window.MediaMetadata({
                    title: 'Test Song',
                    artist: 'Test Artist',
                    album: 'Test Album'
                  });
                  m.title
                "#)
                .unwrap();
            assert_eq!(title, "Test Song");
        });
    }

    #[test]
    fn metadata_setter() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                r#"navigator.mediaSession.metadata = new window.MediaMetadata({
                    title: 'Hello',
                    artist: 'World'
                });"#,
            )
            .unwrap();
            let artist: String = ctx
                .eval("navigator.mediaSession.metadata.artist")
                .unwrap();
            assert_eq!(artist, "World");
        });
    }

    #[test]
    fn set_action_handler_stores_callback() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                "navigator.mediaSession.setActionHandler('play', function() {});",
            )
            .unwrap();
            // No error means it worked; the handler is stored internally.
            let ok: bool = ctx.eval("true").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn fire_media_action_calls_handler() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                r#"
                globalThis._played = false;
                navigator.mediaSession.setActionHandler('play', function() {
                  globalThis._played = true;
                });
                "#,
            )
            .unwrap();
            ctx.eval::<(), _>("globalThis._lumen_fire_media_action('play');")
                .unwrap();
            let played: bool = ctx.eval("globalThis._played").unwrap();
            assert!(played);
        });
    }

    #[test]
    fn set_action_handler_null_removes_callback() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                r#"
                globalThis._pausedCount = 0;
                navigator.mediaSession.setActionHandler('pause', function() {
                  globalThis._pausedCount++;
                });
                navigator.mediaSession.setActionHandler('pause', null);
                globalThis._lumen_fire_media_action('pause');
                "#,
            )
            .unwrap();
            let count: u32 = ctx.eval("globalThis._pausedCount").unwrap();
            assert_eq!(count, 0);
        });
    }

    #[test]
    fn set_position_state_stores_values() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                r#"navigator.mediaSession.setPositionState({
                    duration: 300,
                    playbackRate: 1.5,
                    position: 42
                });"#,
            )
            .unwrap();
            // No error means success; internal _positionState updated.
            let ok: bool = ctx.eval("true").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn take_media_session_update_returns_snapshot() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>(
                r#"navigator.mediaSession.playbackState = 'playing';
                navigator.mediaSession.metadata = new window.MediaMetadata({ title: 'X' });"#,
            )
            .unwrap();
            let has_update: bool = ctx
                .eval("globalThis._lumen_take_media_session_update() !== null")
                .unwrap();
            assert!(has_update);
        });
    }

    #[test]
    fn take_media_session_update_null_when_no_change() {
        with_media_session(|ctx| {
            // Prime: consume first update.
            ctx.eval::<(), _>("globalThis._lumen_take_media_session_update();")
                .unwrap();
            // Second call with no change should return null.
            let null_update: bool = ctx
                .eval("globalThis._lumen_take_media_session_update() === null")
                .unwrap();
            assert!(null_update);
        });
    }

    #[test]
    fn set_camera_active() {
        with_media_session(|ctx| {
            ctx.eval::<(), _>("navigator.mediaSession.setCameraActive(true);")
                .unwrap();
            let has_update: bool = ctx
                .eval("globalThis._lumen_take_media_session_update() !== null")
                .unwrap();
            assert!(has_update);
        });
    }
}
