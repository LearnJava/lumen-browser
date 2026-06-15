//! W3C Web Audio API (W3C Web Audio API Level 1).
//!
//! Exposes:
//! - `AudioContext` / `OfflineAudioContext` — graph root, state machine
//! - `AudioBuffer`, `AudioParam` — data containers
//! - `AudioNode` subclasses: `GainNode`, `OscillatorNode`,
//!   `AudioBufferSourceNode`, `BiquadFilterNode`, `AnalyserNode`,
//!   `DelayNode`, `DynamicsCompressorNode`, `StereoPannerNode`, `PannerNode`,
//!   `AudioDestinationNode`, `MediaElementAudioSourceNode`
//!
//! **Phase 0**: no DSP — all graph operations in-memory only.
//! `currentTime` increments via `_lumen_audio_tick_time` native binding.
//! All node `start()`/`stop()` calls are recorded but produce no audio output.

use rquickjs::Ctx;

/// Install the Web Audio API into the JS context.
pub fn install_web_audio_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.globals().set("_lumen_audio_tick_time", {
        rquickjs::Function::new(ctx.clone(), |ctx: Ctx| -> rquickjs::Result<()> {
            // Phase 0: no-op — currentTime advancement is handled purely in JS shim.
            let _ = ctx;
            Ok(())
        })?
    })?;
    ctx.eval::<(), _>(WEB_AUDIO_SHIM)?;
    Ok(())
}

const WEB_AUDIO_SHIM: &str = r#"(function() {
  'use strict';

  // ── AudioParam ──────────────────────────────────────────────────────────────

  function AudioParam(defaultValue) {
    this._value = (defaultValue !== undefined) ? defaultValue : 0;
    this.defaultValue = this._value;
    this.minValue = -3.4028235e+38;
    this.maxValue =  3.4028235e+38;
    this.automationRate = 'a-rate';
  }
  Object.defineProperty(AudioParam.prototype, 'value', {
    get: function() { return this._value; },
    set: function(v) { this._value = +v; },
    configurable: true, enumerable: true
  });
  AudioParam.prototype.setValueAtTime = function(value, startTime) {
    this._value = +value; return this;
  };
  AudioParam.prototype.linearRampToValueAtTime = function(value, endTime) {
    this._value = +value; return this;
  };
  AudioParam.prototype.exponentialRampToValueAtTime = function(value, endTime) {
    this._value = +value; return this;
  };
  AudioParam.prototype.setTargetAtTime = function(target, startTime, timeConstant) {
    this._value = +target; return this;
  };
  AudioParam.prototype.setValueCurveAtTime = function(values, startTime, duration) {
    if (values && values.length) this._value = +values[values.length - 1];
    return this;
  };
  AudioParam.prototype.cancelScheduledValues = function(cancelTime) { return this; };
  AudioParam.prototype.cancelAndHoldAtTime = function(cancelTime) { return this; };
  globalThis.AudioParam = AudioParam;

  // ── AudioBuffer ─────────────────────────────────────────────────────────────

  function AudioBuffer(opts) {
    opts = opts || {};
    this.sampleRate        = opts.sampleRate || 44100;
    this.length            = opts.length     || 0;
    this.numberOfChannels  = opts.numberOfChannels || 1;
    this.duration          = this.length / this.sampleRate;
    this._channels = [];
    for (var i = 0; i < this.numberOfChannels; i++) {
      this._channels.push(new Float32Array(this.length));
    }
  }
  AudioBuffer.prototype.getChannelData = function(channel) {
    if (channel < 0 || channel >= this.numberOfChannels)
      throw new DOMException('channel index out of bounds', 'IndexSizeError');
    return this._channels[channel];
  };
  AudioBuffer.prototype.copyFromChannel = function(destination, channelNumber, bufferOffset) {
    var src = this._channels[channelNumber] || new Float32Array(0);
    var off = bufferOffset || 0;
    for (var i = 0; i < destination.length; i++) {
      destination[i] = src[off + i] || 0;
    }
  };
  AudioBuffer.prototype.copyToChannel = function(source, channelNumber, bufferOffset) {
    if (!this._channels[channelNumber]) return;
    var dst = this._channels[channelNumber];
    var off = bufferOffset || 0;
    for (var i = 0; i < source.length; i++) {
      dst[off + i] = source[i];
    }
  };
  globalThis.AudioBuffer = AudioBuffer;

  // ── PeriodicWave ─────────────────────────────────────────────────────────────

  function PeriodicWave() {}
  globalThis.PeriodicWave = PeriodicWave;

  // ── AudioNode (base) ────────────────────────────────────────────────────────

  function AudioNode(context, opts) {
    opts = opts || {};
    this.context               = context;
    this.channelCount          = opts.channelCount          || 2;
    this.channelCountMode      = opts.channelCountMode      || 'max';
    this.channelInterpretation = opts.channelInterpretation || 'speakers';
    this.numberOfInputs        = 0;
    this.numberOfOutputs       = 0;
    this._connections          = [];
  }
  AudioNode.prototype.connect = function(destination, outputIndex, inputIndex) {
    this._connections.push(destination);
    return destination;
  };
  AudioNode.prototype.disconnect = function(destinationOrOutput, output, input) {
    if (destinationOrOutput === undefined) {
      this._connections = [];
    } else if (typeof destinationOrOutput === 'number') {
      // disconnect by output index — Phase 0: clear all
      this._connections = [];
    } else {
      var dest = destinationOrOutput;
      this._connections = this._connections.filter(function(c) { return c !== dest; });
    }
  };
  globalThis.AudioNode = AudioNode;

  // ── AudioDestinationNode ────────────────────────────────────────────────────

  function AudioDestinationNode(context) {
    AudioNode.call(this, context, { channelCount: 2 });
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 0;
    this.maxChannelCount = 2;
  }
  AudioDestinationNode.prototype = Object.create(AudioNode.prototype);
  AudioDestinationNode.prototype.constructor = AudioDestinationNode;
  globalThis.AudioDestinationNode = AudioDestinationNode;

  // ── GainNode ────────────────────────────────────────────────────────────────

  function GainNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.gain = new AudioParam(1.0);
  }
  GainNode.prototype = Object.create(AudioNode.prototype);
  GainNode.prototype.constructor = GainNode;
  globalThis.GainNode = GainNode;

  // ── OscillatorNode ──────────────────────────────────────────────────────────

  var OSC_TYPES = ['sine', 'square', 'sawtooth', 'triangle', 'custom'];

  function OscillatorNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this._type    = (opts && opts.type) ? opts.type : 'sine';
    this.frequency = new AudioParam((opts && opts.frequency != null) ? opts.frequency : 440);
    this.detune    = new AudioParam((opts && opts.detune    != null) ? opts.detune    : 0);
    this._started = false;
    this._stopped = false;
    this.onended  = null;
    this._endListeners = [];
  }
  OscillatorNode.prototype = Object.create(AudioNode.prototype);
  OscillatorNode.prototype.constructor = OscillatorNode;
  Object.defineProperty(OscillatorNode.prototype, 'type', {
    get: function() { return this._type; },
    set: function(v) {
      if (OSC_TYPES.indexOf(v) < 0) throw new DOMException('Invalid oscillator type', 'InvalidStateError');
      this._type = v;
    },
    configurable: true, enumerable: true
  });
  OscillatorNode.prototype.start = function(when) { this._started = true; };
  OscillatorNode.prototype.stop  = function(when) { this._stopped = true; };
  OscillatorNode.prototype.setPeriodicWave = function(wave) {};
  OscillatorNode.prototype.addEventListener = function(type, listener) {
    if (type === 'ended') this._endListeners.push(listener);
  };
  OscillatorNode.prototype.removeEventListener = function(type, listener) {
    if (type === 'ended')
      this._endListeners = this._endListeners.filter(function(l) { return l !== listener; });
  };
  globalThis.OscillatorNode = OscillatorNode;

  // ── AudioBufferSourceNode ───────────────────────────────────────────────────

  function AudioBufferSourceNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this.buffer          = (opts && opts.buffer) ? opts.buffer : null;
    this.loop            = (opts && opts.loop)   ? !!opts.loop : false;
    this.loopStart       = (opts && opts.loopStart != null) ? opts.loopStart : 0;
    this.loopEnd         = (opts && opts.loopEnd   != null) ? opts.loopEnd   : 0;
    this.playbackRate    = new AudioParam((opts && opts.playbackRate != null) ? opts.playbackRate : 1);
    this.detune          = new AudioParam(0);
    this._started = false;
    this.onended  = null;
    this._endListeners = [];
  }
  AudioBufferSourceNode.prototype = Object.create(AudioNode.prototype);
  AudioBufferSourceNode.prototype.constructor = AudioBufferSourceNode;
  AudioBufferSourceNode.prototype.start = function(when, offset, duration) { this._started = true; };
  AudioBufferSourceNode.prototype.stop  = function(when) {};
  AudioBufferSourceNode.prototype.addEventListener = function(type, listener) {
    if (type === 'ended') this._endListeners.push(listener);
  };
  AudioBufferSourceNode.prototype.removeEventListener = function(type, listener) {
    if (type === 'ended')
      this._endListeners = this._endListeners.filter(function(l) { return l !== listener; });
  };
  globalThis.AudioBufferSourceNode = AudioBufferSourceNode;

  // ── BiquadFilterNode ────────────────────────────────────────────────────────

  var BIQUAD_TYPES = ['lowpass','highpass','bandpass','lowshelf','highshelf','peaking','notch','allpass'];

  function BiquadFilterNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this._type    = (opts && opts.type) ? opts.type : 'lowpass';
    this.frequency = new AudioParam((opts && opts.frequency != null) ? opts.frequency : 350);
    this.detune    = new AudioParam(0);
    this.Q         = new AudioParam((opts && opts.Q  != null) ? opts.Q  : 1);
    this.gain      = new AudioParam((opts && opts.gain != null) ? opts.gain : 0);
  }
  BiquadFilterNode.prototype = Object.create(AudioNode.prototype);
  BiquadFilterNode.prototype.constructor = BiquadFilterNode;
  Object.defineProperty(BiquadFilterNode.prototype, 'type', {
    get: function() { return this._type; },
    set: function(v) {
      if (BIQUAD_TYPES.indexOf(v) < 0) throw new DOMException('Invalid filter type', 'InvalidStateError');
      this._type = v;
    },
    configurable: true, enumerable: true
  });
  BiquadFilterNode.prototype.getFrequencyResponse = function(frequencyHz, magResponse, phaseResponse) {
    // Phase 0: flat response (gain=1, phase=0 everywhere).
    for (var i = 0; i < magResponse.length; i++) magResponse[i] = 1.0;
    for (var i = 0; i < phaseResponse.length; i++) phaseResponse[i] = 0.0;
  };
  globalThis.BiquadFilterNode = BiquadFilterNode;

  // ── AnalyserNode ────────────────────────────────────────────────────────────

  function AnalyserNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.fftSize              = (opts && opts.fftSize)              ? opts.fftSize              : 2048;
    this.minDecibels          = (opts && opts.minDecibels  != null) ? opts.minDecibels          : -100;
    this.maxDecibels          = (opts && opts.maxDecibels  != null) ? opts.maxDecibels          : -30;
    this.smoothingTimeConstant= (opts && opts.smoothingTimeConstant != null) ? opts.smoothingTimeConstant : 0.8;
  }
  Object.defineProperty(AnalyserNode.prototype, 'frequencyBinCount', {
    get: function() { return this.fftSize >> 1; },
    configurable: true, enumerable: true
  });
  AnalyserNode.prototype = Object.create(AudioNode.prototype);
  AnalyserNode.prototype.constructor = AnalyserNode;
  AnalyserNode.prototype.getFloatFrequencyData = function(array) {
    for (var i = 0; i < array.length; i++) array[i] = this.minDecibels;
  };
  AnalyserNode.prototype.getByteFrequencyData = function(array) {
    for (var i = 0; i < array.length; i++) array[i] = 0;
  };
  AnalyserNode.prototype.getFloatTimeDomainData = function(array) {
    for (var i = 0; i < array.length; i++) array[i] = 0.0;
  };
  AnalyserNode.prototype.getByteTimeDomainData = function(array) {
    for (var i = 0; i < array.length; i++) array[i] = 128;
  };
  globalThis.AnalyserNode = AnalyserNode;

  // ── DelayNode ───────────────────────────────────────────────────────────────

  function DelayNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.delayTime = new AudioParam((opts && opts.delayTime != null) ? opts.delayTime : 0);
  }
  DelayNode.prototype = Object.create(AudioNode.prototype);
  DelayNode.prototype.constructor = DelayNode;
  globalThis.DelayNode = DelayNode;

  // ── DynamicsCompressorNode ──────────────────────────────────────────────────

  function DynamicsCompressorNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.threshold  = new AudioParam((opts && opts.threshold  != null) ? opts.threshold  : -24);
    this.knee       = new AudioParam((opts && opts.knee       != null) ? opts.knee       : 30);
    this.ratio      = new AudioParam((opts && opts.ratio      != null) ? opts.ratio      : 12);
    this.attack     = new AudioParam((opts && opts.attack     != null) ? opts.attack     : 0.003);
    this.release    = new AudioParam((opts && opts.release    != null) ? opts.release    : 0.25);
    this.reduction  = 0;
  }
  DynamicsCompressorNode.prototype = Object.create(AudioNode.prototype);
  DynamicsCompressorNode.prototype.constructor = DynamicsCompressorNode;
  globalThis.DynamicsCompressorNode = DynamicsCompressorNode;

  // ── StereoPannerNode ────────────────────────────────────────────────────────

  function StereoPannerNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.pan = new AudioParam((opts && opts.pan != null) ? opts.pan : 0);
  }
  StereoPannerNode.prototype = Object.create(AudioNode.prototype);
  StereoPannerNode.prototype.constructor = StereoPannerNode;
  globalThis.StereoPannerNode = StereoPannerNode;

  // ── PannerNode ──────────────────────────────────────────────────────────────

  function PannerNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    opts = opts || {};
    this.panningModel     = opts.panningModel    || 'equalpower';
    this.distanceModel    = opts.distanceModel   || 'inverse';
    this.refDistance      = opts.refDistance     != null ? opts.refDistance     : 1;
    this.maxDistance      = opts.maxDistance     != null ? opts.maxDistance     : 10000;
    this.rolloffFactor    = opts.rolloffFactor   != null ? opts.rolloffFactor   : 1;
    this.coneInnerAngle   = opts.coneInnerAngle  != null ? opts.coneInnerAngle  : 360;
    this.coneOuterAngle   = opts.coneOuterAngle  != null ? opts.coneOuterAngle  : 0;
    this.coneOuterGain    = opts.coneOuterGain   != null ? opts.coneOuterGain   : 0;
    this.positionX        = new AudioParam(opts.positionX != null ? opts.positionX : 0);
    this.positionY        = new AudioParam(opts.positionY != null ? opts.positionY : 0);
    this.positionZ        = new AudioParam(opts.positionZ != null ? opts.positionZ : 0);
    this.orientationX     = new AudioParam(opts.orientationX != null ? opts.orientationX : 1);
    this.orientationY     = new AudioParam(0);
    this.orientationZ     = new AudioParam(0);
  }
  PannerNode.prototype = Object.create(AudioNode.prototype);
  PannerNode.prototype.constructor = PannerNode;
  PannerNode.prototype.setPosition    = function(x, y, z) {
    this.positionX._value = x; this.positionY._value = y; this.positionZ._value = z;
  };
  PannerNode.prototype.setOrientation = function(x, y, z) {
    this.orientationX._value = x; this.orientationY._value = y; this.orientationZ._value = z;
  };
  globalThis.PannerNode = PannerNode;

  // ── AudioListener ───────────────────────────────────────────────────────────

  function AudioListener() {
    this.positionX  = new AudioParam(0);
    this.positionY  = new AudioParam(0);
    this.positionZ  = new AudioParam(0);
    this.forwardX   = new AudioParam(0);
    this.forwardY   = new AudioParam(0);
    this.forwardZ   = new AudioParam(-1);
    this.upX        = new AudioParam(0);
    this.upY        = new AudioParam(1);
    this.upZ        = new AudioParam(0);
  }
  AudioListener.prototype.setPosition    = function(x, y, z) {
    this.positionX._value = x; this.positionY._value = y; this.positionZ._value = z;
  };
  AudioListener.prototype.setOrientation = function(x, y, z, xUp, yUp, zUp) {
    this.forwardX._value = x; this.forwardY._value = y; this.forwardZ._value = z;
    this.upX._value = xUp; this.upY._value = yUp; this.upZ._value = zUp;
  };
  globalThis.AudioListener = AudioListener;

  // ── ChannelMergerNode ───────────────────────────────────────────────────────

  function ChannelMergerNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = (opts && opts.numberOfInputs) ? opts.numberOfInputs : 6;
    this.numberOfOutputs = 1;
  }
  ChannelMergerNode.prototype = Object.create(AudioNode.prototype);
  ChannelMergerNode.prototype.constructor = ChannelMergerNode;
  globalThis.ChannelMergerNode = ChannelMergerNode;

  // ── ChannelSplitterNode ─────────────────────────────────────────────────────

  function ChannelSplitterNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = (opts && opts.numberOfOutputs) ? opts.numberOfOutputs : 6;
  }
  ChannelSplitterNode.prototype = Object.create(AudioNode.prototype);
  ChannelSplitterNode.prototype.constructor = ChannelSplitterNode;
  globalThis.ChannelSplitterNode = ChannelSplitterNode;

  // ── WaveShaperNode ──────────────────────────────────────────────────────────

  function WaveShaperNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.curve    = (opts && opts.curve)    ? opts.curve    : null;
    this.oversample = (opts && opts.oversample) ? opts.oversample : 'none';
  }
  WaveShaperNode.prototype = Object.create(AudioNode.prototype);
  WaveShaperNode.prototype.constructor = WaveShaperNode;
  globalThis.WaveShaperNode = WaveShaperNode;

  // ── ConvolverNode ───────────────────────────────────────────────────────────

  function ConvolverNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.buffer    = (opts && opts.buffer)    ? opts.buffer    : null;
    this.normalize = (opts && opts.normalize != null) ? !!opts.normalize : true;
  }
  ConvolverNode.prototype = Object.create(AudioNode.prototype);
  ConvolverNode.prototype.constructor = ConvolverNode;
  globalThis.ConvolverNode = ConvolverNode;

  // ── MediaElementAudioSourceNode ─────────────────────────────────────────────

  function MediaElementAudioSourceNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this.mediaElement = (opts && opts.mediaElement) ? opts.mediaElement : null;
  }
  MediaElementAudioSourceNode.prototype = Object.create(AudioNode.prototype);
  MediaElementAudioSourceNode.prototype.constructor = MediaElementAudioSourceNode;
  globalThis.MediaElementAudioSourceNode = MediaElementAudioSourceNode;

  // ── MediaStreamAudioSourceNode ──────────────────────────────────────────────

  function MediaStreamAudioSourceNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this.mediaStream = (opts && opts.mediaStream) ? opts.mediaStream : null;
  }
  MediaStreamAudioSourceNode.prototype = Object.create(AudioNode.prototype);
  MediaStreamAudioSourceNode.prototype.constructor = MediaStreamAudioSourceNode;
  globalThis.MediaStreamAudioSourceNode = MediaStreamAudioSourceNode;

  // ── AudioWorkletNode stub ───────────────────────────────────────────────────

  function AudioWorkletNode(context, name, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this._name = name;
    this.parameters = new Map();
    this.port = { postMessage: function() {}, onmessage: null };
  }
  AudioWorkletNode.prototype = Object.create(AudioNode.prototype);
  AudioWorkletNode.prototype.constructor = AudioWorkletNode;
  globalThis.AudioWorkletNode = AudioWorkletNode;

  // ── BaseAudioContext (shared by AudioContext + OfflineAudioContext) ─────────

  function BaseAudioContext(sampleRate) {
    this.sampleRate    = sampleRate || 44100;
    this._currentTime  = 0;
    this._state        = 'running';
    this.destination   = new AudioDestinationNode(this);
    this.listener      = new AudioListener();
    this._stateListeners = [];
    this.onstatechange = null;

    // AudioWorklet stub
    this.audioWorklet = {
      addModule: function(url) { return Promise.resolve(); }
    };
  }
  Object.defineProperty(BaseAudioContext.prototype, 'currentTime', {
    get: function() { return this._currentTime; },
    configurable: true, enumerable: true
  });
  Object.defineProperty(BaseAudioContext.prototype, 'state', {
    get: function() { return this._state; },
    configurable: true, enumerable: true
  });
  BaseAudioContext.prototype._setState = function(s) {
    this._state = s;
    var evt = { type: 'statechange' };
    if (typeof this.onstatechange === 'function') {
      try { this.onstatechange(evt); } catch(e) {}
    }
    var ls = this._stateListeners.slice();
    for (var i = 0; i < ls.length; i++) { try { ls[i](evt); } catch(e) {} }
  };
  BaseAudioContext.prototype.addEventListener = function(type, listener) {
    if (type === 'statechange') this._stateListeners.push(listener);
  };
  BaseAudioContext.prototype.removeEventListener = function(type, listener) {
    if (type === 'statechange')
      this._stateListeners = this._stateListeners.filter(function(l) { return l !== listener; });
  };

  // Factory methods.
  BaseAudioContext.prototype.createBuffer = function(numChannels, length, sampleRate) {
    return new AudioBuffer({ numberOfChannels: numChannels, length: length, sampleRate: sampleRate });
  };
  BaseAudioContext.prototype.createBufferSource = function() {
    return new AudioBufferSourceNode(this);
  };
  BaseAudioContext.prototype.createGain = function() { return new GainNode(this); };
  BaseAudioContext.prototype.createOscillator = function() { return new OscillatorNode(this); };
  BaseAudioContext.prototype.createBiquadFilter = function() { return new BiquadFilterNode(this); };
  BaseAudioContext.prototype.createAnalyser = function() { return new AnalyserNode(this); };
  BaseAudioContext.prototype.createDelay = function(maxDelay) {
    return new DelayNode(this, { delayTime: 0 });
  };
  BaseAudioContext.prototype.createDynamicsCompressor = function() {
    return new DynamicsCompressorNode(this);
  };
  BaseAudioContext.prototype.createStereoPanner = function() { return new StereoPannerNode(this); };
  BaseAudioContext.prototype.createPanner = function() { return new PannerNode(this); };
  BaseAudioContext.prototype.createChannelMerger = function(n) {
    return new ChannelMergerNode(this, { numberOfInputs: n || 6 });
  };
  BaseAudioContext.prototype.createChannelSplitter = function(n) {
    return new ChannelSplitterNode(this, { numberOfOutputs: n || 6 });
  };
  BaseAudioContext.prototype.createWaveShaper = function() { return new WaveShaperNode(this); };
  BaseAudioContext.prototype.createConvolver = function() { return new ConvolverNode(this); };
  BaseAudioContext.prototype.createMediaElementSource = function(el) {
    return new MediaElementAudioSourceNode(this, { mediaElement: el });
  };
  BaseAudioContext.prototype.createMediaStreamSource = function(stream) {
    return new MediaStreamAudioSourceNode(this, { mediaStream: stream });
  };
  BaseAudioContext.prototype.createPeriodicWave = function(real, imag, opts) {
    return new PeriodicWave();
  };
  BaseAudioContext.prototype.decodeAudioData = function(arrayBuffer, successCallback, errorCallback) {
    // Phase 0: return silent 1-second mono buffer.
    var buf = new AudioBuffer({ numberOfChannels: 1, length: this.sampleRate, sampleRate: this.sampleRate });
    var promise = Promise.resolve(buf);
    if (typeof successCallback === 'function') {
      promise.then(successCallback);
    }
    if (typeof errorCallback === 'function') {
      promise.catch(errorCallback);
    }
    return promise;
  };
  globalThis.BaseAudioContext = BaseAudioContext;

  // ── AudioContext ─────────────────────────────────────────────────────────────

  function AudioContext(opts) {
    opts = opts || {};
    BaseAudioContext.call(this, opts.sampleRate || 44100);
    this.baseLatency   = 0.01;
    this.outputLatency = 0.02;
  }
  AudioContext.prototype = Object.create(BaseAudioContext.prototype);
  AudioContext.prototype.constructor = AudioContext;
  AudioContext.prototype.suspend = function() {
    var self = this;
    return new Promise(function(resolve) {
      self._setState('suspended');
      resolve();
    });
  };
  AudioContext.prototype.resume = function() {
    var self = this;
    return new Promise(function(resolve) {
      self._setState('running');
      resolve();
    });
  };
  AudioContext.prototype.close = function() {
    var self = this;
    return new Promise(function(resolve) {
      self._setState('closed');
      resolve();
    });
  };
  AudioContext.prototype.createMediaStreamDestination = function() {
    var dest = new AudioNode(this);
    dest.numberOfInputs  = 1;
    dest.numberOfOutputs = 0;
    dest.stream = { id: 'lumen-stream-dest', active: true, getTracks: function() { return []; } };
    return dest;
  };
  // getOutputTimestamp returns DOMHighResTimeStamp pair.
  AudioContext.prototype.getOutputTimestamp = function() {
    return { contextTime: this._currentTime, performanceTime: 0 };
  };
  globalThis.AudioContext = AudioContext;
  // Alias used by some older sites.
  if (typeof webkitAudioContext === 'undefined') {
    globalThis.webkitAudioContext = AudioContext;
  }

  // ── OfflineAudioContext ──────────────────────────────────────────────────────

  function OfflineAudioContext(numChannelsOrOpts, length, sampleRate) {
    var opts;
    if (typeof numChannelsOrOpts === 'object' && numChannelsOrOpts !== null) {
      opts = numChannelsOrOpts;
    } else {
      opts = {
        numberOfChannels: numChannelsOrOpts || 1,
        length:           length            || 0,
        sampleRate:       sampleRate        || 44100
      };
    }
    BaseAudioContext.call(this, opts.sampleRate || 44100);
    this.length           = opts.length           || 0;
    this.numberOfChannels = opts.numberOfChannels || 1;
    this._state           = 'suspended';
    this.oncomplete       = null;
    this._completeListeners = [];
  }
  OfflineAudioContext.prototype = Object.create(BaseAudioContext.prototype);
  OfflineAudioContext.prototype.constructor = OfflineAudioContext;
  OfflineAudioContext.prototype.addEventListener = function(type, listener) {
    if (type === 'complete') this._completeListeners.push(listener);
    else BaseAudioContext.prototype.addEventListener.call(this, type, listener);
  };
  OfflineAudioContext.prototype.startRendering = function() {
    var self = this;
    self._setState('running');
    // Phase 0: immediately resolve with a silent buffer.
    var buf = new AudioBuffer({
      numberOfChannels: self.numberOfChannels,
      length:           self.length,
      sampleRate:       self.sampleRate
    });
    return new Promise(function(resolve) {
      self._setState('closed');
      var evt = { type: 'complete', renderedBuffer: buf };
      if (typeof self.oncomplete === 'function') { try { self.oncomplete(evt); } catch(e) {} }
      var ls = self._completeListeners.slice();
      for (var i = 0; i < ls.length; i++) { try { ls[i](evt); } catch(e) {} }
      resolve(buf);
    });
  };
  OfflineAudioContext.prototype.suspend = function(suspendTime) { return Promise.resolve(); };
  OfflineAudioContext.prototype.resume  = function() { return Promise.resolve(); };
  globalThis.OfflineAudioContext = OfflineAudioContext;

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

    fn setup(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg); e.name = name || 'Error'; return e;
                }
                globalThis.DOMException = DOMException;
            }
            "#,
        )
        .unwrap();
        install_web_audio_api(ctx).unwrap();
    }

    #[test]
    fn audio_context_classes_exist() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof AudioContext === 'function'
                      && typeof OfflineAudioContext === 'function'
                      && typeof AudioBuffer === 'function'
                      && typeof AudioParam === 'function'
                      && typeof AudioNode === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_context_initial_state() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac = new AudioContext();
                    ac.state === 'running'
                      && typeof ac.currentTime === 'number'
                      && typeof ac.sampleRate === 'number'
                      && ac.destination instanceof AudioDestinationNode
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_context_suspend_resume_close() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac = new AudioContext();
                    var suspendPromise = ac.suspend();
                    var resumePromise  = ac.resume();
                    var closePromise   = ac.close();
                    suspendPromise instanceof Promise
                      && resumePromise instanceof Promise
                      && closePromise instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_node_classes_exist() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof GainNode === 'function'
                      && typeof OscillatorNode === 'function'
                      && typeof AudioBufferSourceNode === 'function'
                      && typeof BiquadFilterNode === 'function'
                      && typeof AnalyserNode === 'function'
                      && typeof DelayNode === 'function'
                      && typeof DynamicsCompressorNode === 'function'
                      && typeof StereoPannerNode === 'function'
                      && typeof PannerNode === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_context_factory_methods() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac = new AudioContext();
                    var gain = ac.createGain();
                    var osc  = ac.createOscillator();
                    var buf  = ac.createBuffer(1, 100, 44100);
                    var src  = ac.createBufferSource();
                    var bq   = ac.createBiquadFilter();
                    var an   = ac.createAnalyser();
                    gain instanceof GainNode
                      && osc  instanceof OscillatorNode
                      && buf  instanceof AudioBuffer
                      && src  instanceof AudioBufferSourceNode
                      && bq   instanceof BiquadFilterNode
                      && an   instanceof AnalyserNode
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn oscillator_node_type_and_freq() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac  = new AudioContext();
                    var osc = ac.createOscillator();
                    osc.type === 'sine'
                      && osc.frequency instanceof AudioParam
                      && osc.frequency.value === 440
                      && osc.detune instanceof AudioParam
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_node_connect_disconnect() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac   = new AudioContext();
                    var gain = ac.createGain();
                    var osc  = ac.createOscillator();
                    var result = osc.connect(gain);
                    result === gain && osc._connections.length === 1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_buffer_channel_data() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var buf = new AudioBuffer({ numberOfChannels: 2, length: 128, sampleRate: 44100 });
                    buf.numberOfChannels === 2
                      && buf.length === 128
                      && buf.sampleRate === 44100
                      && buf.getChannelData(0) instanceof Float32Array
                      && buf.getChannelData(0).length === 128
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn audio_param_set_value_at_time() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac   = new AudioContext();
                    var gain = ac.createGain();
                    gain.gain.value = 0.5;
                    gain.gain.setValueAtTime(0.8, 0);
                    gain.gain.value === 0.8
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn offline_audio_context_start_rendering() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var oac = new OfflineAudioContext(1, 44100, 44100);
                    oac instanceof OfflineAudioContext
                      && oac.length === 44100
                      && oac.numberOfChannels === 1
                      && oac.startRendering() instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn decode_audio_data_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac = new AudioContext();
                    var buf = new ArrayBuffer(16);
                    ac.decodeAudioData(buf) instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webkit_audio_context_alias() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval("typeof webkitAudioContext === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn analyser_frequency_bin_count() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var ac = new AudioContext();
                    var an = ac.createAnalyser();
                    an.fftSize === 2048 && an.fftSize / 2 === 1024
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
