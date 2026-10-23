// CSS Painting API Level 1 (Houdini) — Worklet, CSS.paintWorklet, registerPaint().
//
// Lumen has no separate worklet global scope: a module runs on the page's own V8
// context inside `new Function('registerPaint', …)`, so `registerPaint` is visible
// to the module only and never leaks onto `window`. Rendering is pull-based: the
// host (shell / headless driver) hands over the `paint()` layers of the laid-out
// page with `__lumen_paint_set_requests(json)` and calls `__lumen_paint_flush()`;
// each layer is drawn into a throw-away `OffscreenCanvas`, whose pixels
// `_lumen_paint_publish(key, canvasId)` parks in a Rust queue the host then drains.
(function (global) {
  'use strict';

  var defs = new Map();       // paint name -> { cls, inputProperties, inputArguments, alpha, instance }
  var requests = [];          // [{ key, node, name, args, w, h, dpr }] from the last layout
  var rendered = new Map();   // key -> signature of the last bitmap published for it
  var epoch = 0;              // bumped by every successful registerPaint()

  function toStringList(value, what) {
    if (value === undefined) return [];
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')
        || typeof value[Symbol.iterator] !== 'function') {
      throw new TypeError("registerPaint: " + what + " is not iterable");
    }
    var out = [];
    for (var item of value) out.push(String(item));
    return out;
  }

  // Paint API §2.3 «registerPaint(name, paintCtor)».
  function registerPaint(name, cls) {
    name = String(name);
    if (defs.has(name)) {
      throw new DOMException("registerPaint: '" + name + "' is already registered", 'NotSupportedError');
    }
    var inputProperties = toStringList(cls != null ? cls.inputProperties : undefined, 'inputProperties');
    var inputArguments = toStringList(cls != null ? cls.inputArguments : undefined, 'inputArguments');
    var options = cls != null ? cls.contextOptions : undefined;
    var alpha = true;
    if (options !== undefined && options !== null) {
      if (typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError('registerPaint: contextOptions is not an object');
      }
      if (options.alpha !== undefined) alpha = !!options.alpha;
    }
    if (typeof cls !== 'function') {
      throw new TypeError('registerPaint: the second argument is not a constructor');
    }
    var proto = cls.prototype;
    if (proto === null || (typeof proto !== 'object' && typeof proto !== 'function')) {
      throw new TypeError('registerPaint: the class has no prototype object');
    }
    if (typeof proto.paint !== 'function') {
      throw new TypeError('registerPaint: the class has no paint() method');
    }
    defs.set(name, {
      cls: cls, inputProperties: inputProperties, inputArguments: inputArguments,
      alpha: alpha, instance: null
    });
    epoch++;
  }

  function moduleSource(url) {
    if (url.slice(0, 5) === 'blob:') {
      var blob = (typeof _lumen_blob_url_entry === 'function') ? _lumen_blob_url_entry(url) : null;
      if (!blob) return Promise.reject(new Error('object URL is not registered'));
      return Promise.resolve(new TextDecoder().decode(new Uint8Array(blob._bytes)));
    }
    return fetch(url).then(function (r) {
      if (!r.ok) throw new Error('module fetch failed: HTTP ' + r.status);
      return r.text();
    });
  }

  // Worklets §3 «addModule()»: fetch, evaluate, resolve; any failure is an AbortError.
  function Worklet() { throw new TypeError('Illegal constructor'); }
  Worklet.prototype.addModule = function addModule(moduleURL, options) {
    var self = this;
    return new Promise(function (resolve, reject) {
      if (!(self instanceof Worklet)) throw new TypeError('Illegal invocation');
      var resolved;
      try {
        resolved = new URL(String(moduleURL), global.document ? global.document.baseURI : undefined).href;
      } catch (e) {
        reject(new DOMException('addModule: invalid module URL', 'SyntaxError'));
        return;
      }
      moduleSource(resolved).then(function (src) {
        try {
          new Function('registerPaint', src)(registerPaint);
        } catch (e) {
          reject(new DOMException('addModule: module failed to evaluate: ' + e, 'AbortError'));
          return;
        }
        resolve(undefined);
      }, function (e) {
        reject(new DOMException('addModule: ' + (e && e.message || e), 'AbortError'));
      });
    });
  };
  Object.defineProperty(Worklet.prototype, Symbol.toStringTag, { value: 'Worklet', configurable: true });
  Object.defineProperty(global, 'Worklet', { value: Worklet, writable: true, configurable: true });

  var paintWorklet = Object.create(Worklet.prototype);
  if (!global.CSS) global.CSS = {};
  Object.defineProperty(global.CSS, 'paintWorklet', {
    get: function () { return paintWorklet; }, enumerable: true, configurable: true
  });

  // StylePropertyMapReadOnly restricted to the worklet's `inputProperties`.
  function InputStyleMap(map, props) {
    this._map = map;
    this._props = props;
  }
  InputStyleMap.prototype.get = function (p) {
    p = String(p);
    return this._props.indexOf(p) < 0 ? undefined : this._map.get(p);
  };
  InputStyleMap.prototype.getAll = function (p) {
    p = String(p);
    return this._props.indexOf(p) < 0 ? [] : this._map.getAll(p);
  };
  InputStyleMap.prototype.has = function (p) { return this._props.indexOf(String(p)) >= 0; };
  Object.defineProperty(InputStyleMap.prototype, 'size', {
    get: function () { return this._props.length; }
  });
  InputStyleMap.prototype.forEach = function (cb, thisArg) {
    var self = this;
    this._props.forEach(function (p) { cb.call(thisArg, self._map.getAll(p), p, self); });
  };
  InputStyleMap.prototype.entries = function () {
    var self = this;
    return this._props.map(function (p) { return [p, self._map.getAll(p)]; })[Symbol.iterator]();
  };
  InputStyleMap.prototype.keys = function () { return this._props.slice()[Symbol.iterator](); };
  InputStyleMap.prototype.values = function () {
    var self = this;
    return this._props.map(function (p) { return self._map.getAll(p); })[Symbol.iterator]();
  };
  InputStyleMap.prototype[Symbol.iterator] = InputStyleMap.prototype.entries;

  function PaintSize(w, h) {
    Object.defineProperty(this, 'width', { value: w, enumerable: true });
    Object.defineProperty(this, 'height', { value: h, enumerable: true });
  }
  Object.defineProperty(PaintSize.prototype, Symbol.toStringTag, { value: 'PaintSize', configurable: true });

  function argValue(text) {
    // Without a registered-syntax parser the argument stays its source text, which
    // is what `toString()` / `CSSStyleValue` serialisation gives for these tests.
    return { toString: function () { return text; }, value: text };
  }

  function signature(req, def, styleMap) {
    var parts = [epoch, req.w, req.h, req.dpr, req.args.join('\u0001')];
    for (var i = 0; i < def.inputProperties.length; i++) {
      var v = styleMap.get(def.inputProperties[i]);
      parts.push(v === undefined || v === null ? '' : String(v));
    }
    return parts.join('\u0002');
  }

  function renderOne(req) {
    var def = defs.get(req.name);
    if (!def) return;
    var el = _lumen_make_element(req.node);
    if (!el || typeof el.computedStyleMap !== 'function') return;
    var styleMap = new InputStyleMap(el.computedStyleMap(), def.inputProperties);
    var sig = signature(req, def, styleMap);
    if (rendered.get(req.key) === sig) return;

    var dpr = req.dpr > 0 ? req.dpr : 1;
    var pxW = Math.max(1, Math.min(4096, Math.ceil(req.w * dpr)));
    var pxH = Math.max(1, Math.min(4096, Math.ceil(req.h * dpr)));
    var canvas = new OffscreenCanvas(pxW, pxH);
    var ctx = canvas.getContext('2d');
    ctx.scale(pxW / req.w, pxH / req.h);
    if (!def.alpha) {
      ctx.fillStyle = '#000';
      ctx.fillRect(0, 0, req.w, req.h);
    }
    try {
      if (!def.instance) def.instance = new def.cls();
      var args = req.args.map(argValue);
      def.instance.paint(ctx, new PaintSize(req.w, req.h), styleMap, args);
    } catch (e) {
      // Paint API §4.4: a throwing paint() leaves the image invalid — nothing is drawn.
      def.instance = null;
      rendered.set(req.key, sig);
      return;
    }
    _lumen_paint_publish(req.key, canvas.__canvas_id__);
    rendered.set(req.key, sig);
  }

  global.__lumen_paint_set_requests = function (json) {
    requests = JSON.parse(json);
    var live = new Set();
    for (var i = 0; i < requests.length; i++) live.add(requests[i].key);
    rendered.forEach(function (_v, k) { if (!live.has(k)) rendered.delete(k); });
  };
  global.__lumen_paint_flush = function () {
    for (var i = 0; i < requests.length; i++) renderOne(requests[i]);
  };
})(globalThis)
