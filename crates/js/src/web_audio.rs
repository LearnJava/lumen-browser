//! W3C Web Audio API (W3C Web Audio API Level 1).
//!
//! Exposes:
//! - `AudioContext` / `OfflineAudioContext` — graph root, state machine
//! - `AudioBuffer`, `AudioParam` — data containers, with a real automation
//!   timeline (`setValueAtTime`/ramps/`setTargetAtTime`/`setValueCurveAtTime`)
//! - `AudioNode` subclasses: `GainNode`, `OscillatorNode`,
//!   `AudioBufferSourceNode`, `ConstantSourceNode`, `BiquadFilterNode`,
//!   `AnalyserNode`, `DelayNode`, `DynamicsCompressorNode`,
//!   `StereoPannerNode`, `PannerNode`, `WaveShaperNode`, `ChannelMergerNode`,
//!   `ChannelSplitterNode`, `AudioDestinationNode`,
//!   `MediaElementAudioSourceNode`
//!
//! `OfflineAudioContext.startRendering()` runs a real pull-based graph
//! traversal in 128-frame render quanta (BUG-828): sources synthesize,
//! `AudioParam` automation is sampled a-rate, and the mix is written into the
//! rendered `AudioBuffer`. `suspend(t)`/`resume()` stop and continue that loop
//! at a render-quantum boundary, and a source node fires `ended` when its
//! scheduled stop time — or its buffer — runs out.
//!
//! **Not rendered:** `DynamicsCompressorNode`, `PannerNode`, `ConvolverNode`
//! and `AudioWorkletNode` pass their input through unchanged, and a realtime
//! `AudioContext` still makes no sound — it only advances `currentTime` and
//! schedules `ended`, since nothing binds it to an output device.

/// Install the W3C Web Audio API Level 1 into a V8 context (Ph3 V8 migration
/// S5-S7 batch 2). The rquickjs twin (`install_web_audio_api`) was removed in
/// S12b-B19 — this is now the only backend.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_web_audio_api_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn0;
    use lumen_core::ext::JsRuntime as _;

    // Realtime `currentTime` advances from the wall clock inside the shim; the
    // binding is kept so an embedder can still poke the context per frame.
    let native = into_v8_fn0(move || {});
    rt.register_native("_lumen_audio_tick_time", native)?;
    rt.eval(WEB_AUDIO_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const WEB_AUDIO_SHIM: &str = r#"(function() {
  'use strict';

  // ── infrastructure ──────────────────────────────────────────────────────────

  // Every render quantum is 128 frames (Web Audio §1, "render quantum size").
  var RENDER_QUANTUM = 128;

  // A callback the engine makes on the page's behalf is queued as an
  // event-loop task, never dispatched inline — BUG-808's lesson: WPT arms its
  // EventWatcher *after* the call that triggers the event, so a synchronous
  // dispatch arrives with nothing listening and reads as "Not expecting event".
  //
  // A runtime with no DOM (`--dump-*`, SVG rasterization, this module's own
  // unit tests) has no `setTimeout` at all; there the call is made inline,
  // which is the one configuration where no page can observe the difference.
  function _wa_task(fn) {
    if (typeof setTimeout === 'function') { setTimeout(fn, 0); return; }
    fn();
  }

  // BUG-591: an exception thrown by a page handler goes into the ordinary
  // "report the exception" path, never into a bare `catch`. The call is
  // typeof-guarded because a DOM-less runtime has no reporter, and an
  // unguarded reference would throw a ReferenceError out of the `catch` and
  // take the rest of the dispatch loop with it.
  function _wa_report(e) {
    if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e);
  }
  function _wa_invoke(fn, thisArg, arg) {
    try { fn.call(thisArg, arg); } catch (e) { _wa_report(e); }
  }

  function _wa_error(message, name) {
    if (typeof DOMException === 'function') return new DOMException(message, name);
    var e = new Error(message); e.name = name || 'Error'; return e;
  }

  // Shared EventTarget-ish mixin: `on<type>` plus a listener registry. These
  // objects are not wired into the page's own `EventTarget` hierarchy (that
  // lives in `WEB_API_SHIM_MID`, which drags `document`/`window` in with it),
  // so they carry their own minimal dispatch.
  function _wa_events(proto) {
    proto.addEventListener = function(type, listener) {
      if (typeof listener !== 'function') return;
      if (!this._listeners) this._listeners = {};
      if (!this._listeners[type]) this._listeners[type] = [];
      this._listeners[type].push(listener);
    };
    proto.removeEventListener = function(type, listener) {
      if (!this._listeners || !this._listeners[type]) return;
      this._listeners[type] = this._listeners[type].filter(function(l) { return l !== listener; });
    };
    proto.dispatchEvent = function(evt) {
      this._wa_fire(evt && evt.type, evt);
      return true;
    };
    proto._wa_fire = function(type, evt) {
      evt = evt || { type: type };
      if (evt.target === undefined) evt.target = this;
      if (evt.currentTarget === undefined) evt.currentTarget = this;
      var handler = this['on' + type];
      if (typeof handler === 'function') _wa_invoke(handler, this, evt);
      var ls = (this._listeners && this._listeners[type]) ? this._listeners[type].slice() : [];
      for (var i = 0; i < ls.length; i++) _wa_invoke(ls[i], this, evt);
    };
  }

  // ── AudioParam ──────────────────────────────────────────────────────────────

  // An automation event is stored verbatim and compiled into `_segs` on first
  // read; every scheduling call invalidates that compilation.
  function AudioParam(defaultValue, opts) {
    opts = opts || {};
    this._value         = (defaultValue !== undefined) ? +defaultValue : 0;
    this.defaultValue   = this._value;
    this.minValue       = (opts.minValue !== undefined) ? opts.minValue : -3.4028235e+38;
    this.maxValue       = (opts.maxValue !== undefined) ? opts.maxValue :  3.4028235e+38;
    this.automationRate = opts.automationRate || 'a-rate';
    this._events = [];
    this._segs   = null;
    this._inputs = [];   // {node, output} — a node connected into this param
    this._ctx    = opts.context || null;
  }

  function _wa_param(ctx, defaultValue, opts) {
    opts = opts || {};
    opts.context = ctx;
    return new AudioParam(defaultValue, opts);
  }

  // Value of one compiled segment at time `t` (Web Audio §1.6.3).
  function _segValue(seg, t) {
    switch (seg.type) {
      case 'set':
        return seg.v1;
      case 'lin':
        if (t <= seg.t0) return seg.v0;
        if (t >= seg.t1) return seg.v1;
        if (seg.t1 === seg.t0) return seg.v1;
        return seg.v0 + (seg.v1 - seg.v0) * (t - seg.t0) / (seg.t1 - seg.t0);
      case 'exp':
        if (t <= seg.t0) return seg.v0;
        if (t >= seg.t1) return seg.v1;
        // Spec: with V0 zero, or the two values of opposite sign, the curve is
        // undefined and the value simply holds at V0 for the whole interval.
        if (seg.v0 === 0 || seg.v1 === 0 || (seg.v0 < 0) !== (seg.v1 < 0)) return seg.v0;
        if (seg.t1 === seg.t0) return seg.v1;
        return seg.v0 * Math.pow(seg.v1 / seg.v0, (t - seg.t0) / (seg.t1 - seg.t0));
      case 'target':
        if (t <= seg.t0) return seg.v0;
        if (!(seg.tau > 0)) return seg.v1;
        return seg.v1 + (seg.v0 - seg.v1) * Math.exp(-(t - seg.t0) / seg.tau);
      case 'curve': {
        var c = seg.curve, n = c.length;
        if (n === 0) return 0;
        if (t <= seg.t0) return c[0];
        if (t >= seg.t1) return c[n - 1];
        var x = (n - 1) * (t - seg.t0) / (seg.t1 - seg.t0);
        var k = Math.floor(x);
        if (k >= n - 1) return c[n - 1];
        return c[k] + (c[k + 1] - c[k]) * (x - k);
      }
      default:
        return seg.v1;
    }
  }

  AudioParam.prototype._build = function() {
    var evs = this._events, segs = [], prev = null;
    for (var i = 0; i < evs.length; i++) {
      var e = evs[i], seg;
      // T0/V0 of a ramp are "the time of the previous event, and the value at
      // that time"; with no previous event the ramp starts at 0 from the
      // param's intrinsic value.
      var startT = prev ? (isFinite(prev.t1) ? prev.t1 : prev.t0) : 0;
      var startV = prev ? _segValue(prev, startT) : this._value;
      if (e.type === 'set') {
        seg = { type: 'set', t0: e.time, t1: e.time, v0: e.value, v1: e.value };
      } else if (e.type === 'lin' || e.type === 'exp') {
        seg = { type: e.type, t0: startT, t1: e.time, v0: startV, v1: e.value };
      } else if (e.type === 'target') {
        seg = { type: 'target', t0: e.time, t1: Infinity, v0: startV, v1: e.value, tau: e.tau };
      } else {
        var c = e.curve;
        seg = {
          type: 'curve', t0: e.time, t1: e.time + e.duration, curve: c,
          v0: c.length ? c[0] : 0, v1: c.length ? c[c.length - 1] : 0
        };
      }
      segs.push(seg);
      prev = seg;
    }
    this._segs = segs;
  };

  AudioParam.prototype._valueAt = function(t) {
    if (!this._segs) this._build();
    var segs = this._segs, v;
    if (segs.length === 0) {
      v = this._value;
    } else {
      var pick = null;
      for (var i = 0; i < segs.length; i++) {
        if (segs[i].t0 <= t) pick = segs[i]; else break;
      }
      // Before the first event the intrinsic value holds — unless that event
      // is a ramp, whose segment already starts at time 0.
      v = pick ? _segValue(pick, t) : this._value;
    }
    if (v < this.minValue) v = this.minValue;
    if (v > this.maxValue) v = this.maxValue;
    return v;
  };

  // Fill `out` with `n` a-rate samples starting at time `t0`, then add every
  // signal connected into this param (`osc.connect(gain.gain)`).
  AudioParam.prototype._fill = function(out, t0, dt, n) {
    var i;
    if (this._events.length === 0 || this.automationRate === 'k-rate') {
      var v = (this._events.length === 0) ? this._value : this._valueAt(t0);
      for (i = 0; i < n; i++) out[i] = v;
    } else {
      for (i = 0; i < n; i++) out[i] = this._valueAt(t0 + i * dt);
    }
    for (var k = 0; k < this._inputs.length; k++) {
      var e = this._inputs[k];
      var sig = _pull(e.node, n);
      if (!sig || !sig.length) continue;
      var ch = sig[e.output] || sig[0];
      for (i = 0; i < n; i++) out[i] += ch[i];
    }
    return out;
  };

  Object.defineProperty(AudioParam.prototype, 'value', {
    // With a schedule in place, the observable value is the one the timeline
    // says holds at the context's current time — which during a render is the
    // start of the quantum being processed, i.e. the spec's [[current value]].
    get: function() {
      if (this._events.length === 0) return this._value;
      return this._valueAt(this._ctx ? this._ctx.currentTime : 0);
    },
    set: function(v) { this._value = +v; this._segs = null; },
    configurable: true, enumerable: true
  });

  AudioParam.prototype._insert = function(ev) {
    if (!isFinite(ev.time) || ev.time < 0) {
      throw new RangeError('AudioParam: time must be a finite non-negative number');
    }
    var evs = this._events, i = 0;
    while (i < evs.length && evs[i].time <= ev.time) i++;
    evs.splice(i, 0, ev);
    this._segs = null;
    return this;
  };
  AudioParam.prototype.setValueAtTime = function(value, startTime) {
    return this._insert({ type: 'set', value: +value, time: +startTime });
  };
  AudioParam.prototype.linearRampToValueAtTime = function(value, endTime) {
    return this._insert({ type: 'lin', value: +value, time: +endTime });
  };
  AudioParam.prototype.exponentialRampToValueAtTime = function(value, endTime) {
    if (+value === 0) throw new RangeError('exponentialRampToValueAtTime: value must not be 0');
    return this._insert({ type: 'exp', value: +value, time: +endTime });
  };
  AudioParam.prototype.setTargetAtTime = function(target, startTime, timeConstant) {
    if (+timeConstant < 0) throw new RangeError('setTargetAtTime: timeConstant must be non-negative');
    return this._insert({ type: 'target', value: +target, time: +startTime, tau: +timeConstant });
  };
  AudioParam.prototype.setValueCurveAtTime = function(values, startTime, duration) {
    if (!values || values.length < 2) {
      throw _wa_error('setValueCurveAtTime: curve needs at least two elements', 'InvalidStateError');
    }
    if (!(+duration > 0)) throw new RangeError('setValueCurveAtTime: duration must be positive');
    var curve = new Float32Array(values.length);
    for (var i = 0; i < values.length; i++) curve[i] = +values[i];
    return this._insert({ type: 'curve', curve: curve, time: +startTime, duration: +duration });
  };
  AudioParam.prototype.cancelScheduledValues = function(cancelTime) {
    var t = +cancelTime;
    this._events = this._events.filter(function(e) { return e.time < t; });
    this._segs = null;
    return this;
  };
  AudioParam.prototype.cancelAndHoldAtTime = function(cancelTime) {
    var t = +cancelTime;
    var held = this._valueAt(t);
    this._events = this._events.filter(function(e) { return e.time < t; });
    this._segs = null;
    return this._insert({ type: 'set', value: held, time: t });
  };
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
      throw _wa_error('channel index out of bounds', 'IndexSizeError');
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

  // ── render core ─────────────────────────────────────────────────────────────

  // Render state of the quantum currently being processed. A node caches its
  // output under `_qid`, so a fan-out graph pulls each producer exactly once.
  var _qid = 0;      // monotonic quantum id
  var _rt0 = 0;      // time (s) at the start of the quantum
  var _rdt = 0;      // 1 / sampleRate

  function _silence(nch, n) {
    var out = [];
    for (var i = 0; i < nch; i++) out.push(new Float32Array(n));
    return out;
  }

  // Add `src` into `dst` with the channel up/down-mixing rules of §4 (speakers).
  function _addInto(dst, src, n) {
    var dn = dst.length, sn = src.length, c, i;
    if (sn === dn) {
      for (c = 0; c < dn; c++) {
        var a = dst[c], b = src[c];
        for (i = 0; i < n; i++) a[i] += b[i];
      }
    } else if (sn === 1) {
      var mono = src[0];
      for (c = 0; c < dn; c++) {
        var d = dst[c];
        for (i = 0; i < n; i++) d[i] += mono[i];
      }
    } else if (dn === 1) {
      var acc = dst[0];
      for (c = 0; c < sn; c++) {
        var s = src[c];
        for (i = 0; i < n; i++) acc[i] += s[i] / sn;
      }
    } else {
      var m = Math.min(dn, sn);
      for (c = 0; c < m; c++) {
        var dd = dst[c], ss = src[c];
        for (i = 0; i < n; i++) dd[i] += ss[i];
      }
    }
  }

  function _mixSignals(list, n) {
    var nch = 1, i;
    for (i = 0; i < list.length; i++) if (list[i].length > nch) nch = list[i].length;
    var out = _silence(nch, n);
    for (i = 0; i < list.length; i++) _addInto(out, list[i], n);
    return out;
  }

  // Collect a node's incoming signals grouped by input index. A connection
  // made from output > 0 (a channel splitter) contributes that channel alone.
  function _gatherInputs(node, n) {
    var byInput = {};
    for (var k = 0; k < node._inputs.length; k++) {
      var e = node._inputs[k];
      var full = _pull(e.node, n);
      if (!full || !full.length) continue;
      var sig = (e.output > 0) ? [ full[e.output] || new Float32Array(n) ] : full;
      if (!byInput[e.input]) byInput[e.input] = [];
      byInput[e.input].push(sig);
    }
    return byInput;
  }

  function _pull(node, n) {
    if (!node) return null;
    if (node._qid === _qid) return node._qout;
    // Claim the slot with silence *before* recursing: a cycle in the graph
    // then reads a zero signal instead of recursing forever.
    node._qid  = _qid;
    node._qout = _silence(1, n);
    var byInput = _gatherInputs(node, n);
    var out;
    if (typeof node._process === 'function') {
      out = node._process(byInput, n);
    } else {
      out = _mixSignals(byInput[0] || [], n);
    }
    node._qout = out || _silence(1, n);
    return node._qout;
  }

  // Convenience for the common "one input, mixed" case.
  function _input0(byInput, n) { return _mixSignals(byInput[0] || [], n); }

  function _paramBuf(param, n) {
    var buf = new Float32Array(n);
    param._fill(buf, _rt0, _rdt, n);
    return buf;
  }

  // ── AudioNode (base) ────────────────────────────────────────────────────────

  function AudioNode(context, opts) {
    opts = opts || {};
    this.context               = context;
    this.channelCount          = opts.channelCount          || 2;
    this.channelCountMode      = opts.channelCountMode      || 'max';
    this.channelInterpretation = opts.channelInterpretation || 'speakers';
    this.numberOfInputs        = 0;
    this.numberOfOutputs       = 0;
    this._connections          = [];   // outgoing destinations (nodes or params)
    this._inputs               = [];   // {node, output, input}
    this._listeners            = {};
    this._qid                  = -1;
    this._qout                 = null;
  }
  _wa_events(AudioNode.prototype);
  AudioNode.prototype.connect = function(destination, outputIndex, inputIndex) {
    if (!destination) throw new TypeError('connect: destination is required');
    var out  = outputIndex ? (outputIndex | 0) : 0;
    var into = inputIndex  ? (inputIndex  | 0) : 0;
    this._connections.push(destination);
    if (destination instanceof AudioParam) {
      destination._inputs.push({ node: this, output: out });
      return undefined;
    }
    if (destination._inputs) destination._inputs.push({ node: this, output: out, input: into });
    return destination;
  };
  AudioNode.prototype.disconnect = function(destinationOrOutput, output, input) {
    var self = this, targets;
    if (destinationOrOutput === undefined || typeof destinationOrOutput === 'number') {
      targets = this._connections.slice();
      this._connections = [];
    } else {
      targets = [destinationOrOutput];
      this._connections = this._connections.filter(function(c) { return c !== destinationOrOutput; });
    }
    for (var i = 0; i < targets.length; i++) {
      var t = targets[i];
      if (t && t._inputs) {
        t._inputs = t._inputs.filter(function(e) { return e.node !== self; });
      }
    }
  };
  globalThis.AudioNode = AudioNode;

  // Shared scheduling for a source node: `start`/`stop` record times, and the
  // `ended` event fires exactly once.
  function _wa_source(proto) {
    proto._wa_initSource = function() {
      this._startTime  = null;
      this._stopTime   = null;
      this._endedFired = false;
      this.onended     = null;
    };
    proto._wa_fireEnded = function() {
      if (this._endedFired) return;
      this._endedFired = true;
      var self = this;
      _wa_task(function() { self._wa_fire('ended'); });
    };
    // A realtime context has no render loop to notice the stop time, so the
    // event is scheduled off the wall clock instead.
    proto._wa_scheduleRealtimeEnd = function(when) {
      if (!this.context || this.context._offline) return;
      if (typeof setTimeout !== 'function') return;
      var self = this;
      var delay = Math.max(0, (when - this.context.currentTime) * 1000);
      setTimeout(function() { self._wa_fireEnded(); }, delay);
    };
  }

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
    this.gain = _wa_param(context, (opts && opts.gain != null) ? opts.gain : 1.0);
  }
  GainNode.prototype = Object.create(AudioNode.prototype);
  GainNode.prototype.constructor = GainNode;
  GainNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var g = _paramBuf(this.gain, n);
    var out = _silence(input.length, n);
    for (var c = 0; c < input.length; c++) {
      var src = input[c], dst = out[c];
      for (var i = 0; i < n; i++) dst[i] = src[i] * g[i];
    }
    return out;
  };
  globalThis.GainNode = GainNode;

  // ── OscillatorNode ──────────────────────────────────────────────────────────

  var OSC_TYPES = ['sine', 'square', 'sawtooth', 'triangle', 'custom'];

  function _oscSample(type, phase) {
    var p = phase - Math.floor(phase);
    switch (type) {
      case 'square':   return p < 0.5 ? 1 : -1;
      case 'sawtooth': return 2 * p - 1;
      case 'triangle': return p < 0.25 ? 4 * p : (p < 0.75 ? 2 - 4 * p : 4 * p - 4);
      default:         return Math.sin(2 * Math.PI * p);
    }
  }

  function OscillatorNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this._type     = (opts && opts.type) ? opts.type : 'sine';
    this.frequency = _wa_param(context, (opts && opts.frequency != null) ? opts.frequency : 440);
    this.detune    = _wa_param(context, (opts && opts.detune    != null) ? opts.detune    : 0);
    this._phase   = 0;
    this._started = false;
    this._stopped = false;
    this._wa_initSource();
  }
  OscillatorNode.prototype = Object.create(AudioNode.prototype);
  OscillatorNode.prototype.constructor = OscillatorNode;
  _wa_source(OscillatorNode.prototype);
  Object.defineProperty(OscillatorNode.prototype, 'type', {
    get: function() { return this._type; },
    set: function(v) {
      if (OSC_TYPES.indexOf(v) < 0) throw _wa_error('Invalid oscillator type', 'InvalidStateError');
      this._type = v;
    },
    configurable: true, enumerable: true
  });
  OscillatorNode.prototype.start = function(when) {
    if (this._started) throw _wa_error('start() called twice', 'InvalidStateError');
    this._started   = true;
    this._startTime = (when != null) ? +when : this.context.currentTime;
  };
  OscillatorNode.prototype.stop = function(when) {
    if (!this._started) throw _wa_error('stop() called before start()', 'InvalidStateError');
    this._stopped  = true;
    this._stopTime = (when != null) ? +when : this.context.currentTime;
    this._wa_scheduleRealtimeEnd(this._stopTime);
  };
  OscillatorNode.prototype.setPeriodicWave = function(wave) { this._type = 'custom'; };
  OscillatorNode.prototype._process = function(byInput, n) {
    var out = _silence(1, n), o = out[0];
    if (this._startTime == null) return out;
    var f  = _paramBuf(this.frequency, n);
    var d  = _paramBuf(this.detune, n);
    var sr = this.context.sampleRate;
    var start = this._startTime;
    var stop  = (this._stopTime == null) ? Infinity : this._stopTime;
    for (var i = 0; i < n; i++) {
      var t = _rt0 + i * _rdt;
      if (t < start || t >= stop) continue;
      o[i] = _oscSample(this._type, this._phase);
      this._phase += (f[i] * Math.pow(2, d[i] / 1200)) / sr;
      if (this._phase >= 1 || this._phase <= -1) this._phase -= Math.floor(this._phase);
    }
    if (stop !== Infinity && _rt0 + n * _rdt > stop) this._wa_fireEnded();
    return out;
  };
  globalThis.OscillatorNode = OscillatorNode;

  // ── ConstantSourceNode ──────────────────────────────────────────────────────

  function ConstantSourceNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this.offset = _wa_param(context, (opts && opts.offset != null) ? opts.offset : 1);
    this._started = false;
    this._wa_initSource();
  }
  ConstantSourceNode.prototype = Object.create(AudioNode.prototype);
  ConstantSourceNode.prototype.constructor = ConstantSourceNode;
  _wa_source(ConstantSourceNode.prototype);
  ConstantSourceNode.prototype.start = function(when) {
    if (this._started) throw _wa_error('start() called twice', 'InvalidStateError');
    this._started   = true;
    this._startTime = (when != null) ? +when : this.context.currentTime;
  };
  ConstantSourceNode.prototype.stop = function(when) {
    if (!this._started) throw _wa_error('stop() called before start()', 'InvalidStateError');
    this._stopTime = (when != null) ? +when : this.context.currentTime;
    this._wa_scheduleRealtimeEnd(this._stopTime);
  };
  ConstantSourceNode.prototype._process = function(byInput, n) {
    var out = _silence(1, n), o = out[0];
    if (this._startTime == null) return out;
    var v = _paramBuf(this.offset, n);
    var stop = (this._stopTime == null) ? Infinity : this._stopTime;
    for (var i = 0; i < n; i++) {
      var t = _rt0 + i * _rdt;
      if (t < this._startTime || t >= stop) continue;
      o[i] = v[i];
    }
    if (stop !== Infinity && _rt0 + n * _rdt > stop) this._wa_fireEnded();
    return out;
  };
  globalThis.ConstantSourceNode = ConstantSourceNode;

  // ── AudioBufferSourceNode ───────────────────────────────────────────────────

  function AudioBufferSourceNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 0;
    this.numberOfOutputs = 1;
    this.buffer          = (opts && opts.buffer) ? opts.buffer : null;
    this.loop            = (opts && opts.loop)   ? !!opts.loop : false;
    this.loopStart       = (opts && opts.loopStart != null) ? opts.loopStart : 0;
    this.loopEnd         = (opts && opts.loopEnd   != null) ? opts.loopEnd   : 0;
    this.playbackRate    = _wa_param(context, (opts && opts.playbackRate != null) ? opts.playbackRate : 1);
    this.detune          = _wa_param(context, 0);
    this._started  = false;
    this._offset   = 0;
    this._duration = null;
    this._pos      = null;
    this._wa_initSource();
  }
  AudioBufferSourceNode.prototype = Object.create(AudioNode.prototype);
  AudioBufferSourceNode.prototype.constructor = AudioBufferSourceNode;
  _wa_source(AudioBufferSourceNode.prototype);
  AudioBufferSourceNode.prototype.start = function(when, offset, duration) {
    if (this._started) throw _wa_error('start() called twice', 'InvalidStateError');
    this._started   = true;
    this._startTime = (when != null) ? +when : this.context.currentTime;
    this._offset    = (offset   != null) ? +offset   : 0;
    this._duration  = (duration != null) ? +duration : null;
    if (this.context && !this.context._offline) {
      var dur = this._duration;
      if (dur == null && this.buffer) dur = Math.max(0, this.buffer.duration - this._offset);
      if (dur != null) this._wa_scheduleRealtimeEnd(this._startTime + dur);
    }
  };
  AudioBufferSourceNode.prototype.stop = function(when) {
    if (!this._started) throw _wa_error('stop() called before start()', 'InvalidStateError');
    this._stopTime = (when != null) ? +when : this.context.currentTime;
    this._wa_scheduleRealtimeEnd(this._stopTime);
  };
  AudioBufferSourceNode.prototype._process = function(byInput, n) {
    var buf = this.buffer;
    var nch = buf ? buf.numberOfChannels : 1;
    var out = _silence(nch, n);
    if (!buf || this._startTime == null || this._endedFired) return out;
    var rate = _paramBuf(this.playbackRate, n);
    var det  = _paramBuf(this.detune, n);
    var sr   = this.context.sampleRate;
    var bsr  = buf.sampleRate;
    var chans = [];
    for (var c = 0; c < nch; c++) chans.push(buf.getChannelData(c));
    var loopStartF = Math.max(0, this.loopStart * bsr);
    var loopEndF   = (this.loopEnd > 0 && this.loopEnd * bsr <= buf.length)
      ? this.loopEnd * bsr : buf.length;
    var stop = (this._stopTime == null) ? Infinity : this._stopTime;
    for (var i = 0; i < n; i++) {
      var t = _rt0 + i * _rdt;
      if (t < this._startTime) continue;
      if (t >= stop) { this._wa_fireEnded(); break; }
      if (this._pos == null) this._pos = this._offset * bsr;
      var p = this._pos;
      if (this.loop && loopEndF > loopStartF) {
        while (p >= loopEndF) p -= (loopEndF - loopStartF);
      } else if (p >= buf.length ||
                 (this._duration != null && (p / bsr - this._offset) >= this._duration)) {
        this._wa_fireEnded();
        break;
      }
      var k = Math.floor(p), frac = p - k;
      for (var c2 = 0; c2 < nch; c2++) {
        var ch = chans[c2];
        var a = ch[k] || 0;
        var b = (k + 1 < ch.length) ? ch[k + 1] : (this.loop ? (ch[Math.floor(loopStartF)] || 0) : 0);
        out[c2][i] = a + (b - a) * frac;
      }
      this._pos = p + (rate[i] * Math.pow(2, det[i] / 1200) * bsr / sr);
    }
    return out;
  };
  globalThis.AudioBufferSourceNode = AudioBufferSourceNode;

  // ── BiquadFilterNode ────────────────────────────────────────────────────────

  var BIQUAD_TYPES = ['lowpass','highpass','bandpass','lowshelf','highshelf','peaking','notch','allpass'];

  // RBJ cookbook coefficients as specialized by Web Audio §1.10 ("Filter
  // characteristics"): Q is in decibels for lowpass/highpass and a plain
  // quality factor everywhere else; the shelving filters use S = 1.
  function _biquadCoeffs(type, sampleRate, freqHz, q, gainDb) {
    var nyquist = sampleRate / 2;
    var f = Math.min(Math.max(freqHz / nyquist, 0), 1);   // normalized 0..1
    var w0 = Math.PI * f;
    var cosw = Math.cos(w0), sinw = Math.sin(w0);
    var A = Math.pow(10, gainDb / 40);
    var alpha, b0, b1, b2, a0, a1, a2, sq;
    switch (type) {
      case 'highpass':
        alpha = sinw / (2 * Math.pow(10, q / 20));
        b0 = (1 + cosw) / 2; b1 = -(1 + cosw); b2 = (1 + cosw) / 2;
        a0 = 1 + alpha; a1 = -2 * cosw; a2 = 1 - alpha;
        break;
      case 'bandpass':
        alpha = sinw / (2 * (q || 1e-6));
        b0 = alpha; b1 = 0; b2 = -alpha;
        a0 = 1 + alpha; a1 = -2 * cosw; a2 = 1 - alpha;
        break;
      case 'notch':
        alpha = sinw / (2 * (q || 1e-6));
        b0 = 1; b1 = -2 * cosw; b2 = 1;
        a0 = 1 + alpha; a1 = -2 * cosw; a2 = 1 - alpha;
        break;
      case 'allpass':
        alpha = sinw / (2 * (q || 1e-6));
        b0 = 1 - alpha; b1 = -2 * cosw; b2 = 1 + alpha;
        a0 = 1 + alpha; a1 = -2 * cosw; a2 = 1 - alpha;
        break;
      case 'peaking':
        alpha = sinw / (2 * (q || 1e-6));
        b0 = 1 + alpha * A; b1 = -2 * cosw; b2 = 1 - alpha * A;
        a0 = 1 + alpha / A; a1 = -2 * cosw; a2 = 1 - alpha / A;
        break;
      case 'lowshelf':
        sq = 2 * Math.sqrt(A) * (sinw / 2) * Math.sqrt(2);
        b0 = A * ((A + 1) - (A - 1) * cosw + sq);
        b1 = 2 * A * ((A - 1) - (A + 1) * cosw);
        b2 = A * ((A + 1) - (A - 1) * cosw - sq);
        a0 = (A + 1) + (A - 1) * cosw + sq;
        a1 = -2 * ((A - 1) + (A + 1) * cosw);
        a2 = (A + 1) + (A - 1) * cosw - sq;
        break;
      case 'highshelf':
        sq = 2 * Math.sqrt(A) * (sinw / 2) * Math.sqrt(2);
        b0 = A * ((A + 1) + (A - 1) * cosw + sq);
        b1 = -2 * A * ((A - 1) + (A + 1) * cosw);
        b2 = A * ((A + 1) + (A - 1) * cosw - sq);
        a0 = (A + 1) - (A - 1) * cosw + sq;
        a1 = 2 * ((A - 1) - (A + 1) * cosw);
        a2 = (A + 1) - (A - 1) * cosw - sq;
        break;
      case 'lowpass':
      default:
        alpha = sinw / (2 * Math.pow(10, q / 20));
        b0 = (1 - cosw) / 2; b1 = 1 - cosw; b2 = (1 - cosw) / 2;
        a0 = 1 + alpha; a1 = -2 * cosw; a2 = 1 - alpha;
        break;
    }
    return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
  }

  function BiquadFilterNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this._type     = (opts && opts.type) ? opts.type : 'lowpass';
    this.frequency = _wa_param(context, (opts && opts.frequency != null) ? opts.frequency : 350);
    this.detune    = _wa_param(context, 0);
    this.Q         = _wa_param(context, (opts && opts.Q    != null) ? opts.Q    : 1);
    this.gain      = _wa_param(context, (opts && opts.gain != null) ? opts.gain : 0);
    this._state    = [];   // per channel [x1, x2, y1, y2]
  }
  BiquadFilterNode.prototype = Object.create(AudioNode.prototype);
  BiquadFilterNode.prototype.constructor = BiquadFilterNode;
  Object.defineProperty(BiquadFilterNode.prototype, 'type', {
    get: function() { return this._type; },
    set: function(v) {
      if (BIQUAD_TYPES.indexOf(v) < 0) throw _wa_error('Invalid filter type', 'InvalidStateError');
      this._type = v;
    },
    configurable: true, enumerable: true
  });
  BiquadFilterNode.prototype._coeffs = function(t) {
    var freq = this.frequency._valueAt(t) * Math.pow(2, this.detune._valueAt(t) / 1200);
    return _biquadCoeffs(this._type, this.context ? this.context.sampleRate : 44100,
                         freq, this.Q._valueAt(t), this.gain._valueAt(t));
  };
  BiquadFilterNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var out = _silence(input.length, n);
    var c = this._coeffs(_rt0);   // coefficients are k-rate, one set per quantum
    for (var ch = 0; ch < input.length; ch++) {
      if (!this._state[ch]) this._state[ch] = [0, 0, 0, 0];
      var st = this._state[ch], src = input[ch], dst = out[ch];
      var x1 = st[0], x2 = st[1], y1 = st[2], y2 = st[3];
      for (var i = 0; i < n; i++) {
        var x0 = src[i];
        var y0 = c.b0 * x0 + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
        x2 = x1; x1 = x0; y2 = y1; y1 = y0;
        dst[i] = y0;
      }
      st[0] = x1; st[1] = x2; st[2] = y1; st[3] = y2;
    }
    return out;
  };
  BiquadFilterNode.prototype.getFrequencyResponse = function(frequencyHz, magResponse, phaseResponse) {
    var sr = this.context ? this.context.sampleRate : 44100;
    var c = this._coeffs(this.context ? this.context.currentTime : 0);
    for (var i = 0; i < frequencyHz.length; i++) {
      var w = 2 * Math.PI * frequencyHz[i] / sr;
      var cw = Math.cos(w), sw = Math.sin(w);
      var c2w = Math.cos(2 * w), s2w = Math.sin(2 * w);
      var nr = c.b0 + c.b1 * cw + c.b2 * c2w;
      var ni = -(c.b1 * sw + c.b2 * s2w);
      var dr = 1 + c.a1 * cw + c.a2 * c2w;
      var di = -(c.a1 * sw + c.a2 * s2w);
      var den = dr * dr + di * di;
      var hr = (den === 0) ? 0 : (nr * dr + ni * di) / den;
      var hi = (den === 0) ? 0 : (ni * dr - nr * di) / den;
      if (magResponse)   magResponse[i]   = Math.sqrt(hr * hr + hi * hi);
      if (phaseResponse) phaseResponse[i] = Math.atan2(hi, hr);
    }
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
    this._ring    = null;   // most recent `fftSize` mono samples
    this._ringPos = 0;
  }
  AnalyserNode.prototype = Object.create(AudioNode.prototype);
  AnalyserNode.prototype.constructor = AnalyserNode;
  Object.defineProperty(AnalyserNode.prototype, 'frequencyBinCount', {
    get: function() { return this.fftSize >> 1; },
    configurable: true, enumerable: true
  });
  AnalyserNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    if (!this._ring || this._ring.length !== this.fftSize) {
      this._ring = new Float32Array(this.fftSize);
      this._ringPos = 0;
    }
    var ring = this._ring, nch = input.length;
    for (var i = 0; i < n; i++) {
      var s = 0;
      for (var c = 0; c < nch; c++) s += input[c][i];
      ring[this._ringPos] = s / (nch || 1);
      this._ringPos = (this._ringPos + 1) % ring.length;
    }
    return input;   // the analyser is a pass-through node
  };
  AnalyserNode.prototype.getFloatFrequencyData = function(array) {
    // No FFT yet: report the floor, which is what an all-silent input means.
    for (var i = 0; i < array.length; i++) array[i] = this.minDecibels;
  };
  AnalyserNode.prototype.getByteFrequencyData = function(array) {
    for (var i = 0; i < array.length; i++) array[i] = 0;
  };
  AnalyserNode.prototype.getFloatTimeDomainData = function(array) {
    var ring = this._ring, n = array.length;
    for (var i = 0; i < n; i++) {
      array[i] = ring ? ring[((this._ringPos - n + i) % ring.length + ring.length) % ring.length] : 0.0;
    }
  };
  AnalyserNode.prototype.getByteTimeDomainData = function(array) {
    var tmp = new Float32Array(array.length);
    this.getFloatTimeDomainData(tmp);
    for (var i = 0; i < array.length; i++) {
      var v = Math.round(128 * (1 + tmp[i]));
      array[i] = v < 0 ? 0 : (v > 255 ? 255 : v);
    }
  };
  globalThis.AnalyserNode = AnalyserNode;

  // ── DelayNode ───────────────────────────────────────────────────────────────

  function DelayNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this._maxDelayTime = (opts && opts.maxDelayTime != null) ? +opts.maxDelayTime : 1;
    this.delayTime = _wa_param(context, (opts && opts.delayTime != null) ? opts.delayTime : 0,
                               { minValue: 0, maxValue: this._maxDelayTime });
    this._lines = [];
    this._writePos = 0;
  }
  DelayNode.prototype = Object.create(AudioNode.prototype);
  DelayNode.prototype.constructor = DelayNode;
  DelayNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var sr = this.context.sampleRate;
    var lineLen = Math.max(RENDER_QUANTUM, Math.ceil(this._maxDelayTime * sr) + RENDER_QUANTUM);
    var d = _paramBuf(this.delayTime, n);
    var out = _silence(input.length, n);
    var endPos = this._writePos;
    for (var c = 0; c < input.length; c++) {
      if (!this._lines[c] || this._lines[c].length !== lineLen) {
        this._lines[c] = new Float32Array(lineLen);
      }
      var line = this._lines[c], src = input[c], dst = out[c];
      var w = this._writePos;
      for (var i = 0; i < n; i++) {
        line[w] = src[i];
        var back = Math.min(Math.max(d[i], 0), this._maxDelayTime) * sr;
        var rp = w - back;
        while (rp < 0) rp += lineLen;
        var k = Math.floor(rp), frac = rp - k;
        var a = line[k % lineLen], b = line[(k + 1) % lineLen];
        dst[i] = a + (b - a) * frac;
        w = (w + 1) % lineLen;
      }
      endPos = w;
    }
    this._writePos = endPos;
    return out;
  };
  globalThis.DelayNode = DelayNode;

  // ── DynamicsCompressorNode ──────────────────────────────────────────────────

  function DynamicsCompressorNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.threshold  = _wa_param(context, (opts && opts.threshold  != null) ? opts.threshold  : -24);
    this.knee       = _wa_param(context, (opts && opts.knee       != null) ? opts.knee       : 30);
    this.ratio      = _wa_param(context, (opts && opts.ratio      != null) ? opts.ratio      : 12);
    this.attack     = _wa_param(context, (opts && opts.attack     != null) ? opts.attack     : 0.003);
    this.release    = _wa_param(context, (opts && opts.release    != null) ? opts.release    : 0.25);
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
    this.pan = _wa_param(context, (opts && opts.pan != null) ? opts.pan : 0);
  }
  StereoPannerNode.prototype = Object.create(AudioNode.prototype);
  StereoPannerNode.prototype.constructor = StereoPannerNode;
  StereoPannerNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var p = _paramBuf(this.pan, n);
    var out = _silence(2, n);
    var mono = input.length === 1;
    for (var i = 0; i < n; i++) {
      var pan = Math.min(Math.max(p[i], -1), 1);
      if (mono) {
        // §"StereoPannerNode", mono input: x = (pan + 1) / 2 * PI / 2
        var x = (pan + 1) / 2 * Math.PI / 2;
        out[0][i] = input[0][i] * Math.cos(x);
        out[1][i] = input[0][i] * Math.sin(x);
      } else {
        var l = input[0][i], r = (input[1] ? input[1][i] : 0);
        var x2 = (pan <= 0 ? pan + 1 : pan) * Math.PI / 2;
        if (pan <= 0) {
          out[0][i] = l + r * Math.cos(x2);
          out[1][i] = r * Math.sin(x2);
        } else {
          out[0][i] = l * Math.cos(x2);
          out[1][i] = r + l * Math.sin(x2);
        }
      }
    }
    return out;
  };
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
    this.positionX        = _wa_param(context, opts.positionX != null ? opts.positionX : 0);
    this.positionY        = _wa_param(context, opts.positionY != null ? opts.positionY : 0);
    this.positionZ        = _wa_param(context, opts.positionZ != null ? opts.positionZ : 0);
    this.orientationX     = _wa_param(context, opts.orientationX != null ? opts.orientationX : 1);
    this.orientationY     = _wa_param(context, 0);
    this.orientationZ     = _wa_param(context, 0);
  }
  PannerNode.prototype = Object.create(AudioNode.prototype);
  PannerNode.prototype.constructor = PannerNode;
  PannerNode.prototype.setPosition    = function(x, y, z) {
    this.positionX.value = x; this.positionY.value = y; this.positionZ.value = z;
  };
  PannerNode.prototype.setOrientation = function(x, y, z) {
    this.orientationX.value = x; this.orientationY.value = y; this.orientationZ.value = z;
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
    this.positionX.value = x; this.positionY.value = y; this.positionZ.value = z;
  };
  AudioListener.prototype.setOrientation = function(x, y, z, xUp, yUp, zUp) {
    this.forwardX.value = x; this.forwardY.value = y; this.forwardZ.value = z;
    this.upX.value = xUp; this.upY.value = yUp; this.upZ.value = zUp;
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
  ChannelMergerNode.prototype._process = function(byInput, n) {
    var out = _silence(this.numberOfInputs, n);
    for (var j = 0; j < this.numberOfInputs; j++) {
      var list = byInput[j];
      if (!list) continue;
      var mixed = _mixSignals(list, n);
      var src = mixed[0], dst = out[j];
      for (var i = 0; i < n; i++) dst[i] = src[i];
    }
    return out;
  };
  globalThis.ChannelMergerNode = ChannelMergerNode;

  // ── ChannelSplitterNode ─────────────────────────────────────────────────────

  function ChannelSplitterNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = (opts && opts.numberOfOutputs) ? opts.numberOfOutputs : 6;
  }
  ChannelSplitterNode.prototype = Object.create(AudioNode.prototype);
  ChannelSplitterNode.prototype.constructor = ChannelSplitterNode;
  ChannelSplitterNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var out = _silence(this.numberOfOutputs, n);
    for (var c = 0; c < this.numberOfOutputs && c < input.length; c++) {
      var src = input[c], dst = out[c];
      for (var i = 0; i < n; i++) dst[i] = src[i];
    }
    return out;
  };
  globalThis.ChannelSplitterNode = ChannelSplitterNode;

  // ── WaveShaperNode ──────────────────────────────────────────────────────────

  function WaveShaperNode(context, opts) {
    AudioNode.call(this, context, opts);
    this.numberOfInputs  = 1;
    this.numberOfOutputs = 1;
    this.curve      = (opts && opts.curve)      ? opts.curve      : null;
    this.oversample = (opts && opts.oversample) ? opts.oversample : 'none';
  }
  WaveShaperNode.prototype = Object.create(AudioNode.prototype);
  WaveShaperNode.prototype.constructor = WaveShaperNode;
  WaveShaperNode.prototype._process = function(byInput, n) {
    var input = _input0(byInput, n);
    var curve = this.curve;
    if (!curve || curve.length < 2) return input;
    var out = _silence(input.length, n);
    var last = curve.length - 1;
    for (var c = 0; c < input.length; c++) {
      var src = input[c], dst = out[c];
      for (var i = 0; i < n; i++) {
        // §"WaveShaperNode": x in [-1, 1] maps onto the whole curve.
        var x = (src[i] + 1) * 0.5 * last;
        if (x <= 0)    { dst[i] = curve[0];    continue; }
        if (x >= last) { dst[i] = curve[last]; continue; }
        var k = Math.floor(x);
        dst[i] = curve[k] + (curve[k + 1] - curve[k]) * (x - k);
      }
    }
    return out;
  };
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
    this._offline      = false;
    this._baseTime     = 0;
    this._runSince     = (typeof Date !== 'undefined') ? Date.now() : 0;
    this.destination   = new AudioDestinationNode(this);
    this.listener      = new AudioListener();
    this._listeners    = {};
    this.onstatechange = null;

    // AudioWorklet stub
    this.audioWorklet = {
      addModule: function(url) { return Promise.resolve(); }
    };
  }
  _wa_events(BaseAudioContext.prototype);
  Object.defineProperty(BaseAudioContext.prototype, 'currentTime', {
    // Offline: the frame the render loop has reached. Realtime: the wall clock
    // since the context last started running, quantized to a render quantum —
    // there is no audio device to take a clock from, but `currentTime` must
    // still advance monotonically for scheduling to mean anything.
    get: function() {
      if (this._offline) return this._currentTime;
      var q = RENDER_QUANTUM / this.sampleRate;
      var t = this._baseTime;
      if (this._state === 'running' && typeof Date !== 'undefined') {
        t += (Date.now() - this._runSince) / 1000;
      }
      return Math.floor(t / q) * q;
    },
    configurable: true, enumerable: true
  });
  Object.defineProperty(BaseAudioContext.prototype, 'state', {
    get: function() { return this._state; },
    configurable: true, enumerable: true
  });
  BaseAudioContext.prototype._setState = function(s) {
    if (this._state === s) return;
    if (!this._offline && typeof Date !== 'undefined') {
      if (this._state === 'running') this._baseTime += (Date.now() - this._runSince) / 1000;
      if (s === 'running') this._runSince = Date.now();
    }
    this._state = s;
    // Deliberately synchronous, unlike every other dispatch in this file.
    // The spec queues `statechange`, but this engine only pumps timers when
    // it redraws, so on a static page a task waits up to a second — long
    // enough for `suspend()`/`resume()`/`close()` in sequence to have moved
    // the state on before the handler for the *previous* transition runs, and
    // the handler reads `context.state`, not the event. Measured 2026-08-25
    // with `verify_preload_script_audio_gaps.py --variant audio-context-state`:
    // queued, the `running` transition reports `closed`. The state machine
    // already worked before BUG-828 and is not part of it.
    this._wa_fire('statechange');
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
  BaseAudioContext.prototype.createConstantSource = function() { return new ConstantSourceNode(this); };
  BaseAudioContext.prototype.createBiquadFilter = function() { return new BiquadFilterNode(this); };
  BaseAudioContext.prototype.createAnalyser = function() { return new AnalyserNode(this); };
  BaseAudioContext.prototype.createDelay = function(maxDelay) {
    return new DelayNode(this, { maxDelayTime: (maxDelay != null) ? maxDelay : 1, delayTime: 0 });
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
    // No decoder: return a silent 1-second mono buffer.
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
    return { contextTime: this.currentTime, performanceTime: 0 };
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
    this._offline         = true;
    this._state           = 'suspended';
    this.oncomplete       = null;
    this._renderStarted   = false;
    this._renderedFrames  = 0;
    this._renderBuffer    = null;
    this._renderResolve   = null;
    this._suspends        = [];
  }
  OfflineAudioContext.prototype = Object.create(BaseAudioContext.prototype);
  OfflineAudioContext.prototype.constructor = OfflineAudioContext;

  OfflineAudioContext.prototype.startRendering = function() {
    var self = this;
    if (this._renderStarted) {
      return Promise.reject(_wa_error('startRendering() called twice', 'InvalidStateError'));
    }
    this._renderStarted = true;
    this._renderBuffer = new AudioBuffer({
      numberOfChannels: this.numberOfChannels,
      length:           this.length,
      sampleRate:       this.sampleRate
    });
    return new Promise(function(resolve) {
      self._renderResolve = resolve;
      self._setState('running');
      _wa_task(function() { self._renderStep(); });
    });
  };

  // One slice of the render loop: process render quanta until the buffer is
  // full or the next scheduled `suspend()` point is reached.
  OfflineAudioContext.prototype._renderStep = function() {
    var self = this;
    var sr  = this.sampleRate;
    var buf = this._renderBuffer;
    var nch = this.numberOfChannels;
    _rdt = 1 / sr;
    while (this._renderedFrames < this.length) {
      var next = this._suspends.length ? this._suspends[0] : null;
      if (next && next.frame === this._renderedFrames) {
        this._suspends.shift();
        this._currentTime = this._renderedFrames / sr;
        this._setState('suspended');
        _wa_task(function() { next.resolve(); });
        return;
      }
      var n = Math.min(RENDER_QUANTUM, this.length - this._renderedFrames);
      _qid++;
      _rt0 = this._renderedFrames / sr;
      this._currentTime = _rt0;
      // Nodes always see a full quantum; only `n` frames are kept.
      var byInput = _gatherInputs(this.destination, RENDER_QUANTUM);
      var mixed = _mixSignals(byInput[0] || [], RENDER_QUANTUM);
      var acc = _silence(nch, RENDER_QUANTUM);
      _addInto(acc, mixed, RENDER_QUANTUM);
      for (var c = 0; c < nch; c++) {
        var dst = buf.getChannelData(c), src = acc[c];
        for (var i = 0; i < n; i++) dst[this._renderedFrames + i] = src[i];
      }
      this._renderedFrames += n;
    }
    this._currentTime = this.length / sr;
    this._setState('closed');
    var rendered = buf;
    _wa_task(function() {
      var evt = { type: 'complete', renderedBuffer: rendered };
      self._wa_fire('complete', evt);
      if (self._renderResolve) self._renderResolve(rendered);
    });
  };

  OfflineAudioContext.prototype.suspend = function(suspendTime) {
    var self = this;
    return new Promise(function(resolve, reject) {
      var t = +suspendTime;
      if (!isFinite(t) || t < 0) {
        reject(_wa_error('suspend: time must be a finite non-negative number', 'InvalidStateError'));
        return;
      }
      // §"OfflineAudioContext.suspend": the time is quantized and rounded up
      // to a render-quantum boundary.
      var frame = Math.ceil(t * self.sampleRate / RENDER_QUANTUM) * RENDER_QUANTUM;
      if (frame < self._renderedFrames || frame >= self.length) {
        reject(_wa_error('suspend: time is outside the rendered range', 'InvalidStateError'));
        return;
      }
      for (var i = 0; i < self._suspends.length; i++) {
        if (self._suspends[i].frame === frame) {
          reject(_wa_error('suspend: already scheduled at this render quantum', 'InvalidStateError'));
          return;
        }
      }
      self._suspends.push({ frame: frame, resolve: resolve });
      self._suspends.sort(function(a, b) { return a.frame - b.frame; });
    });
  };

  OfflineAudioContext.prototype.resume = function() {
    var self = this;
    return new Promise(function(resolve) {
      if (self._renderStarted && self._state === 'suspended') {
        self._setState('running');
        _wa_task(function() { self._renderStep(); });
      }
      resolve();
    });
  };
  globalThis.OfflineAudioContext = OfflineAudioContext;

})();
"#;

/// V8 test coverage for the Web Audio API shim (the rquickjs twin was
/// removed in S12b-B19; this module ports its 12 tests to V8 verbatim).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn rt_with_web_audio() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
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
        super::install_web_audio_api_v8(&rt).unwrap();
        rt
    }

    #[test]
    fn audio_context_classes_exist() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_initial_state() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_suspend_resume_close() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_node_classes_exist() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                typeof GainNode === 'function'
                  && typeof OscillatorNode === 'function'
                  && typeof AudioBufferSourceNode === 'function'
                  && typeof ConstantSourceNode === 'function'
                  && typeof BiquadFilterNode === 'function'
                  && typeof AnalyserNode === 'function'
                  && typeof DelayNode === 'function'
                  && typeof DynamicsCompressorNode === 'function'
                  && typeof StereoPannerNode === 'function'
                  && typeof PannerNode === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_context_factory_methods() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var gain = ac.createGain();
                var osc  = ac.createOscillator();
                var buf  = ac.createBuffer(1, 100, 44100);
                var src  = ac.createBufferSource();
                var bq   = ac.createBiquadFilter();
                var an   = ac.createAnalyser();
                var cs   = ac.createConstantSource();
                gain instanceof GainNode
                  && osc  instanceof OscillatorNode
                  && buf  instanceof AudioBuffer
                  && src  instanceof AudioBufferSourceNode
                  && bq   instanceof BiquadFilterNode
                  && an   instanceof AnalyserNode
                  && cs   instanceof ConstantSourceNode
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn oscillator_node_type_and_freq() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_node_connect_disconnect() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac   = new AudioContext();
                var gain = ac.createGain();
                var osc  = ac.createOscillator();
                var result = osc.connect(gain);
                var linked = gain._inputs.length === 1 && gain._inputs[0].node === osc;
                osc.disconnect();
                result === gain && osc._connections.length === 0
                  && linked && gain._inputs.length === 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_buffer_channel_data() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn audio_param_set_value_at_time() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn offline_audio_context_start_rendering() {
        let rt = rt_with_web_audio();
        let ok = rt
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
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn decode_audio_data_returns_promise() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var buf = new ArrayBuffer(16);
                ac.decodeAudioData(buf) instanceof Promise
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// BUG-591: `oncomplete` used to run inside a bare `catch (e) {}`, which
    /// is why every `webaudio/resources/audioparam-testing.js` comparison —
    /// all of which run from that handler — died without a word (BUG-828
    /// names this as the reason those files TIMEOUT instead of failing).
    ///
    /// The call is typeof-guarded, and this runtime is exactly why: it has no
    /// page DOM at all, so `_lumen_report_exception` does not exist unless a
    /// caller (here, the test) supplies one. An unguarded call would throw a
    /// `ReferenceError` from inside the `catch` and take the rest of the
    /// dispatch with it — worse than the swallow it replaces.
    #[test]
    fn bug591_offline_context_oncomplete_exception_is_reported() {
        let rt = rt_with_web_audio();
        let reported = rt
            .eval(
                r#"
                var seen = null;
                globalThis._lumen_report_exception = function(e) { seen = e.message; };
                var oc = new OfflineAudioContext(1, 128, 44100);
                oc.oncomplete = function() { throw new Error('oncomplete-boom'); };
                oc.startRendering();
                seen
                "#,
            )
            .unwrap();
        assert_eq!(reported, JsValue::String("oncomplete-boom".to_string()));
    }

    /// The other half of the guard: with no reporter installed the handler's
    /// exception must still not escape the shim (the DOM-less runtime is a
    /// real configuration — `--dump-*`, SVG rasterization, unit tests).
    #[test]
    fn bug591_offline_context_oncomplete_exception_without_reporter_is_contained() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var oc = new OfflineAudioContext(1, 128, 44100);
                oc.oncomplete = function() { throw new Error('no-reporter-boom'); };
                oc.startRendering();
                true
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn webkit_audio_context_alias() {
        let rt = rt_with_web_audio();
        let ok = rt.eval("typeof webkitAudioContext === 'function'").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn analyser_frequency_bin_count() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ac = new AudioContext();
                var an = ac.createAnalyser();
                an.fftSize === 2048 && an.fftSize / 2 === 1024
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    // ── BUG-828: the offline render itself ──────────────────────────────────
    //
    // This runtime has no `setTimeout`, so the shim's task queue falls back to
    // an inline call (documented at `_wa_task`) — which is what lets these
    // tests read the rendered buffer through `oncomplete` in the same `eval`.
    // On a page the same code is a task, as the spec requires.
    //
    // A `Promise` callback is a different matter: V8 drains its microtask
    // queue when the *script* ends, so a `.then` never runs mid-`eval`. Any
    // test that needs one splits into two evals — setup, then assertion.

    /// The defect this bug is named for: a graph playing for the whole render
    /// used to produce a buffer without a single non-zero sample.
    #[test]
    fn bug828_offline_render_is_not_silent() {
        let rt = rt_with_web_audio();
        let nonzero = rt
            .eval(
                r#"
                var ctx  = new OfflineAudioContext(1, 4410, 44100);
                var osc  = ctx.createOscillator();
                var gain = ctx.createGain();
                osc.connect(gain);
                gain.connect(ctx.destination);
                gain.gain.setValueAtTime(0.25, 0);
                gain.gain.linearRampToValueAtTime(1.0, 0.05);
                osc.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var data = rendered.getChannelData(0);
                var n = 0;
                for (var i = 0; i < data.length; i++) if (data[i] !== 0) n++;
                n
                "#,
            )
            .unwrap();
        // A sine starting at phase 0 has its first sample at exactly zero;
        // every other frame of the 4410 carries signal.
        assert_eq!(nonzero, JsValue::Number(4409.0));
    }

    /// `AudioParam` automation reaches the rendered samples: a constant source
    /// under a linear gain ramp traces the ramp itself.
    #[test]
    fn bug828_param_automation_shapes_the_render() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx  = new OfflineAudioContext(1, 44100, 44100);
                var src  = ctx.createConstantSource();
                var gain = ctx.createGain();
                src.connect(gain);
                gain.connect(ctx.destination);
                gain.gain.setValueAtTime(0.0, 0);
                gain.gain.linearRampToValueAtTime(1.0, 1.0);
                src.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var d = rendered.getChannelData(0);
                Math.abs(d[22050] - 0.5) < 0.001
                  && Math.abs(d[0]) < 0.001
                  && d[44099] > 0.99
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `setValueCurveAtTime` and `setTargetAtTime` land on the timeline too.
    #[test]
    fn bug828_param_curve_and_target_segments() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 128, 44100);
                var g = ctx.createGain();
                g.gain.setValueCurveAtTime([0, 1], 0, 1);
                var half = g.gain._valueAt(0.5);
                var g2 = ctx.createGain();
                g2.gain.setValueAtTime(1, 0);
                g2.gain.setTargetAtTime(0, 0, 1);
                var decayed = g2.gain._valueAt(1);
                Math.abs(half - 0.5) < 1e-6 && Math.abs(decayed - Math.exp(-1)) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `ended` had no dispatch site at all: an `AudioBufferSourceNode` whose
    /// buffer runs out mid-render must fire it, in both handler forms.
    #[test]
    fn bug828_buffer_source_fires_ended() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 44100, 44100);
                var buf = ctx.createBuffer(1, 5512, 44100);
                var src = ctx.createBufferSource();
                src.buffer = buf;
                src.connect(ctx.destination);
                var viaProp = 0, viaListener = 0;
                src.onended = function() { viaProp++; };
                src.addEventListener('ended', function() { viaListener++; });
                src.start();
                var len = 0;
                ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
                ctx.startRendering();
                viaProp === 1 && viaListener === 1 && len === 44100
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A stopped oscillator fires `ended` exactly once and stays silent after
    /// its stop time.
    #[test]
    fn bug828_oscillator_stop_fires_ended_once_and_goes_silent() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 44100, 44100);
                var osc = ctx.createOscillator();
                osc.connect(ctx.destination);
                var ended = 0;
                osc.onended = function() { ended++; };
                osc.start(0);
                osc.stop(0.5);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                var d = rendered.getChannelData(0);
                var tailNonzero = 0;
                for (var i = 30000; i < d.length; i++) if (d[i] !== 0) tailNonzero++;
                ended === 1 && tailNonzero === 0 && d[1000] !== 0
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `suspend(t)` used to be a bare `Promise.resolve()`. It must now stop the
    /// render at a quantum boundary with the context still open, and `resume()`
    /// must carry it to the end.
    #[test]
    fn bug828_offline_suspend_stops_the_render_and_resume_finishes_it() {
        let rt = rt_with_web_audio();
        rt.eval(
            r#"
            var ctx = new OfflineAudioContext(1, 44100, 44100);
            var osc = ctx.createOscillator();
            osc.connect(ctx.destination);
            osc.start();
            var atSuspend = null, stateAtSuspend = null, len = 0;
            ctx.suspend(0.5).then(function() {
                atSuspend = ctx.currentTime;
                stateAtSuspend = ctx.state;
                ctx.resume();
            });
            ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
            ctx.startRendering();
            "#,
        )
        .unwrap();
        // 0.5 s is 22050 frames, rounded up to the next 128-frame boundary:
        // 173 * 128 = 22144. The whole suspend/resume handshake runs on the
        // microtask queue V8 drains when the eval above returns.
        let ok = rt
            .eval(
                r#"
                stateAtSuspend === 'suspended'
                  && Math.abs(atSuspend - 22144 / 44100) < 1e-9
                  && len === 44100
                  && ctx.state === 'closed'
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A node connected into an `AudioParam` is summed onto the automation
    /// curve rather than ignored.
    #[test]
    fn bug828_node_connected_to_param_is_summed() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 256, 44100);
                var carrier = ctx.createConstantSource();
                var gain = ctx.createGain();
                gain.gain.setValueAtTime(0.0, 0);
                var mod = ctx.createConstantSource();
                mod.offset.value = 0.5;
                mod.connect(gain.gain);
                mod.start(0);
                carrier.connect(gain);
                gain.connect(ctx.destination);
                carrier.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                Math.abs(rendered.getChannelData(0)[100] - 0.5) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// A cycle in the graph must not recurse forever — the pull guard hands
    /// out silence for the second visit inside one quantum.
    #[test]
    fn bug828_feedback_cycle_terminates() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(1, 256, 44100);
                var a = ctx.createGain(), b = ctx.createGain();
                a.connect(b); b.connect(a);
                a.connect(ctx.destination);
                var src = ctx.createConstantSource();
                src.connect(a); src.start(0);
                var len = 0;
                ctx.oncomplete = function(e) { len = e.renderedBuffer.length; };
                ctx.startRendering();
                len === 256
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// The merger routes input `j` onto output channel `j`, so a stereo
    /// destination gets two independently written channels.
    #[test]
    fn bug828_channel_merger_routes_inputs_to_channels() {
        let rt = rt_with_web_audio();
        let ok = rt
            .eval(
                r#"
                var ctx = new OfflineAudioContext(2, 256, 44100);
                var m = ctx.createChannelMerger(2);
                var l = ctx.createConstantSource(); l.offset.value = 0.25;
                var r = ctx.createConstantSource(); r.offset.value = 0.75;
                l.connect(m, 0, 0);
                r.connect(m, 0, 1);
                m.connect(ctx.destination);
                l.start(0); r.start(0);
                var rendered = null;
                ctx.oncomplete = function(e) { rendered = e.renderedBuffer; };
                ctx.startRendering();
                Math.abs(rendered.getChannelData(0)[10] - 0.25) < 1e-6
                  && Math.abs(rendered.getChannelData(1)[10] - 0.75) < 1e-6
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    /// `startRendering()` a second time rejects instead of re-rendering.
    #[test]
    fn bug828_second_start_rendering_rejects() {
        let rt = rt_with_web_audio();
        rt.eval(
            r#"
            var ctx = new OfflineAudioContext(1, 128, 44100);
            var rejected = null;
            ctx.startRendering();
            ctx.startRendering().catch(function(e) { rejected = e.name; });
            "#,
        )
        .unwrap();
        let ok = rt.eval("rejected === 'InvalidStateError'").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }
}
