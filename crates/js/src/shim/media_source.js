// Media Source Extensions (W3C MSE): MediaSource, SourceBuffer, SourceBufferList, TimeRanges.
//
// The JS side owns objects, state checks and events; bytes, container parsing and
// `buffered` live in `lumen-media-mse` behind the `__lumen_mse_*` natives
// (`mse_bindings.rs`). Appends and removals are performed in a queued task, as the
// spec's «buffer append algorithm» is asynchronous (`updating` stays true until
// `updateend`). `<video>` playback from a MediaSource is wired in `video_element.js`
// (`startMseLoad`), which calls the `_lumen_mse_*` hooks defined at the bottom.
(function () {
  'use strict';
  if (typeof __lumen_mse_create !== 'function') return;

  var _base = (typeof EventTarget === 'function') ? EventTarget : function () {};
  var _state = new WeakMap(); // object → private state (spec «internal slots»)

  function domException(message, name) {
    if (typeof DOMException === 'function') return new DOMException(message, name);
    var e = new Error(message); e.name = name; return e;
  }
  function task(fn) {
    if (typeof setTimeout === 'function') setTimeout(fn, 0); else fn();
  }
  function fire(target, type) {
    try { target.dispatchEvent(new Event(type, { bubbles: false, cancelable: false })); }
    catch (e) { if (typeof _lumen_report_exception === 'function') _lumen_report_exception(e); }
  }

  // ── TimeRanges ───────────────────────────────────────────────────────────────
  var _ranges = new WeakMap();
  function TimeRanges() { throw new TypeError('Illegal constructor'); }
  function makeRanges(list) {
    var r = Object.create(TimeRanges.prototype);
    _ranges.set(r, list);
    return r;
  }
  function rangeAt(self, i, which) {
    var list = _ranges.get(self);
    if (!list) throw new TypeError('Illegal invocation');
    if (arguments.length < 3 || i === undefined) throw new TypeError('index is required');
    i = Number(i) >>> 0;
    if (i >= list.length) throw domException('The index provided (' + i + ') is greater than or equal to the maximum bound (' + list.length + ').', 'IndexSizeError');
    return list[i][which];
  }
  Object.defineProperties(TimeRanges.prototype, {
    length: { get: function () { var l = _ranges.get(this); if (!l) throw new TypeError('Illegal invocation'); return l.length; }, enumerable: true, configurable: true },
    start: { value: function (i) { return rangeAt(this, i, 0); }, writable: true, enumerable: true, configurable: true },
    end: { value: function (i) { return rangeAt(this, i, 1); }, writable: true, enumerable: true, configurable: true },
  });
  Object.defineProperty(TimeRanges.prototype, Symbol.toStringTag, { value: 'TimeRanges', configurable: true });
  if (typeof globalThis.TimeRanges === 'undefined') {
    Object.defineProperty(globalThis, 'TimeRanges', { value: TimeRanges, writable: true, configurable: true });
  }

  function readRanges(msId, index) {
    var list;
    try { list = JSON.parse(__lumen_mse_buffered(msId, index)); } catch (e) { list = []; }
    return list;
  }

  // ── SourceBufferList ─────────────────────────────────────────────────────────
  function SourceBufferList() { throw new TypeError('Illegal constructor'); }
  SourceBufferList.prototype = Object.create(_base.prototype);
  Object.defineProperty(SourceBufferList.prototype, 'constructor', { value: SourceBufferList, writable: true, configurable: true });
  Object.defineProperty(SourceBufferList.prototype, 'length', {
    get: function () { return _state.get(this).items.length; }, enumerable: true, configurable: true,
  });
  ['onaddsourcebuffer', 'onremovesourcebuffer'].forEach(function (name) {
    Object.defineProperty(SourceBufferList.prototype, name, {
      get: function () { return _state.get(this).handlers[name] || null; },
      set: function (fn) {
        var st = _state.get(this);
        st.handlers[name] = (typeof fn === 'function' || (fn && typeof fn === 'object')) ? fn : null;
        // Mirror onto the shim's `on<type>` dispatch path.
        this['__on' + name.slice(2)] = st.handlers[name];
      },
      enumerable: true, configurable: true,
    });
  });
  Object.defineProperty(SourceBufferList.prototype, Symbol.toStringTag, { value: 'SourceBufferList', configurable: true });
  function makeList() {
    var list = Object.create(SourceBufferList.prototype);
    _base.call(list);
    _state.set(list, { items: [], handlers: Object.create(null) });
    return list;
  }
  function listSync(list) {
    var items = _state.get(list).items;
    var i = 0;
    for (; i < items.length; i++) list[i] = items[i];
    while (list.hasOwnProperty(i)) { delete list[i]; i++; }
  }
  function listAdd(list, sb) { _state.get(list).items.push(sb); listSync(list); }
  function listRemove(list, sb) {
    var items = _state.get(list).items;
    var i = items.indexOf(sb);
    if (i >= 0) items.splice(i, 1);
    delete list[items.length];
    listSync(list);
  }
  SourceBufferList.prototype.item = function (i) { var items = _state.get(this).items; return items[Number(i) >>> 0] || null; };

  // ── SourceBuffer ─────────────────────────────────────────────────────────────
  function SourceBuffer() { throw new TypeError('Illegal constructor'); }
  SourceBuffer.prototype = Object.create(_base.prototype);
  Object.defineProperty(SourceBuffer.prototype, 'constructor', { value: SourceBuffer, writable: true, configurable: true });
  Object.defineProperty(SourceBuffer.prototype, Symbol.toStringTag, { value: 'SourceBuffer', configurable: true });

  function makeBuffer(ms, index, type) {
    var sb = Object.create(SourceBuffer.prototype);
    _base.call(sb);
    _state.set(sb, {
      ms: ms, index: index, type: type, removed: false, updating: false, token: 0,
      mode: 'segments', timestampOffset: 0, appendWindowStart: 0, appendWindowEnd: Infinity,
      handlers: Object.create(null), generateTimestamps: false,
    });
    return sb;
  }
  function sbState(sb) {
    var st = _state.get(sb);
    if (!st || st.index === undefined) throw new TypeError('Illegal invocation');
    return st;
  }
  function msId(st) { return _state.get(st.ms).id; }

  Object.defineProperties(SourceBuffer.prototype, {
    updating: { get: function () { return sbState(this).updating; }, enumerable: true, configurable: true },
    buffered: {
      get: function () {
        var st = sbState(this);
        if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
        var list = readRanges(msId(st), st.index);
        var key = JSON.stringify(list);
        if (!st.rangesCache || st.rangesCache.key !== key) st.rangesCache = { key: key, obj: makeRanges(list) };
        return st.rangesCache.obj;
      }, enumerable: true, configurable: true,
    },
    mode: {
      get: function () { return sbState(this).mode; },
      set: function (v) {
        var st = sbState(this);
        v = String(v);
        if (v !== 'segments' && v !== 'sequence') return; // invalid enum values are ignored by WebIDL
        if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
        if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
        if (st.generateTimestamps && v === 'segments') throw new TypeError('The mode \'segments\' is not allowed for this byte stream format.');
        if (_state.get(st.ms).readyState === 'ended') _lumen_mse_open(st.ms);
        st.mode = v;
        __lumen_mse_set_sequence_mode(msId(st), st.index, v === 'sequence');
      }, enumerable: true, configurable: true,
    },
    timestampOffset: {
      get: function () { return sbState(this).timestampOffset; },
      set: function (v) {
        var st = sbState(this);
        v = Number(v);
        if (!isFinite(v)) throw new TypeError('The provided double value is non-finite.');
        if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
        if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
        if (_state.get(st.ms).readyState === 'ended') _lumen_mse_open(st.ms);
        st.timestampOffset = v;
        __lumen_mse_set_timestamp_offset(msId(st), st.index, v);
      }, enumerable: true, configurable: true,
    },
    appendWindowStart: {
      get: function () { return sbState(this).appendWindowStart; },
      set: function (v) {
        var st = sbState(this);
        v = Number(v);
        if (!isFinite(v) && v !== Infinity) throw new TypeError('The provided double value is non-finite.');
        if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
        if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
        if (v < 0 || v >= st.appendWindowEnd) throw new TypeError('The value provided is outside the range [0, appendWindowEnd).');
        st.appendWindowStart = v;
        __lumen_mse_set_append_window(msId(st), st.index, v, st.appendWindowEnd);
      }, enumerable: true, configurable: true,
    },
    appendWindowEnd: {
      get: function () { return sbState(this).appendWindowEnd; },
      set: function (v) {
        var st = sbState(this);
        v = Number(v);
        if (v !== v) throw new TypeError('The provided double value is non-finite.');
        if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
        if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
        if (v <= st.appendWindowStart) throw new TypeError('The value provided is not greater than appendWindowStart.');
        st.appendWindowEnd = v;
        __lumen_mse_set_append_window(msId(st), st.index, st.appendWindowStart, v);
      }, enumerable: true, configurable: true,
    },
  });
  ['onupdatestart', 'onupdate', 'onupdateend', 'onerror', 'onabort'].forEach(function (name) {
    Object.defineProperty(SourceBuffer.prototype, name, {
      get: function () { return sbState(this).handlers[name] || null; },
      set: function (fn) {
        var st = sbState(this);
        st.handlers[name] = (typeof fn === 'function' || (fn && typeof fn === 'object')) ? fn : null;
        this['__' + name] = st.handlers[name];
      },
      enumerable: true, configurable: true,
    });
  });

  // §3.5.1 «prepare append» + §3.5.5 «buffer append».
  SourceBuffer.prototype.appendBuffer = function (data) {
    var st = sbState(this);
    var bytes;
    if (data instanceof ArrayBuffer) bytes = new Uint8Array(data);
    else if (ArrayBuffer.isView(data)) bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    else throw new TypeError('The provided value is not of type \'(ArrayBuffer or ArrayBufferView)\'.');
    if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
    if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
    var ms = _state.get(st.ms);
    if (ms.readyState === 'ended') _lumen_mse_open(st.ms);
    st.updating = true;
    var token = ++st.token;
    var self = this;
    task(function () { fire(self, 'updatestart'); });
    task(function () {
      if (st.token !== token || !st.updating) return; // aborted
      var res = __lumen_mse_append(ms.id, st.index, bytes);
      st.updating = false;
      if (res.indexOf('error:') === 0) {
        fire(self, 'error');
        fire(self, 'updateend');
        _lumen_mse_end_of_stream(st.ms, 'decode');
        return;
      }
      if (res.indexOf('init') === 0) {
        ms.initReceived = true;
        // §3.5.8 «initialization segment received»: the buffer becomes active, and a NaN
        // duration is taken from the segment (+Infinity when it carries none).
        var activeList = ms.active;
        var activeItems = _state.get(activeList).items;
        if (activeItems.indexOf(self) < 0) {
          // `activeSourceBuffers` keeps the order of `sourceBuffers`.
          activeItems.push(self);
          activeItems.sort(function (x, y) { return ms.buffers.indexOf(x) - ms.buffers.indexOf(y); });
          listSync(activeList);
          task(function () { fire(activeList, 'addsourcebuffer'); });
        }
        if (ms.duration !== ms.duration) {
          var secs = res.length > 5 ? Number(res.slice(5)) : Infinity;
          _lumen_mse_set_duration(st.ms, secs === secs ? secs : Infinity);
        }
      }
      fire(self, 'update');
      fire(self, 'updateend');
    });
  };

  // §3.5.7 «range removal».
  SourceBuffer.prototype.remove = function (start, end) {
    var st = sbState(this);
    var ms = _state.get(st.ms);
    start = Number(start); end = Number(end);
    if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
    if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
    if (ms.duration !== ms.duration) throw new TypeError('The duration is NaN.');
    if (start !== start || start < 0 || start > ms.duration) throw new TypeError('The start value provided is outside the range [0, duration].');
    if (end !== end || end <= start) throw new TypeError('The end value provided must be greater than start.');
    if (ms.readyState === 'ended') _lumen_mse_open(st.ms);
    st.updating = true;
    var token = ++st.token;
    var self = this;
    task(function () { fire(self, 'updatestart'); });
    task(function () {
      if (st.token !== token || !st.updating) return;
      __lumen_mse_remove(ms.id, st.index, start, end);
      st.updating = false;
      fire(self, 'update');
      fire(self, 'updateend');
    });
  };

  // §3.5.3 «abort».
  SourceBuffer.prototype.abort = function () {
    var st = sbState(this);
    var ms = _state.get(st.ms);
    if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
    if (ms.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    var wasUpdating = st.updating;
    if (wasUpdating) { st.token++; st.updating = false; }
    __lumen_mse_abort(ms.id, st.index);
    st.appendWindowStart = 0;
    st.appendWindowEnd = Infinity;
    __lumen_mse_set_append_window(ms.id, st.index, 0, Infinity);
    if (wasUpdating) {
      var self = this;
      task(function () { fire(self, 'abort'); fire(self, 'updateend'); });
    }
  };

  // §3.5.8 «change type»: the container may change; the next init segment replaces the tracks.
  SourceBuffer.prototype.changeType = function (type) {
    var st = sbState(this);
    if (arguments.length < 1) throw new TypeError('1 argument required, but only 0 present.');
    type = String(type);
    if (type === '') throw new TypeError('The type provided is empty.');
    if (st.removed) throw domException('This SourceBuffer has been removed from the parent media source.', 'InvalidStateError');
    if (st.updating) throw domException('This SourceBuffer is still processing an append or remove operation.', 'InvalidStateError');
    if (!__lumen_mse_is_type_supported(type)) throw domException('The type provided (\'' + type + '\') is not supported.', 'NotSupportedError');
    var ms = _state.get(st.ms);
    if (ms.readyState !== 'open' && ms.readyState !== 'ended') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    if (!__lumen_mse_change_type(ms.id, st.index, type)) throw domException('The type provided (\'' + type + '\') is not supported.', 'NotSupportedError');
    if (ms.readyState === 'ended') _lumen_mse_open(st.ms);
    st.type = type;
  };

  // ── MediaSource ──────────────────────────────────────────────────────────────
  function MediaSource() {
    if (!new.target) throw new TypeError('Failed to construct \'MediaSource\': Please use the \'new\' operator, this DOM object constructor cannot be called as a function.');
    _base.call(this);
    var list = makeList();
    var active = makeList();
    _state.set(this, {
      id: __lumen_mse_create(), readyState: 'closed', duration: NaN, list: list, active: active,
      buffers: [], element: null, initReceived: false, handlers: Object.create(null),
      liveSeekableRange: null,
    });
    Object.defineProperty(this, '__lumen_mse_id', { value: _state.get(this).id });
  }
  MediaSource.prototype = Object.create(_base.prototype);
  Object.defineProperty(MediaSource.prototype, 'constructor', { value: MediaSource, writable: true, configurable: true });
  Object.defineProperty(MediaSource.prototype, Symbol.toStringTag, { value: 'MediaSource', configurable: true });

  function msState(ms) {
    var st = _state.get(ms);
    if (!st || st.buffers === undefined) throw new TypeError('Illegal invocation');
    return st;
  }
  function anyUpdating(st) {
    for (var i = 0; i < st.buffers.length; i++) if (_state.get(st.buffers[i]).updating) return true;
    return false;
  }

  Object.defineProperties(MediaSource.prototype, {
    sourceBuffers: { get: function () { return msState(this).list; }, enumerable: true, configurable: true },
    activeSourceBuffers: { get: function () { return msState(this).active; }, enumerable: true, configurable: true },
    readyState: { get: function () { return msState(this).readyState; }, enumerable: true, configurable: true },
    duration: {
      get: function () { var st = msState(this); return st.readyState === 'closed' ? NaN : st.duration; },
      set: function (v) {
        var st = msState(this);
        v = Number(v);
        if (v !== v) throw new TypeError('The provided double value is non-finite.');
        if (v < 0) throw new TypeError('The value provided (' + v + ') is outside the range [0, Infinity].');
        if (st.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
        if (anyUpdating(st)) throw domException('One or more SourceBuffers are still processing an append or remove operation.', 'InvalidStateError');
        _lumen_mse_set_duration(this, v);
      }, enumerable: true, configurable: true,
    },
  });
  ['onsourceopen', 'onsourceended', 'onsourceclose'].forEach(function (name) {
    Object.defineProperty(MediaSource.prototype, name, {
      get: function () { return msState(this).handlers[name] || null; },
      set: function (fn) {
        var st = msState(this);
        st.handlers[name] = (typeof fn === 'function' || (fn && typeof fn === 'object')) ? fn : null;
        this['__' + name] = st.handlers[name];
      },
      enumerable: true, configurable: true,
    });
  });

  MediaSource.prototype.addSourceBuffer = function (type) {
    var st = msState(this);
    if (arguments.length < 1) throw new TypeError('1 argument required, but only 0 present.');
    type = String(type);
    if (type === '') throw new TypeError('The type provided is empty.');
    if (!__lumen_mse_is_type_supported(type)) throw domException('The type provided (\'' + type + '\') is unsupported.', 'NotSupportedError');
    if (st.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    var index = __lumen_mse_add_buffer(st.id, type);
    if (index < 0) throw domException('The type provided (\'' + type + '\') is unsupported.', 'NotSupportedError');
    var sb = makeBuffer(this, index, type);
    st.buffers.push(sb);
    listAdd(st.list, sb);
    var list = st.list;
    task(function () { fire(list, 'addsourcebuffer'); });
    return sb;
  };

  MediaSource.prototype.removeSourceBuffer = function (sb) {
    var st = msState(this);
    if (!(sb instanceof SourceBuffer)) throw new TypeError('The parameter 1 is not of type \'SourceBuffer\'.');
    var i = st.buffers.indexOf(sb);
    if (i < 0) throw domException('The SourceBuffer provided is not contained in this MediaSource.', 'NotFoundError');
    var bs = _state.get(sb);
    if (bs.updating) { bs.token++; bs.updating = false; task(function () { fire(sb, 'abort'); fire(sb, 'updateend'); }); }
    bs.removed = true;
    st.buffers.splice(i, 1);
    listRemove(st.list, sb);
    listRemove(st.active, sb);
    var list = st.list;
    task(function () { fire(list, 'removesourcebuffer'); });
  };

  // §2.4.7 «end of stream» (with `error`) / «endOfStream(error)».
  MediaSource.prototype.endOfStream = function (error) {
    var st = msState(this);
    if (error !== undefined && error !== 'network' && error !== 'decode') {
      throw new TypeError('The provided value \'' + error + '\' is not a valid enum value of type EndOfStreamError.');
    }
    if (st.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    if (anyUpdating(st)) throw domException('One or more SourceBuffers are still processing an append or remove operation.', 'InvalidStateError');
    _lumen_mse_end_of_stream(this, error);
  };

  MediaSource.prototype.setLiveSeekableRange = function (start, end) {
    var st = msState(this);
    start = Number(start); end = Number(end);
    if (st.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    if (start !== start || end !== end || start < 0 || start > end) throw new TypeError('The provided range is invalid.');
    st.liveSeekableRange = [start, end];
  };
  MediaSource.prototype.clearLiveSeekableRange = function () {
    var st = msState(this);
    if (st.readyState !== 'open') throw domException('The MediaSource\'s readyState is not \'open\'.', 'InvalidStateError');
    st.liveSeekableRange = null;
  };

  MediaSource.isTypeSupported = function (type) {
    if (arguments.length < 1) throw new TypeError('1 argument required, but only 0 present.');
    type = String(type);
    if (type === '') return false;
    return __lumen_mse_is_type_supported(type) === true;
  };
  Object.defineProperty(MediaSource, 'canConstructInDedicatedWorker', { value: false, enumerable: true, configurable: true });

  // ── Hooks used by `video_element.js` and the object-URL store ───────────────
  // §2.4.2 «attach to a media element» → readyState 'open' + `sourceopen`.
  function _lumen_mse_open(ms) {
    var st = _state.get(ms);
    var was = st.readyState;
    st.readyState = 'open';
    __lumen_mse_end_of_stream(st.id, false);
    if (was === 'ended') task(function () { fire(ms, 'sourceopen'); });
    else if (was === 'closed') task(function () { fire(ms, 'sourceopen'); });
  }
  function _lumen_mse_set_duration(ms, v) {
    var st = _state.get(ms);
    st.duration = v;
    __lumen_mse_set_duration(st.id, v);
    if (st.element && typeof st.element.__lumen_mse_duration_changed === 'function') st.element.__lumen_mse_duration_changed();
  }
  function _lumen_mse_end_of_stream(ms, error) {
    var st = _state.get(ms);
    if (!error) {
      // §2.4.7 «end of stream» step 1: the duration becomes the largest buffered end time.
      var hi = 0;
      for (var i = 0; i < st.buffers.length; i++) {
        var rr = readRanges(st.id, _state.get(st.buffers[i]).index);
        if (rr.length) hi = Math.max(hi, rr[rr.length - 1][1]);
      }
      if (st.buffers.length && hi !== st.duration) _lumen_mse_set_duration(ms, hi);
    }
    st.readyState = 'ended';
    __lumen_mse_end_of_stream(st.id, true);
    task(function () { fire(ms, 'sourceended'); });
    if (error && st.element && typeof st.element.__lumen_mse_error === 'function') st.element.__lumen_mse_error(error);
    else if (st.element && typeof st.element.__lumen_mse_ended === 'function') st.element.__lumen_mse_ended();
  }
  // Called by the media element: attach (`el` set) or detach (`el` null).
  globalThis._lumen_mse_attach_element = function (ms, el) {
    var st = _state.get(ms);
    if (!st) return false;
    if (el) {
      if (st.element && st.element !== el) return false; // already attached elsewhere
      st.element = el;
      _lumen_mse_open(ms);
      return true;
    }
    if (st.element) {
      st.element = null;
      var was = st.readyState;
      st.readyState = 'closed';
      st.buffers.slice().forEach(function (sb) {
        var bs = _state.get(sb);
        bs.removed = true; bs.updating = false; bs.token++;
      });
      st.buffers = [];
      st.list = makeList(); st.active = makeList();
      st.duration = NaN;
      if (was !== 'closed') task(function () { fire(ms, 'sourceclose'); });
    }
    return true;
  };
  // Intersection of the buffered ranges of all buffers (element `buffered`, §2.4.8 «HTMLMediaElement.buffered»).
  globalThis._lumen_mse_element_ranges = function (ms) {
    var st = _state.get(ms);
    if (!st || st.buffers.length === 0) return [];
    var acc = null;
    for (var i = 0; i < st.buffers.length; i++) {
      var r = readRanges(st.id, _state.get(st.buffers[i]).index);
      if (acc === null) { acc = r; continue; }
      var out = [], a = 0, b = 0;
      while (a < acc.length && b < r.length) {
        var s = Math.max(acc[a][0], r[b][0]), e = Math.min(acc[a][1], r[b][1]);
        if (s < e) out.push([s, e]);
        if (acc[a][1] < r[b][1]) a++; else b++;
      }
      acc = out;
    }
    if (st.readyState === 'ended' && acc && acc.length) {
      var hi = 0;
      for (var j = 0; j < st.buffers.length; j++) {
        var rr = readRanges(st.id, _state.get(st.buffers[j]).index);
        if (rr.length) hi = Math.max(hi, rr[rr.length - 1][1]);
      }
      acc[acc.length - 1][1] = Math.max(acc[acc.length - 1][1], hi);
    }
    return acc || [];
  };
  globalThis._lumen_mse_info = function (ms) {
    var st = _state.get(ms);
    return st ? { id: st.id, readyState: st.readyState, duration: st.duration, initReceived: st.initReceived } : null;
  };

  [['MediaSource', MediaSource], ['SourceBuffer', SourceBuffer], ['SourceBufferList', SourceBufferList]].forEach(function (p) {
    Object.defineProperty(globalThis, p[0], { value: p[1], writable: true, configurable: true });
  });
})();
