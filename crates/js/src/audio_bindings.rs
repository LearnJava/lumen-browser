//! AudioContext API Phase 1 with per-session fingerprint noise (ADR-007 Layer 4, 9D.3).
//!
//! Injects the full W3C Web Audio API Level 2 graph into the QuickJS context:
//! - `BaseAudioContext` — shared methods for `AudioContext` and `OfflineAudioContext`
//! - `AudioContext` — real-time context with all standard node factories
//! - `OfflineAudioContext` — offline rendering context
//! - `AudioBuffer` / `AudioBufferSourceNode` — sample playback
//! - `AudioParam` — automatable parameter with scheduling
//! - `AudioNode` base — `connect/disconnect` chain
//! - `GainNode`, `BiquadFilterNode`, `OscillatorNode`, `AnalyserNode`
//! - `PannerNode`, `StereoPannerNode`, `ConvolverNode`, `DelayNode`
//! - `DynamicsCompressorNode`, `WaveShaperNode`, `IIRFilterNode`
//! - `ChannelSplitterNode`, `ChannelMergerNode`
//! - `MediaElementAudioSourceNode`, `MediaStreamAudioSourceNode`
//! - `MediaStreamAudioDestinationNode`
//! - `AudioWorklet` stub (`addModule` → Promise.resolve())
//! - `AudioWorkletNode` stub
//! - `AudioListener` — spatial audio listener position/orientation
//! - `AudioDestinationNode` — `ctx.destination`
//!
//! `getChannelData()`, `copyFromChannel()`, and `getFloatFrequencyData()`
//! add tiny per-session LCG noise (±1e-7) to returned samples, preventing audio
//! fingerprinting while preserving the API shape for feature detection.

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
pub fn new_session_seed() -> u32 {
    SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Install the complete Web Audio API Level 2 into the JS context.
///
/// Defines all standard `AudioContext` node factories, `AudioParam` scheduling,
/// `AudioNode` connect/disconnect, and the full class hierarchy.
/// The `seed` should be obtained via `new_session_seed()`.
pub fn install_audio_bindings(ctx: &Ctx, seed: u32) -> rquickjs::Result<()> {
    ctx.globals().set("_LUMEN_AUDIO_NOISE_SEED", seed)?;
    ctx.eval::<(), _>(AUDIO_SHIM)?;
    Ok(())
}

/// Complete Web Audio API Level 2 JavaScript shim.
const AUDIO_SHIM: &str = r#"(function(seed) {
  // --- LCG noise for fingerprint resistance ---
  var _s = seed >>> 0;
  if (_s === 0) _s = 2654435761;
  function _next() {
    _s = Math.imul(_s, 1664525) + 1013904223 | 0;
    _s = _s >>> 0;
    return (_s / 4294967295.0 - 0.5) * 2e-7;
  }

  // --- AudioParam ---
  // W3C Web Audio API §4.5: automatable parameter with scheduling
  function AudioParam(value) {
    this.value = (value !== undefined) ? +value : 0;
    this.defaultValue = this.value;
    this.minValue = -3.4028234663852886e+38;
    this.maxValue = 3.4028234663852886e+38;
    this.automationRate = 'a-rate';
    this._events = [];
  }
  AudioParam.prototype.setValueAtTime = function(v, t) {
    this._events.push({type:'set', value:+v, time:+t});
    return this;
  };
  AudioParam.prototype.linearRampToValueAtTime = function(v, t) {
    this._events.push({type:'linear', value:+v, time:+t});
    return this;
  };
  AudioParam.prototype.exponentialRampToValueAtTime = function(v, t) {
    this._events.push({type:'exp', value:+v, time:+t});
    return this;
  };
  AudioParam.prototype.setTargetAtTime = function(target, startTime, timeConstant) {
    this._events.push({type:'target', value:+target, time:+startTime, tc:+timeConstant});
    return this;
  };
  AudioParam.prototype.setValueCurveAtTime = function(values, startTime, duration) {
    this._events.push({type:'curve', values:values, time:+startTime, dur:+duration});
    return this;
  };
  AudioParam.prototype.cancelScheduledValues = function(startTime) {
    var t = +startTime;
    this._events = this._events.filter(function(e) { return e.time < t; });
    return this;
  };
  AudioParam.prototype.cancelAndHoldAtTime = function(cancelTime) {
    this._events = this._events.filter(function(e) { return e.time < +cancelTime; });
    return this;
  };

  // --- AudioNode base ---
  // W3C Web Audio API §1.8: base class for all audio processing graph nodes
  function AudioNode(ctx, opts) {
    this.context = ctx || null;
    this.channelCount = (opts && opts.channelCount) || 2;
    this.channelCountMode = (opts && opts.channelCountMode) || 'max';
    this.channelInterpretation = (opts && opts.channelInterpretation) || 'speakers';
    this.numberOfInputs = 0;
    this.numberOfOutputs = 1;
    this._connections = [];
  }
  AudioNode.prototype.connect = function(dest) {
    if (dest) this._connections.push(dest);
    return dest;
  };
  AudioNode.prototype.disconnect = function() {
    this._connections = [];
  };

  // --- AudioBuffer ---
  // W3C Web Audio API §4.4
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

  // --- AudioDestinationNode ---
  // W3C Web Audio API §1.11: ctx.destination
  function AudioDestinationNode(ctx) {
    AudioNode.call(this, ctx);
    this.numberOfInputs = 1;
    this.numberOfOutputs = 0;
    this.maxChannelCount = 2;
  }
  AudioDestinationNode.prototype = Object.create(AudioNode.prototype);
  AudioDestinationNode.prototype.constructor = AudioDestinationNode;

  // --- AudioListener ---
  // W3C Web Audio API §4.3
  function AudioListener() {
    this.positionX = new AudioParam(0);
    this.positionY = new AudioParam(0);
    this.positionZ = new AudioParam(0);
    this.forwardX = new AudioParam(0);
    this.forwardY = new AudioParam(0);
    this.forwardZ = new AudioParam(-1);
    this.upX = new AudioParam(0);
    this.upY = new AudioParam(1);
    this.upZ = new AudioParam(0);
  }
  AudioListener.prototype.setPosition = function() {};
  AudioListener.prototype.setOrientation = function() {};

  // --- AudioWorklet stub ---
  // W3C Web Audio API §6: addModule returns resolved Promise (Phase 0)
  function AudioWorklet() {}
  AudioWorklet.prototype.addModule = function() {
    return Promise.resolve();
  };

  // --- AudioWorkletNode stub ---
  // W3C Web Audio API §6.1
  function AudioWorkletNode(ctx, name, opts) {
    AudioNode.call(this, ctx);
    this.processorName = name;
    this.numberOfInputs = (opts && opts.numberOfInputs !== undefined) ? opts.numberOfInputs : 1;
    this.numberOfOutputs = (opts && opts.numberOfOutputs !== undefined) ? opts.numberOfOutputs : 1;
    this.parameters = new Map();
    this.port = { postMessage: function() {}, onmessage: null };
  }
  AudioWorkletNode.prototype = Object.create(AudioNode.prototype);
  AudioWorkletNode.prototype.constructor = AudioWorkletNode;

  // --- GainNode ---
  // W3C Web Audio API §1.16
  function GainNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.gain = new AudioParam((opts && opts.gain !== undefined) ? opts.gain : 1.0);
    this.numberOfInputs = 1;
  }
  GainNode.prototype = Object.create(AudioNode.prototype);
  GainNode.prototype.constructor = GainNode;

  // --- BiquadFilterNode ---
  // W3C Web Audio API §1.8: lowpass/highpass/bandpass/notch/allpass/peaking/lowshelf/highshelf
  function BiquadFilterNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.type = (opts && opts.type) || 'lowpass';
    this.frequency = new AudioParam((opts && opts.frequency !== undefined) ? opts.frequency : 350);
    this.detune = new AudioParam(0);
    this.Q = new AudioParam((opts && opts.Q !== undefined) ? opts.Q : 1.0);
    this.gain = new AudioParam((opts && opts.gain !== undefined) ? opts.gain : 0);
    this.numberOfInputs = 1;
  }
  BiquadFilterNode.prototype = Object.create(AudioNode.prototype);
  BiquadFilterNode.prototype.constructor = BiquadFilterNode;
  BiquadFilterNode.prototype.getFrequencyResponse = function(freq, magResp, phaseResp) {
    for (var i = 0; i < magResp.length; i++) { magResp[i] = 1.0; }
    for (var i = 0; i < phaseResp.length; i++) { phaseResp[i] = 0.0; }
  };

  // --- OscillatorNode ---
  // W3C Web Audio API §1.24
  function OscillatorNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.type = (opts && opts.type) || 'sine';
    this.frequency = new AudioParam((opts && opts.frequency !== undefined) ? opts.frequency : 440);
    this.detune = new AudioParam(0);
    this.numberOfInputs = 0;
    this._started = false;
    this._stopped = false;
    this.onended = null;
    this._listeners = {};
  }
  OscillatorNode.prototype = Object.create(AudioNode.prototype);
  OscillatorNode.prototype.constructor = OscillatorNode;
  OscillatorNode.prototype.start = function(when) { this._started = true; };
  OscillatorNode.prototype.stop = function(when) { this._stopped = true; };
  OscillatorNode.prototype.setPeriodicWave = function(wave) { this._wave = wave; };
  OscillatorNode.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  OscillatorNode.prototype.removeEventListener = function(type, fn) {
    if (this._listeners[type])
      this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };

  // --- AudioBufferSourceNode ---
  // W3C Web Audio API §1.5
  function AudioBufferSourceNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.buffer = (opts && opts.buffer) || null;
    this.playbackRate = new AudioParam((opts && opts.playbackRate !== undefined) ? opts.playbackRate : 1.0);
    this.detune = new AudioParam(0);
    this.loop = (opts && !!opts.loop) || false;
    this.loopStart = (opts && opts.loopStart) || 0;
    this.loopEnd = (opts && opts.loopEnd) || 0;
    this.numberOfInputs = 0;
    this._started = false;
    this.onended = null;
    this._listeners = {};
  }
  AudioBufferSourceNode.prototype = Object.create(AudioNode.prototype);
  AudioBufferSourceNode.prototype.constructor = AudioBufferSourceNode;
  AudioBufferSourceNode.prototype.start = function(when, offset, dur) { this._started = true; };
  AudioBufferSourceNode.prototype.stop = function(when) {};
  AudioBufferSourceNode.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  AudioBufferSourceNode.prototype.removeEventListener = function(type, fn) {
    if (this._listeners[type])
      this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };

  // --- AnalyserNode ---
  // W3C Web Audio API §1.3
  function AnalyserNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.fftSize = (opts && opts.fftSize) || 2048;
    this.frequencyBinCount = this.fftSize >>> 1;
    this.minDecibels = (opts && opts.minDecibels !== undefined) ? opts.minDecibels : -100;
    this.maxDecibels = (opts && opts.maxDecibels !== undefined) ? opts.maxDecibels : -30;
    this.smoothingTimeConstant = (opts && opts.smoothingTimeConstant !== undefined) ? opts.smoothingTimeConstant : 0.8;
    this.numberOfInputs = 1;
  }
  AnalyserNode.prototype = Object.create(AudioNode.prototype);
  AnalyserNode.prototype.constructor = AnalyserNode;
  AnalyserNode.prototype.getFloatFrequencyData = function(arr) {
    for (var i = 0; i < arr.length; i++) { arr[i] = this.minDecibels + _next(); }
  };
  AnalyserNode.prototype.getByteFrequencyData = function(arr) {
    for (var i = 0; i < arr.length; i++) { arr[i] = 0; }
  };
  AnalyserNode.prototype.getFloatTimeDomainData = function(arr) {
    for (var i = 0; i < arr.length; i++) { arr[i] = _next(); }
  };
  AnalyserNode.prototype.getByteTimeDomainData = function(arr) {
    for (var i = 0; i < arr.length; i++) { arr[i] = 128; }
  };

  // --- DynamicsCompressorNode ---
  // W3C Web Audio API §1.13
  function DynamicsCompressorNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.threshold = new AudioParam((opts && opts.threshold !== undefined) ? opts.threshold : -24);
    this.knee = new AudioParam((opts && opts.knee !== undefined) ? opts.knee : 30);
    this.ratio = new AudioParam((opts && opts.ratio !== undefined) ? opts.ratio : 12);
    this.reduction = 0;
    this.attack = new AudioParam((opts && opts.attack !== undefined) ? opts.attack : 0.003);
    this.release = new AudioParam((opts && opts.release !== undefined) ? opts.release : 0.25);
    this.numberOfInputs = 1;
  }
  DynamicsCompressorNode.prototype = Object.create(AudioNode.prototype);
  DynamicsCompressorNode.prototype.constructor = DynamicsCompressorNode;

  // --- DelayNode ---
  // W3C Web Audio API §1.12
  function DelayNode(ctx, opts) {
    AudioNode.call(this, ctx);
    var maxDelay = (opts && opts.maxDelayTime) || 1.0;
    this.delayTime = new AudioParam((opts && opts.delayTime !== undefined) ? opts.delayTime : 0);
    this.delayTime.maxValue = maxDelay;
    this.numberOfInputs = 1;
  }
  DelayNode.prototype = Object.create(AudioNode.prototype);
  DelayNode.prototype.constructor = DelayNode;

  // --- ConvolverNode ---
  // W3C Web Audio API §1.10
  function ConvolverNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.buffer = (opts && opts.buffer) || null;
    this.normalize = (opts && opts.normalize !== undefined) ? !!opts.normalize : true;
    this.numberOfInputs = 1;
  }
  ConvolverNode.prototype = Object.create(AudioNode.prototype);
  ConvolverNode.prototype.constructor = ConvolverNode;

  // --- PannerNode ---
  // W3C Web Audio API §1.25
  function PannerNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.panningModel = (opts && opts.panningModel) || 'equalpower';
    this.distanceModel = (opts && opts.distanceModel) || 'inverse';
    this.refDistance = (opts && opts.refDistance !== undefined) ? opts.refDistance : 1;
    this.maxDistance = (opts && opts.maxDistance !== undefined) ? opts.maxDistance : 10000;
    this.rolloffFactor = (opts && opts.rolloffFactor !== undefined) ? opts.rolloffFactor : 1;
    this.coneInnerAngle = (opts && opts.coneInnerAngle !== undefined) ? opts.coneInnerAngle : 360;
    this.coneOuterAngle = (opts && opts.coneOuterAngle !== undefined) ? opts.coneOuterAngle : 0;
    this.coneOuterGain = (opts && opts.coneOuterGain !== undefined) ? opts.coneOuterGain : 0;
    this.positionX = new AudioParam((opts && opts.positionX !== undefined) ? opts.positionX : 0);
    this.positionY = new AudioParam((opts && opts.positionY !== undefined) ? opts.positionY : 0);
    this.positionZ = new AudioParam((opts && opts.positionZ !== undefined) ? opts.positionZ : 0);
    this.orientationX = new AudioParam((opts && opts.orientationX !== undefined) ? opts.orientationX : 1);
    this.orientationY = new AudioParam(0);
    this.orientationZ = new AudioParam(0);
    this.numberOfInputs = 1;
  }
  PannerNode.prototype = Object.create(AudioNode.prototype);
  PannerNode.prototype.constructor = PannerNode;
  PannerNode.prototype.setPosition = function(x, y, z) {
    this.positionX.value = +x; this.positionY.value = +y; this.positionZ.value = +z;
  };
  PannerNode.prototype.setOrientation = function(x, y, z) {
    this.orientationX.value = +x; this.orientationY.value = +y; this.orientationZ.value = +z;
  };

  // --- StereoPannerNode ---
  // W3C Web Audio API §1.27
  function StereoPannerNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.pan = new AudioParam((opts && opts.pan !== undefined) ? opts.pan : 0);
    this.numberOfInputs = 1;
  }
  StereoPannerNode.prototype = Object.create(AudioNode.prototype);
  StereoPannerNode.prototype.constructor = StereoPannerNode;

  // --- WaveShaperNode ---
  // W3C Web Audio API §1.29
  function WaveShaperNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.curve = (opts && opts.curve) || null;
    this.oversample = (opts && opts.oversample) || 'none';
    this.numberOfInputs = 1;
  }
  WaveShaperNode.prototype = Object.create(AudioNode.prototype);
  WaveShaperNode.prototype.constructor = WaveShaperNode;

  // --- IIRFilterNode ---
  // W3C Web Audio API §1.17
  function IIRFilterNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this._feedforward = (opts && opts.feedforward) || [];
    this._feedback = (opts && opts.feedback) || [];
    this.numberOfInputs = 1;
  }
  IIRFilterNode.prototype = Object.create(AudioNode.prototype);
  IIRFilterNode.prototype.constructor = IIRFilterNode;
  IIRFilterNode.prototype.getFrequencyResponse = function(freq, magResp, phaseResp) {
    for (var i = 0; i < magResp.length; i++) { magResp[i] = 1.0; }
    for (var i = 0; i < phaseResp.length; i++) { phaseResp[i] = 0.0; }
  };

  // --- ChannelSplitterNode ---
  // W3C Web Audio API §1.9
  function ChannelSplitterNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.channelCount = 1;
    this.channelCountMode = 'explicit';
    this.channelInterpretation = 'discrete';
    this.numberOfInputs = 1;
    this.numberOfOutputs = (opts && opts.numberOfOutputs) || 6;
  }
  ChannelSplitterNode.prototype = Object.create(AudioNode.prototype);
  ChannelSplitterNode.prototype.constructor = ChannelSplitterNode;

  // --- ChannelMergerNode ---
  // W3C Web Audio API §1.9
  function ChannelMergerNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.channelCount = 1;
    this.channelCountMode = 'explicit';
    this.channelInterpretation = 'speakers';
    this.numberOfInputs = (opts && opts.numberOfInputs) || 6;
    this.numberOfOutputs = 1;
  }
  ChannelMergerNode.prototype = Object.create(AudioNode.prototype);
  ChannelMergerNode.prototype.constructor = ChannelMergerNode;

  // --- MediaElementAudioSourceNode ---
  // W3C Web Audio API §4.9
  function MediaElementAudioSourceNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.mediaElement = (opts && opts.mediaElement) || null;
    this.numberOfInputs = 0;
  }
  MediaElementAudioSourceNode.prototype = Object.create(AudioNode.prototype);
  MediaElementAudioSourceNode.prototype.constructor = MediaElementAudioSourceNode;

  // --- MediaStreamAudioSourceNode ---
  // W3C Web Audio API §4.10
  function MediaStreamAudioSourceNode(ctx, opts) {
    AudioNode.call(this, ctx);
    this.mediaStream = (opts && opts.mediaStream) || null;
    this.numberOfInputs = 0;
  }
  MediaStreamAudioSourceNode.prototype = Object.create(AudioNode.prototype);
  MediaStreamAudioSourceNode.prototype.constructor = MediaStreamAudioSourceNode;

  // --- MediaStreamAudioDestinationNode ---
  // W3C Web Audio API §4.11
  function MediaStreamAudioDestinationNode(ctx) {
    AudioNode.call(this, ctx);
    this.stream = { id: 'lumen-audio-dest', active: true, getTracks: function() { return []; } };
    this.numberOfInputs = 1;
    this.numberOfOutputs = 0;
  }
  MediaStreamAudioDestinationNode.prototype = Object.create(AudioNode.prototype);
  MediaStreamAudioDestinationNode.prototype.constructor = MediaStreamAudioDestinationNode;

  // --- PeriodicWave ---
  // W3C Web Audio API §4.15
  function PeriodicWave() {}

  // --- BaseAudioContext factory methods (shared by AudioContext and OfflineAudioContext) ---
  function _installBaseFactories(proto) {
    proto.createBuffer = function(channels, length, sampleRate) {
      return new AudioBuffer({ numberOfChannels: channels, length: length, sampleRate: sampleRate });
    };
    proto.createGain = function(opts) { return new GainNode(this, opts); };
    proto.createBiquadFilter = function(opts) { return new BiquadFilterNode(this, opts); };
    proto.createOscillator = function(opts) { return new OscillatorNode(this, opts); };
    proto.createBufferSource = function(opts) { return new AudioBufferSourceNode(this, opts); };
    proto.createAnalyser = function(opts) { return new AnalyserNode(this, opts); };
    proto.createDynamicsCompressor = function(opts) { return new DynamicsCompressorNode(this, opts); };
    proto.createDelay = function(maxDelayTime) { return new DelayNode(this, {maxDelayTime: maxDelayTime}); };
    proto.createConvolver = function(opts) { return new ConvolverNode(this, opts); };
    proto.createPanner = function(opts) { return new PannerNode(this, opts); };
    proto.createStereoPanner = function(opts) { return new StereoPannerNode(this, opts); };
    proto.createWaveShaper = function(opts) { return new WaveShaperNode(this, opts); };
    proto.createIIRFilter = function(feedforward, feedback) {
      return new IIRFilterNode(this, {feedforward: feedforward, feedback: feedback});
    };
    proto.createChannelSplitter = function(n) { return new ChannelSplitterNode(this, {numberOfOutputs: n || 6}); };
    proto.createChannelMerger = function(n) { return new ChannelMergerNode(this, {numberOfInputs: n || 6}); };
    proto.createPeriodicWave = function() { return new PeriodicWave(); };
    proto.decodeAudioData = function(buffer, successCb) {
      // Phase 0: returns a silent mono buffer; real decoding needs native codec support
      var buf = new AudioBuffer({ numberOfChannels: 1, length: 4096, sampleRate: this.sampleRate });
      if (typeof successCb === 'function') {
        Promise.resolve().then(function() { successCb(buf); });
        return undefined;
      }
      return Promise.resolve(buf);
    };
  }

  // --- AudioContext ---
  // W3C Web Audio API §1.2
  function AudioContext(opts) {
    this.sampleRate = (opts && opts.sampleRate) || 44100;
    this.state = 'running';
    this.currentTime = 0;
    this.baseLatency = 0.01;
    this.outputLatency = 0.02;
    this.destination = new AudioDestinationNode(this);
    this.listener = new AudioListener();
    this.audioWorklet = new AudioWorklet();
    this._listeners = {};
  }
  _installBaseFactories(AudioContext.prototype);
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
  AudioContext.prototype.createMediaElementSource = function(elem) {
    return new MediaElementAudioSourceNode(this, {mediaElement: elem});
  };
  AudioContext.prototype.createMediaStreamSource = function(stream) {
    return new MediaStreamAudioSourceNode(this, {mediaStream: stream});
  };
  AudioContext.prototype.createMediaStreamDestination = function() {
    return new MediaStreamAudioDestinationNode(this);
  };
  AudioContext.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  AudioContext.prototype.removeEventListener = function(type, fn) {
    if (this._listeners[type])
      this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };
  AudioContext.prototype.dispatchEvent = function(evt) {
    var list = this._listeners[evt && evt.type] || [];
    for (var i = 0; i < list.length; i++) { try { list[i](evt); } catch(e) {} }
    return true;
  };

  // --- OfflineAudioContext ---
  // W3C Web Audio API §1.23
  function OfflineAudioContext(channels, length, sampleRate) {
    // Accept either (channels, length, sampleRate) or {numberOfChannels, length, sampleRate}
    if (typeof channels === 'object' && channels !== null) {
      var opts = channels;
      this._channels = opts.numberOfChannels || 1;
      this._length = opts.length || 0;
      this.sampleRate = opts.sampleRate || 44100;
    } else {
      this._channels = channels || 1;
      this._length = length || 0;
      this.sampleRate = sampleRate || 44100;
    }
    this.length = this._length;
    this.currentTime = 0;
    this.state = 'suspended';
    this.destination = new AudioDestinationNode(this);
    this.listener = new AudioListener();
    this.audioWorklet = new AudioWorklet();
    this.oncomplete = null;
    this._listeners = {};
  }
  _installBaseFactories(OfflineAudioContext.prototype);
  OfflineAudioContext.prototype.startRendering = function() {
    var self = this;
    self.state = 'running';
    var buf = new AudioBuffer({
      numberOfChannels: self._channels,
      length: self._length,
      sampleRate: self.sampleRate
    });
    var evt = { renderedBuffer: buf, target: self };
    return new Promise(function(resolve) {
      Promise.resolve().then(function() {
        self.state = 'closed';
        if (typeof self.oncomplete === 'function') { self.oncomplete(evt); }
        var list = self._listeners['complete'] || [];
        for (var i = 0; i < list.length; i++) { try { list[i](evt); } catch(e) {} }
        resolve(buf);
      });
    });
  };
  OfflineAudioContext.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  OfflineAudioContext.prototype.removeEventListener = function(type, fn) {
    if (this._listeners[type])
      this._listeners[type] = this._listeners[type].filter(function(f) { return f !== fn; });
  };
  OfflineAudioContext.prototype.suspend = function() { return Promise.resolve(); };
  OfflineAudioContext.prototype.resume = function() { return Promise.resolve(); };

  // --- Export all to globalThis ---
  globalThis.AudioBuffer = AudioBuffer;
  globalThis.AudioParam = AudioParam;
  globalThis.AudioNode = AudioNode;
  globalThis.AudioDestinationNode = AudioDestinationNode;
  globalThis.AudioListener = AudioListener;
  globalThis.AudioWorklet = AudioWorklet;
  globalThis.AudioWorkletNode = AudioWorkletNode;
  globalThis.GainNode = GainNode;
  globalThis.BiquadFilterNode = BiquadFilterNode;
  globalThis.OscillatorNode = OscillatorNode;
  globalThis.AudioBufferSourceNode = AudioBufferSourceNode;
  globalThis.AnalyserNode = AnalyserNode;
  globalThis.DynamicsCompressorNode = DynamicsCompressorNode;
  globalThis.DelayNode = DelayNode;
  globalThis.ConvolverNode = ConvolverNode;
  globalThis.PannerNode = PannerNode;
  globalThis.StereoPannerNode = StereoPannerNode;
  globalThis.WaveShaperNode = WaveShaperNode;
  globalThis.IIRFilterNode = IIRFilterNode;
  globalThis.ChannelSplitterNode = ChannelSplitterNode;
  globalThis.ChannelMergerNode = ChannelMergerNode;
  globalThis.MediaElementAudioSourceNode = MediaElementAudioSourceNode;
  globalThis.MediaStreamAudioSourceNode = MediaStreamAudioSourceNode;
  globalThis.MediaStreamAudioDestinationNode = MediaStreamAudioDestinationNode;
  globalThis.PeriodicWave = PeriodicWave;
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

    fn install(ctx: &Context, seed: u32) {
        ctx.with(|ctx| {
            install_audio_bindings(&ctx, seed).unwrap();
        });
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
        install(&ctx, 42);
    }

    #[test]
    fn audio_context_is_defined() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 1);
        ctx.with(|ctx| {
            let ty: String = ctx.eval("typeof AudioContext").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn webkit_audio_context_alias() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 1);
        ctx.with(|ctx| {
            let same: bool = ctx.eval("AudioContext === webkitAudioContext").unwrap();
            assert!(same);
        });
    }

    #[test]
    fn offline_audio_context_is_defined() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 1);
        ctx.with(|ctx| {
            let ty: String = ctx.eval("typeof OfflineAudioContext").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn audio_buffer_is_defined() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 1);
        ctx.with(|ctx| {
            let ty: String = ctx.eval("typeof AudioBuffer").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn audio_buffer_get_channel_data_length() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 42);
        ctx.with(|ctx| {
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
        install(&ctx, 7);
        ctx.with(|ctx| {
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
        install(&ctx, 1);
        ctx.with(|ctx| {
            let state: String = ctx
                .eval("(function() { var a = new AudioContext(); return a.state; })()")
                .unwrap();
            assert_eq!(state, "running");
        });
    }

    #[test]
    fn analyser_frequency_bin_count() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 5);
        ctx.with(|ctx| {
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
        install(&ctx, 3);
        ctx.with(|ctx| {
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
        install(&ctx, 3);
        ctx.with(|ctx| {
            let len: f64 = ctx
                .eval("new OfflineAudioContext(1, 256, 44100).length")
                .unwrap();
            assert_eq!(len as usize, 256);
        });
    }

    // --- Phase 1 tests ---

    #[test]
    fn audio_context_has_destination() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 10);
        ctx.with(|ctx| {
            let ty: String = ctx
                .eval("typeof new AudioContext().destination")
                .unwrap();
            assert_eq!(ty, "object");
        });
    }

    #[test]
    fn audio_context_has_listener() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 10);
        ctx.with(|ctx| {
            let ty: String = ctx
                .eval("typeof new AudioContext().listener")
                .unwrap();
            assert_eq!(ty, "object");
        });
    }

    #[test]
    fn audio_context_has_audio_worklet() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 10);
        ctx.with(|ctx| {
            let ty: String = ctx
                .eval("typeof new AudioContext().audioWorklet")
                .unwrap();
            assert_eq!(ty, "object");
            let add_module: String = ctx
                .eval("typeof new AudioContext().audioWorklet.addModule")
                .unwrap();
            assert_eq!(add_module, "function");
        });
    }

    #[test]
    fn create_gain_returns_node_with_gain_param() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 11);
        ctx.with(|ctx| {
            let val: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var g = ctx.createGain(); \
                       return g.gain.value; \
                     })()",
                )
                .unwrap();
            assert!((val - 1.0).abs() < 1e-9, "default gain should be 1.0, got {val}");
        });
    }

    #[test]
    fn create_biquad_filter_has_frequency_param() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 12);
        ctx.with(|ctx| {
            let val: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var f = ctx.createBiquadFilter(); \
                       return f.frequency.value; \
                     })()",
                )
                .unwrap();
            assert!((val - 350.0).abs() < 1e-9, "default frequency should be 350, got {val}");
        });
    }

    #[test]
    fn create_oscillator_and_connect() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 13);
        ctx.with(|ctx| {
            let started: bool = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var osc = ctx.createOscillator(); \
                       osc.connect(ctx.destination); \
                       osc.start(); \
                       return osc._started; \
                     })()",
                )
                .unwrap();
            assert!(started, "oscillator._started should be true after start()");
        });
    }

    #[test]
    fn create_buffer_source_has_playback_rate() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 14);
        ctx.with(|ctx| {
            let val: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var src = ctx.createBufferSource(); \
                       return src.playbackRate.value; \
                     })()",
                )
                .unwrap();
            assert!((val - 1.0).abs() < 1e-9, "default playbackRate should be 1.0");
        });
    }

    #[test]
    fn create_stereo_panner_has_pan_param() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 15);
        ctx.with(|ctx| {
            let val: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var p = ctx.createStereoPanner(); \
                       return p.pan.value; \
                     })()",
                )
                .unwrap();
            assert!((val - 0.0).abs() < 1e-9, "default pan should be 0.0");
        });
    }

    #[test]
    fn create_delay_has_delay_time_param() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 16);
        ctx.with(|ctx| {
            let val: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var d = ctx.createDelay(5.0); \
                       return d.delayTime.value; \
                     })()",
                )
                .unwrap();
            assert!((val - 0.0).abs() < 1e-9, "default delayTime should be 0.0");
        });
    }

    #[test]
    fn create_channel_splitter_and_merger() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 17);
        ctx.with(|ctx| {
            let outputs: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var s = ctx.createChannelSplitter(4); \
                       return s.numberOfOutputs; \
                     })()",
                )
                .unwrap();
            assert_eq!(outputs as usize, 4);
            let inputs: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var m = ctx.createChannelMerger(4); \
                       return m.numberOfInputs; \
                     })()",
                )
                .unwrap();
            assert_eq!(inputs as usize, 4);
        });
    }

    #[test]
    fn audio_param_scheduling() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 18);
        ctx.with(|ctx| {
            let len: f64 = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var g = ctx.createGain(); \
                       g.gain.setValueAtTime(0.5, 0.0); \
                       g.gain.linearRampToValueAtTime(1.0, 1.0); \
                       return g.gain._events.length; \
                     })()",
                )
                .unwrap();
            assert_eq!(len as usize, 2);
        });
    }

    #[test]
    fn audio_worklet_node_is_defined() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 19);
        ctx.with(|ctx| {
            let ty: String = ctx.eval("typeof AudioWorkletNode").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn all_node_classes_exported() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 20);
        ctx.with(|ctx| {
            let classes = [
                "GainNode", "BiquadFilterNode", "OscillatorNode", "AudioBufferSourceNode",
                "AnalyserNode", "DynamicsCompressorNode", "DelayNode", "ConvolverNode",
                "PannerNode", "StereoPannerNode", "WaveShaperNode", "IIRFilterNode",
                "ChannelSplitterNode", "ChannelMergerNode",
                "MediaElementAudioSourceNode", "MediaStreamAudioSourceNode",
                "MediaStreamAudioDestinationNode", "PeriodicWave",
                "AudioWorkletNode", "AudioDestinationNode", "AudioListener",
            ];
            for cls in &classes {
                let expr = format!("typeof {cls}");
                let ty: String = ctx.eval(expr.as_str()).unwrap();
                assert_eq!(ty, "function", "{cls} should be a function");
            }
        });
    }

    #[test]
    fn offline_context_object_opts() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 21);
        ctx.with(|ctx| {
            let len: f64 = ctx
                .eval(
                    "new OfflineAudioContext({numberOfChannels:2, length:512, sampleRate:48000}).length",
                )
                .unwrap();
            assert_eq!(len as usize, 512);
        });
    }

    #[test]
    fn media_stream_destination_has_stream() {
        let (_rt, ctx) = make_ctx();
        install(&ctx, 22);
        ctx.with(|ctx| {
            let ty: String = ctx
                .eval(
                    "(function() { \
                       var ctx = new AudioContext(); \
                       var dest = ctx.createMediaStreamDestination(); \
                       return typeof dest.stream; \
                     })()",
                )
                .unwrap();
            assert_eq!(ty, "object");
        });
    }
}
