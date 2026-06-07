//! Web Codecs API stub (W3C Web Codecs).
//!
//! Phase 0: VideoDecoder / VideoEncoder / AudioDecoder / AudioEncoder
//! constructors, state machine, isConfigSupported() → {supported:false}.
//! All encode/decode operations are no-ops; flush() resolves immediately.

use rquickjs::Ctx;

/// Install Web Codecs API stubs into the JS context.
pub fn install_web_codecs(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEB_CODECS_SHIM)?;
    Ok(())
}

const WEB_CODECS_SHIM: &str = r#"(function() {
  'use strict';

  // ── EncodedVideoChunk ──────────────────────────────────────────────────────
  // W3C Web Codecs §EncodedVideoChunk
  function EncodedVideoChunk(init) {
    if (!init || typeof init !== 'object') {
      throw new TypeError('EncodedVideoChunk: init dict required');
    }
    this.type      = String(init.type || 'key');
    this.timestamp = Number(init.timestamp || 0);
    this.duration  = (init.duration !== undefined && init.duration !== null)
                       ? Number(init.duration) : null;
    this.byteLength = (init.data && typeof init.data.byteLength === 'number')
                       ? init.data.byteLength : 0;
    this._data = init.data || null;
  }
  EncodedVideoChunk.prototype.copyTo = function(destination) {
    if (this._data) {
      var src = new Uint8Array(this._data);
      var dst = new Uint8Array(destination);
      dst.set(src.subarray(0, Math.min(src.length, dst.length)));
    }
  };

  // ── EncodedAudioChunk ──────────────────────────────────────────────────────
  // W3C Web Codecs §EncodedAudioChunk
  function EncodedAudioChunk(init) {
    if (!init || typeof init !== 'object') {
      throw new TypeError('EncodedAudioChunk: init dict required');
    }
    this.type       = String(init.type || 'key');
    this.timestamp  = Number(init.timestamp || 0);
    this.duration   = (init.duration !== undefined && init.duration !== null)
                        ? Number(init.duration) : null;
    this.byteLength = (init.data && typeof init.data.byteLength === 'number')
                        ? init.data.byteLength : 0;
    this._data = init.data || null;
  }
  EncodedAudioChunk.prototype.copyTo = function(destination) {
    if (this._data) {
      var src = new Uint8Array(this._data);
      var dst = new Uint8Array(destination);
      dst.set(src.subarray(0, Math.min(src.length, dst.length)));
    }
  };

  // ── VideoFrame ────────────────────────────────────────────────────────────
  // W3C Web Codecs §VideoFrame (minimal Phase 0 stub)
  function VideoFrame(image, init) {
    init = init || {};
    this.codedWidth     = Number(init.codedWidth  || 0);
    this.codedHeight    = Number(init.codedHeight || 0);
    this.displayWidth   = this.codedWidth;
    this.displayHeight  = this.codedHeight;
    this.timestamp      = Number(init.timestamp || 0);
    this.duration       = (init.duration !== undefined && init.duration !== null)
                            ? Number(init.duration) : null;
    this.format         = init.format || null;
    this._closed        = false;
  }
  VideoFrame.prototype.close = function() { this._closed = true; };
  VideoFrame.prototype.clone = function() {
    if (this._closed) { throw new DOMException('VideoFrame is closed', 'InvalidStateError'); }
    return new VideoFrame(null, {
      codedWidth: this.codedWidth, codedHeight: this.codedHeight,
      timestamp: this.timestamp, duration: this.duration, format: this.format
    });
  };

  // ── AudioData ─────────────────────────────────────────────────────────────
  // W3C Web Codecs §AudioData (minimal Phase 0 stub)
  function AudioData(init) {
    init = init || {};
    this.format         = init.format || null;
    this.sampleRate     = Number(init.sampleRate     || 0);
    this.numberOfFrames = Number(init.numberOfFrames || 0);
    this.numberOfChannels = Number(init.numberOfChannels || 0);
    this.duration       = this.numberOfFrames
                            ? Math.round(this.numberOfFrames / (this.sampleRate || 1) * 1e6)
                            : 0;
    this.timestamp      = Number(init.timestamp || 0);
    this._closed        = false;
  }
  AudioData.prototype.close = function() { this._closed = true; };
  AudioData.prototype.clone = function() {
    if (this._closed) { throw new DOMException('AudioData is closed', 'InvalidStateError'); }
    return new AudioData({
      format: this.format, sampleRate: this.sampleRate,
      numberOfFrames: this.numberOfFrames, numberOfChannels: this.numberOfChannels,
      timestamp: this.timestamp
    });
  };

  // ── VideoDecoder ──────────────────────────────────────────────────────────
  // W3C Web Codecs §VideoDecoder
  function VideoDecoder(init) {
    if (!init || typeof init.output !== 'function' || typeof init.error !== 'function') {
      throw new TypeError('VideoDecoder: {output, error} callbacks required');
    }
    this._output         = init.output;
    this._error          = init.error;
    this.state           = 'unconfigured';
    this.decodeQueueSize = 0;
  }
  VideoDecoder.prototype.configure = function(config) {
    if (this.state === 'closed') {
      throw new DOMException('VideoDecoder is closed', 'InvalidStateError');
    }
    this.state = 'configured';
    this._config = config;
  };
  VideoDecoder.prototype.decode = function(chunk) {
    if (this.state !== 'configured') {
      throw new DOMException('VideoDecoder not configured', 'InvalidStateError');
    }
    // Phase 0: no-op (no real codec)
  };
  VideoDecoder.prototype.flush = function() {
    if (this.state === 'closed') {
      return Promise.reject(new DOMException('VideoDecoder is closed', 'InvalidStateError'));
    }
    return Promise.resolve();
  };
  VideoDecoder.prototype.reset = function() {
    if (this.state === 'closed') {
      throw new DOMException('VideoDecoder is closed', 'InvalidStateError');
    }
    this.state = 'unconfigured';
    this.decodeQueueSize = 0;
  };
  VideoDecoder.prototype.close = function() {
    this.state = 'closed';
    this.decodeQueueSize = 0;
  };
  VideoDecoder.isConfigSupported = function(config) {
    return Promise.resolve({ supported: false, config: config });
  };

  // ── VideoEncoder ──────────────────────────────────────────────────────────
  // W3C Web Codecs §VideoEncoder
  function VideoEncoder(init) {
    if (!init || typeof init.output !== 'function' || typeof init.error !== 'function') {
      throw new TypeError('VideoEncoder: {output, error} callbacks required');
    }
    this._output         = init.output;
    this._error          = init.error;
    this.state           = 'unconfigured';
    this.encodeQueueSize = 0;
  }
  VideoEncoder.prototype.configure = function(config) {
    if (this.state === 'closed') {
      throw new DOMException('VideoEncoder is closed', 'InvalidStateError');
    }
    this.state = 'configured';
    this._config = config;
  };
  VideoEncoder.prototype.encode = function(frame, options) {
    if (this.state !== 'configured') {
      throw new DOMException('VideoEncoder not configured', 'InvalidStateError');
    }
    // Phase 0: no-op
  };
  VideoEncoder.prototype.flush = function() {
    if (this.state === 'closed') {
      return Promise.reject(new DOMException('VideoEncoder is closed', 'InvalidStateError'));
    }
    return Promise.resolve();
  };
  VideoEncoder.prototype.reset = function() {
    if (this.state === 'closed') {
      throw new DOMException('VideoEncoder is closed', 'InvalidStateError');
    }
    this.state = 'unconfigured';
    this.encodeQueueSize = 0;
  };
  VideoEncoder.prototype.close = function() {
    this.state = 'closed';
    this.encodeQueueSize = 0;
  };
  VideoEncoder.isConfigSupported = function(config) {
    return Promise.resolve({ supported: false, config: config });
  };

  // ── AudioDecoder ──────────────────────────────────────────────────────────
  // W3C Web Codecs §AudioDecoder
  function AudioDecoder(init) {
    if (!init || typeof init.output !== 'function' || typeof init.error !== 'function') {
      throw new TypeError('AudioDecoder: {output, error} callbacks required');
    }
    this._output         = init.output;
    this._error          = init.error;
    this.state           = 'unconfigured';
    this.decodeQueueSize = 0;
  }
  AudioDecoder.prototype.configure = function(config) {
    if (this.state === 'closed') {
      throw new DOMException('AudioDecoder is closed', 'InvalidStateError');
    }
    this.state = 'configured';
    this._config = config;
  };
  AudioDecoder.prototype.decode = function(chunk) {
    if (this.state !== 'configured') {
      throw new DOMException('AudioDecoder not configured', 'InvalidStateError');
    }
    // Phase 0: no-op
  };
  AudioDecoder.prototype.flush = function() {
    if (this.state === 'closed') {
      return Promise.reject(new DOMException('AudioDecoder is closed', 'InvalidStateError'));
    }
    return Promise.resolve();
  };
  AudioDecoder.prototype.reset = function() {
    if (this.state === 'closed') {
      throw new DOMException('AudioDecoder is closed', 'InvalidStateError');
    }
    this.state = 'unconfigured';
    this.decodeQueueSize = 0;
  };
  AudioDecoder.prototype.close = function() {
    this.state = 'closed';
    this.decodeQueueSize = 0;
  };
  AudioDecoder.isConfigSupported = function(config) {
    return Promise.resolve({ supported: false, config: config });
  };

  // ── AudioEncoder ──────────────────────────────────────────────────────────
  // W3C Web Codecs §AudioEncoder
  function AudioEncoder(init) {
    if (!init || typeof init.output !== 'function' || typeof init.error !== 'function') {
      throw new TypeError('AudioEncoder: {output, error} callbacks required');
    }
    this._output         = init.output;
    this._error          = init.error;
    this.state           = 'unconfigured';
    this.encodeQueueSize = 0;
  }
  AudioEncoder.prototype.configure = function(config) {
    if (this.state === 'closed') {
      throw new DOMException('AudioEncoder is closed', 'InvalidStateError');
    }
    this.state = 'configured';
    this._config = config;
  };
  AudioEncoder.prototype.encode = function(data, options) {
    if (this.state !== 'configured') {
      throw new DOMException('AudioEncoder not configured', 'InvalidStateError');
    }
    // Phase 0: no-op
  };
  AudioEncoder.prototype.flush = function() {
    if (this.state === 'closed') {
      return Promise.reject(new DOMException('AudioEncoder is closed', 'InvalidStateError'));
    }
    return Promise.resolve();
  };
  AudioEncoder.prototype.reset = function() {
    if (this.state === 'closed') {
      throw new DOMException('AudioEncoder is closed', 'InvalidStateError');
    }
    this.state = 'unconfigured';
    this.encodeQueueSize = 0;
  };
  AudioEncoder.prototype.close = function() {
    this.state = 'closed';
    this.encodeQueueSize = 0;
  };
  AudioEncoder.isConfigSupported = function(config) {
    return Promise.resolve({ supported: false, config: config });
  };

  // ── Export ────────────────────────────────────────────────────────────────
  globalThis.EncodedVideoChunk  = EncodedVideoChunk;
  globalThis.EncodedAudioChunk  = EncodedAudioChunk;
  globalThis.VideoFrame         = VideoFrame;
  globalThis.AudioData          = AudioData;
  globalThis.VideoDecoder       = VideoDecoder;
  globalThis.VideoEncoder       = VideoEncoder;
  globalThis.AudioDecoder       = AudioDecoder;
  globalThis.AudioEncoder       = AudioEncoder;
})();"#;

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn with_web_codecs(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                "globalThis.DOMException = function(msg, name) { \
                   this.message = msg; this.name = name || 'Error'; \
                 }; \
                 globalThis.DOMException.prototype = Object.create(Error.prototype);",
            )
            .expect("install DOMException stub");
            super::install_web_codecs(&ctx).expect("install_web_codecs");
            f(&ctx);
        });
    }

    #[test]
    fn video_decoder_class_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof VideoDecoder === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn video_encoder_class_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof VideoEncoder === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn audio_decoder_class_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof AudioDecoder === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn audio_encoder_class_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof AudioEncoder === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn initial_state_is_unconfigured() {
        with_web_codecs(|ctx| {
            let s: String = ctx
                .eval(
                    "var d = new VideoDecoder({output:function(){},error:function(){}}); \
                     d.state",
                )
                .expect("eval");
            assert_eq!(s, "unconfigured");
        });
    }

    #[test]
    fn configure_transitions_to_configured() {
        with_web_codecs(|ctx| {
            let s: String = ctx
                .eval(
                    "var d = new AudioDecoder({output:function(){},error:function(){}}); \
                     d.configure({codec:'mp4a.40.2',sampleRate:48000,numberOfChannels:2}); \
                     d.state",
                )
                .expect("eval");
            assert_eq!(s, "configured");
        });
    }

    #[test]
    fn is_config_supported_returns_promise() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("VideoDecoder.isConfigSupported({codec:'vp8'}) instanceof Promise")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn flush_returns_promise() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval(
                    "var e = new VideoEncoder({output:function(){},error:function(){}}); \
                     e.flush() instanceof Promise",
                )
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn encoded_video_chunk_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof EncodedVideoChunk === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn encoded_audio_chunk_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof EncodedAudioChunk === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn video_frame_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof VideoFrame === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }

    #[test]
    fn audio_data_exists() {
        with_web_codecs(|ctx| {
            let ok: bool = ctx
                .eval("typeof AudioData === 'function'")
                .expect("eval");
            assert!(ok);
        });
    }
}
