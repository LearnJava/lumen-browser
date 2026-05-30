//! AudioContext API stub with per-session fingerprint noise (ADR-007 Layer 4, 9D.3).
//!
//! Injects `AudioContext`, `OfflineAudioContext`, and `AudioBuffer` stubs into the
//! QuickJS context. `getChannelData()`, `copyFromChannel()`, and `getFloatFrequencyData()`
//! add tiny per-session LCG noise (±1e-7) to returned samples, preventing audio
//! fingerprinting while preserving the API shape for feature detection.
//!
//! Follows the same pattern as `canvas/src/fp_noise.rs` (Brave-style per-session noise).

use rquickjs::Ctx;
use std::sync::atomic::{AtomicU32, Ordering};

/// Global counter: each `install_audio_bindings` call gets a unique u32 seed.
///
/// Starts at 1 so the first session seed is never 0 (zero seed produces a degenerate
/// LCG sequence where every `_next()` call returns the same value).
static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Generate a unique per-session noise seed.
///
/// Each call increments a process-global counter and returns the old value.
/// The seed is stable within a session (one `QuickJsRuntime` instance) and
/// different across sessions — sufficient to defeat fingerprinting without
/// requiring a cryptographic RNG.
pub fn new_session_seed() -> u32 {
    SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Install AudioContext stub with fingerprint noise into the JS context.
///
/// Defines `globalThis.AudioContext`, `globalThis.OfflineAudioContext`, and
/// `globalThis.AudioBuffer` with LCG noise baked into buffer reads. The `seed`
/// should be obtained via `new_session_seed()` — different per `QuickJsRuntime`
/// instance, deterministic within a session.
///
/// Must be called before any user script that touches the Web Audio API.
pub fn install_audio_bindings(ctx: &Ctx, seed: u32) -> rquickjs::Result<()> {
    ctx.globals().set("_LUMEN_AUDIO_NOISE_SEED", seed)?;
    ctx.eval::<(), _>(AUDIO_SHIM)?;
    Ok(())
}

/// JavaScript shim: AudioContext / OfflineAudioContext / AudioBuffer stubs.
///
/// The IIFE captures `_LUMEN_AUDIO_NOISE_SEED` at evaluation time. Each reinstall
/// (e.g., per tab) overwrites the globals and creates a fresh noise closure with
/// the new seed. Noise magnitude ±1e-7 — below perceptual threshold but large enough
/// to change the fingerprint hash.
const AUDIO_SHIM: &str = r#"(function(seed) {
  var _s = seed >>> 0;
  if (_s === 0) _s = 2654435761;
  function _next() {
    _s = Math.imul(_s, 1664525) + 1013904223 | 0;
    _s = _s >>> 0;
    return (_s / 4294967295.0 - 0.5) * 2e-7;
  }

  function AudioBuffer(opts) {
    var nc = (opts && opts.numberOfChannels) || 1;
    var len = (opts && opts.length) || 0;
    this.sampleRate = (opts && opts.sampleRate) || 44100;
    this.numberOfChannels = nc;
    this.length = len;
    this.duration = len / this.sampleRate;
    this._ch = [];
    for (var i = 0; i < nc; i++) {
      this._ch.push(new Float32Array(len));
    }
  }

  AudioBuffer.prototype.getChannelData = function(channel) {
    var data = this._ch[channel >>> 0] || new Float32Array(0);
    for (var i = 0; i < data.length; i++) {
      data[i] = data[i] + _next();
    }
    return data;
  };

  AudioBuffer.prototype.copyFromChannel = function(dest, channel, offset) {
    offset = offset >>> 0;
    var src = this._ch[channel >>> 0] || new Float32Array(0);
    for (var i = 0; i < dest.length && (i + offset) < src.length; i++) {
      dest[i] = src[i + offset] + _next();
    }
  };

  AudioBuffer.prototype.copyToChannel = function(src, channel, offset) {
    offset = offset >>> 0;
    var dst = this._ch[channel >>> 0];
    if (!dst) return;
    for (var i = 0; i < src.length && (i + offset) < dst.length; i++) {
      dst[i + offset] = src[i];
    }
  };

  function OfflineAudioContext(channels, length, sampleRate) {
    this._channels = channels || 1;
    this._length = length || 0;
    this.sampleRate = sampleRate || 44100;
    this.length = this._length;
    this.currentTime = 0;
  }

  OfflineAudioContext.prototype.createOscillator = function() {
    return {
      type: 'triangle',
      frequency: { value: 440, setValueAtTime: function() {} },
      connect: function() {},
      start: function() {}
    };
  };

  OfflineAudioContext.prototype.createDynamicsCompressor = function() {
    return {
      threshold: { value: -50 }, knee: { value: 40 },
      ratio: { value: 12 }, attack: { value: 0 }, release: { value: 0.25 },
      connect: function() {},
      disconnect: function() {}
    };
  };

  OfflineAudioContext.prototype.createGain = function() {
    return {
      gain: { value: 1.0, setValueAtTime: function() {} },
      connect: function() {},
      disconnect: function() {}
    };
  };

  OfflineAudioContext.prototype.startRendering = function() {
    var buf = new AudioBuffer({
      numberOfChannels: this._channels,
      length: this._length,
      sampleRate: this.sampleRate
    });
    return Promise.resolve(buf);
  };

  function AudioContext() {
    this.sampleRate = 44100;
    this.state = 'running';
    this.currentTime = 0;
  }

  AudioContext.prototype.createAnalyser = function() {
    return {
      fftSize: 2048,
      frequencyBinCount: 1024,
      getFloatFrequencyData: function(arr) {
        for (var i = 0; i < arr.length; i++) {
          arr[i] = -100.0 + _next();
        }
      },
      getByteFrequencyData: function(arr) {
        for (var i = 0; i < arr.length; i++) { arr[i] = 0; }
      },
      getFloatTimeDomainData: function(arr) {
        for (var i = 0; i < arr.length; i++) { arr[i] = _next(); }
      },
      connect: function() {},
      disconnect: function() {}
    };
  };

  AudioContext.prototype.createOscillator = function() {
    return {
      type: 'sine',
      frequency: { value: 440, setValueAtTime: function() {} },
      connect: function() {},
      disconnect: function() {},
      start: function() {},
      stop: function() {}
    };
  };

  AudioContext.prototype.createBuffer = function(channels, length, sampleRate) {
    return new AudioBuffer({ numberOfChannels: channels, length: length, sampleRate: sampleRate });
  };

  AudioContext.prototype.decodeAudioData = function() {
    return Promise.reject(new Error('decodeAudioData not supported'));
  };

  AudioContext.prototype.close = function() {
    this.state = 'closed';
    return Promise.resolve();
  };

  AudioContext.prototype.resume = function() {
    this.state = 'running';
    return Promise.resolve();
  };

  AudioContext.prototype.suspend = function() {
    this.state = 'suspended';
    return Promise.resolve();
  };

  globalThis.AudioBuffer = AudioBuffer;
  globalThis.AudioContext = AudioContext;
  globalThis.webkitAudioContext = AudioContext;
  globalThis.OfflineAudioContext = OfflineAudioContext;
})(_LUMEN_AUDIO_NOISE_SEED | 0);
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

    #[test]
    fn session_seeds_are_unique() {
        let s1 = new_session_seed();
        let s2 = new_session_seed();
        assert_ne!(s1, s2);
    }

    #[test]
    fn session_seeds_monotonically_increase() {
        let seeds: Vec<u32> = (0..5).map(|_| new_session_seed()).collect();
        for i in 1..seeds.len() {
            assert!(seeds[i] > seeds[i - 1]);
        }
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 42).expect("install should succeed");
        });
    }

    #[test]
    fn audio_context_is_defined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 1).unwrap();
            let ty: String = ctx.eval("typeof AudioContext").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn webkit_audio_context_alias() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 1).unwrap();
            let same: bool = ctx.eval("AudioContext === webkitAudioContext").unwrap();
            assert!(same);
        });
    }

    #[test]
    fn offline_audio_context_is_defined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 1).unwrap();
            let ty: String = ctx.eval("typeof OfflineAudioContext").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn audio_buffer_is_defined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 1).unwrap();
            let ty: String = ctx.eval("typeof AudioBuffer").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn audio_buffer_get_channel_data_length() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 42).unwrap();
            let len: f64 = ctx
                .eval(
                    "new AudioBuffer({numberOfChannels:1, length:128, sampleRate:44100})\
                     .getChannelData(0).length",
                )
                .unwrap();
            assert_eq!(len as usize, 128);
        });
    }

    #[test]
    fn audio_buffer_noise_is_tiny() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 7).unwrap();
            // getChannelData adds noise; all values should be tiny (±2e-7 max)
            let max_abs: f64 = ctx
                .eval(
                    "(function() { \
                       var b = new AudioBuffer({numberOfChannels:1, length:64, sampleRate:44100}); \
                       var d = b.getChannelData(0); \
                       var m = 0; \
                       for (var i = 0; i < d.length; i++) { \
                         var v = Math.abs(d[i]); if (v > m) m = v; \
                       } \
                       return m; \
                     })()",
                )
                .unwrap();
            assert!(max_abs <= 2e-7, "noise magnitude {max_abs} exceeds ±2e-7");
        });
    }

    #[test]
    fn different_seeds_produce_different_noise() {
        // Two separate runtimes, different seeds → first getChannelData sample differs.
        let first = {
            let (_rt, ctx) = make_ctx();
            ctx.with(|ctx| {
                install_audio_bindings(&ctx, 100).unwrap();
                let v: f64 = ctx
                    .eval(
                        "new AudioBuffer({numberOfChannels:1,length:64,sampleRate:44100})\
                         .getChannelData(0)[0]",
                    )
                    .unwrap();
                v
            })
        };

        let second = {
            let (_rt, ctx) = make_ctx();
            ctx.with(|ctx| {
                install_audio_bindings(&ctx, 999).unwrap();
                let v: f64 = ctx
                    .eval(
                        "new AudioBuffer({numberOfChannels:1,length:64,sampleRate:44100})\
                         .getChannelData(0)[0]",
                    )
                    .unwrap();
                v
            })
        };

        assert_ne!(
            first.to_bits(),
            second.to_bits(),
            "seeds 100 and 999 must produce different first noise sample"
        );
    }

    #[test]
    fn audio_context_state_transitions() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 1).unwrap();
            let state: String = ctx
                .eval("(function() { var a = new AudioContext(); return a.state; })()")
                .unwrap();
            assert_eq!(state, "running");
        });
    }

    #[test]
    fn analyser_frequency_data_length() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 5).unwrap();
            let len: f64 = ctx
                .eval(
                    "(function() { \
                       var a = new AudioContext(); \
                       var n = a.createAnalyser(); \
                       return n.frequencyBinCount; \
                     })()",
                )
                .unwrap();
            assert_eq!(len as usize, 1024);
        });
    }

    #[test]
    fn offline_audio_context_start_rendering_returns_thenable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 3).unwrap();
            // startRendering() must return a thenable (Promise). QuickJS Promises are
            // async microtasks, so we only verify the shape — not synchronous resolution.
            let is_thenable: bool = ctx
                .eval(
                    "(function() { \
                       var oac = new OfflineAudioContext(1, 256, 44100); \
                       var p = oac.startRendering(); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(is_thenable, "startRendering() must return a thenable");
        });
    }

    #[test]
    fn offline_audio_context_length_matches_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, 3).unwrap();
            let len: f64 = ctx
                .eval("new OfflineAudioContext(1, 256, 44100).length")
                .unwrap();
            assert_eq!(len as usize, 256);
        });
    }
}
