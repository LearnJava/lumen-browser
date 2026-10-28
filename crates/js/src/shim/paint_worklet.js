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
  var AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

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

  // Reads the class's static registration data once: `inputProperties`, `inputArguments`,
  // `contextOptions.alpha` (Paint API §2.3 steps 5-10).
  function readStatics(cls) {
    var options = cls != null ? cls.contextOptions : undefined;
    var alpha = true;
    if (options !== undefined && options !== null) {
      if (typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError('registerPaint: contextOptions is not an object');
      }
      if (options.alpha !== undefined) alpha = !!options.alpha;
    }
    return {
      inputProperties: toStringList(cls != null ? cls.inputProperties : undefined, 'inputProperties'),
      inputArguments: toStringList(cls != null ? cls.inputArguments : undefined, 'inputArguments'),
      alpha: alpha
    };
  }

  // Paint API §2.3 «registerPaint(name, paintCtor)».
  function registerPaint(name, cls) {
    name = String(name);
    if (defs.has(name)) {
      throw new DOMException("registerPaint: '" + name + "' is already registered", 'InvalidModificationError');
    }
    var statics = readStatics(cls);
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
    // The spec registers a definition in every global scope of the worklet and the
    // document keeps the name only if all of them agree (§2.3 "document paint
    // definition"). There is one scope here, so the static getters are read a second
    // time — a class whose `inputProperties`/`inputArguments` differ between reads
    // would disagree across scopes, and its name becomes invalid (never painted).
    var again = readStatics(cls);
    var consistent = JSON.stringify([statics.inputProperties, statics.inputArguments, statics.alpha])
      === JSON.stringify([again.inputProperties, again.inputArguments, again.alpha]);
    defs.set(name, {
      cls: cls, inputProperties: statics.inputProperties, inputArguments: statics.inputArguments,
      alpha: statics.alpha, instance: null, invalid: !consistent
    });
    epoch++;
    _lumen_paint_registered();
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

  function workletImport() {
    return Promise.reject(new TypeError('import() is not supported in a worklet'));
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
        var run;
        // Worklets do not support `import()` (Worklets §2.1: the module map's dynamic import
        // always fails with a TypeError), so a call is routed to a rejecting stub.
        src = src.replace(/(^|[^.\w$])import\s*\(/g, '$1__lumen_worklet_import(');
        try {
          run = new Function('registerPaint', '__lumen_worklet_import', src);
        } catch (e) {
          // Worklet modules are ES modules, so a top-level `await` is legal: evaluate the
          // body as an async function (a genuine syntax error fails here again).
          try {
            run = new AsyncFunction('registerPaint', '__lumen_worklet_import', src);
          } catch (e2) {
            reject(new DOMException('addModule: module failed to evaluate: ' + e, 'AbortError'));
            return;
          }
        }
        try {
          var done = run(registerPaint, workletImport);
          if (done && typeof done.then === 'function') {
            done.then(function () { resolve(undefined); }, function (e) {
              reject(new DOMException('addModule: module failed to evaluate: ' + e, 'AbortError'));
            });
            return;
          }
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

  // ── Typed values of custom properties (Paint API §5 / CSS Properties & Values §5) ──
  // `computedStyleMap()` answers a custom property with its serialised text; the worklet
  // sees a typed value instead: an unregistered (or `*`) property is a CSSUnparsedValue,
  // a registered one is reified by the component of its syntax that matches the value.
  // The Typed OM shim loads after this file, so the class is built on first use.
  var CSSImageValueCtor = null;
  function imageValueClass() {
    if (CSSImageValueCtor) return CSSImageValueCtor;
    if (typeof global.CSSImageValue === 'function') return (CSSImageValueCtor = global.CSSImageValue);
    CSSImageValueCtor = function CSSImageValue() { throw new TypeError('Illegal constructor'); };
    CSSImageValueCtor.prototype = Object.create(CSSStyleValue.prototype);
    CSSImageValueCtor.prototype.constructor = CSSImageValueCtor;
    Object.defineProperty(global, 'CSSImageValue', { value: CSSImageValueCtor, writable: true, configurable: true });
    return CSSImageValueCtor;
  }

  // Splits `text` on top-level `sep` (',' or whitespace), outside `()` and quotes.
  function splitTop(text, sep) {
    var out = [], depth = 0, quote = null, cur = '';
    for (var i = 0; i < text.length; i++) {
      var ch = text.charAt(i);
      if (quote) { cur += ch; if (ch === quote) quote = null; continue; }
      if (ch === '"' || ch === "'") { quote = ch; cur += ch; continue; }
      if (ch === '(') depth++;
      else if (ch === ')') depth--;
      if (depth === 0 && (sep === ',' ? ch === ',' : /\s/.test(ch))) {
        if (cur.trim() !== '') out.push(cur.trim());
        cur = '';
      } else {
        cur += ch;
      }
    }
    if (cur.trim() !== '') out.push(cur.trim());
    return out;
  }

  var NUM = '[+-]?(?:\\d+\\.?\\d*|\\.\\d+)(?:[eE][+-]?\\d+)?';
  var UNITS = {
    length: 'px|em|rem|ex|rex|cap|rcap|ch|rch|ic|ric|lh|rlh|vw|vh|vi|vb|vmin|vmax|svw|svh|lvw|lvh|dvw|dvh|cm|mm|q|in|pt|pc|cqw|cqh|cqi|cqb|cqmin|cqmax',
    angle: 'deg|grad|rad|turn',
    time: 's|ms',
    resolution: 'dpi|dpcm|dppx|x',
    flex: 'fr'
  };
  // `{ n, unit }` when `text` is a dimension of one of the allowed `kinds`, else null.
  function dimension(text, kinds) {
    var m = new RegExp('^(' + NUM + ')([a-zA-Z%]*)$').exec(text);
    if (!m) return null;
    var unit = m[2].toLowerCase();
    if (unit === '') {
      if (kinds.number) return { n: Number(m[1]), unit: 'number' };
      return (kinds.length && Number(m[1]) === 0) ? { n: 0, unit: 'px' } : null;
    }
    if (unit === '%') return kinds.percentage ? { n: Number(m[1]), unit: 'percent' } : null;
    for (var k in UNITS) {
      if (kinds[k] && new RegExp('^(?:' + UNITS[k] + ')$').test(unit)) return { n: Number(m[1]), unit: m[2] };
    }
    return null;
  }
  function unitValue(d) { return new CSSUnitValue(d.n, d.unit); }
  // `calc(a + b)` of dimensions → CSSMathSum; any other maths stays an opaque CSSStyleValue.
  function mathValue(text, kinds) {
    var m = /^calc\((.*)\)$/i.exec(text);
    if (!m) return null;
    var terms = m[1].split(/\s+\+\s+/), vals = [];
    for (var i = 0; i < terms.length; i++) {
      var d = dimension(terms[i].trim(), kinds);
      if (!d) return new CSSStyleValue(text);
      vals.push(unitValue(d));
    }
    return vals.length > 1 ? new CSSMathSum(vals) : vals[0];
  }
  var COLOR_FN = /^(?:#[0-9a-f]{3,8}|(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color|color-mix|light-dark)\(.*\))$/i;
  var IMAGE_FN = /^(?:(?:repeating-)?(?:linear|radial|conic)-gradient|-webkit-[a-z-]*gradient|image-set|-webkit-image-set|cross-fade|element|paint)\(.*\)$/i;
  var URL_FN = /^url\(.*\)$/i;
  var IDENT = /^-?[A-Za-z_][\w-]*$/;
  var TYPE_KINDS = {
    'length': { length: 1 }, 'percentage': { percentage: 1 }, 'length-percentage': { length: 1, percentage: 1 },
    'number': { number: 1 }, 'integer': { number: 1 }, 'angle': { angle: 1 }, 'time': { time: 1 },
    'resolution': { resolution: 1 }, 'flex': { flex: 1 }
  };
  // The CSSStyleValue `item` reifies to under `<type>`, or null when it is not of that type.
  function reifyItem(item, type) {
    var kinds = TYPE_KINDS[type];
    if (kinds) {
      var d = dimension(item, kinds);
      if (d) return (type === 'integer' && !/^[+-]?\d+$/.test(item)) ? null : unitValue(d);
      return mathValue(item, kinds);
    }
    if (type === 'color') {
      return (COLOR_FN.test(item) || IDENT.test(item)) ? new CSSStyleValue(item) : null;
    }
    if (type === 'url') return URL_FN.test(item) ? new CSSStyleValue(item) : null;
    if (type === 'image') {
      if (URL_FN.test(item)) {
        var v = new CSSStyleValue(item);
        Object.setPrototypeOf(v, imageValueClass().prototype);
        return v;
      }
      return IMAGE_FN.test(item) ? new CSSStyleValue(item) : null;
    }
    if (type === 'custom-ident') return IDENT.test(item) ? new CSSKeywordValue(item) : null;
    if (type === 'transform-function' || type === 'transform-list') {
      return /^[a-z0-9]+\(.*\)$/i.test(item) ? new CSSStyleValue(item) : null;
    }
    if (type === 'string') return /^(?:"[^"]*"|'[^']*')$/.test(item) ? new CSSStyleValue(item) : null;
    return null;
  }
  // Typed values of the registered custom property `reg` holding `raw`; null if none fits.
  function reifyRegistered(raw, reg) {
    var alts = String(reg.syntax).split('|');
    for (var a = 0; a < alts.length; a++) {
      var alt = alts[a].trim();
      var m = /^<([a-z-]+)>([+#]?)$/.exec(alt);
      if (!m) {
        if (alt !== '' && alt === raw) return [new CSSKeywordValue(raw)];
        continue;
      }
      var items = m[2] === '#' ? splitTop(raw, ',') : m[2] === '+' ? splitTop(raw, ' ') : [raw];
      if (items.length === 0) continue;
      var out = [];
      for (var i = 0; i < items.length; i++) {
        var v = reifyItem(items[i], m[1]);
        if (!v) break;
        out.push(v);
      }
      if (out.length === items.length) return out;
    }
    return null;
  }
  function unparsed(raw) { return new CSSUnparsedValue(raw === '' ? [] : [raw]); }
  function customValues(name, raw) {
    var regs = __lumen_C._lumen_registered_properties;
    var reg = regs && regs.get(name);
    if (!reg || reg.syntax === '*') return [unparsed(raw)];
    if (raw === '') return [];
    return reifyRegistered(raw, reg) || [unparsed(raw)];
  }

  // StylePropertyMapReadOnly restricted to the worklet's `inputProperties`.
  function InputStyleMap(map, props, el) {
    this._map = map;
    this._props = props;
    this._el = el;
  }
  InputStyleMap.prototype._values = function (p) {
    // `getComputedStyle()` carries the running CSS animations / transitions of the property.
    if (p.slice(0, 2) === '--') return customValues(p, String(getComputedStyle(this._el).getPropertyValue(p) || '').trim());
    var v = this._map.get(p);
    return v === undefined ? [] : [v];
  };
  InputStyleMap.prototype.get = function (p) {
    p = String(p);
    if (this._props.indexOf(p) < 0) return undefined;
    return this._values(p)[0];
  };
  InputStyleMap.prototype.getAll = function (p) {
    p = String(p);
    return this._props.indexOf(p) < 0 ? [] : this._values(p);
  };
  InputStyleMap.prototype.has = function (p) { return this._props.indexOf(String(p)) >= 0; };
  Object.defineProperty(InputStyleMap.prototype, 'size', {
    get: function () { return this._props.length; }
  });
  InputStyleMap.prototype.forEach = function (cb, thisArg) {
    var self = this;
    this._props.forEach(function (p) { cb.call(thisArg, self._values(p), p, self); });
  };
  InputStyleMap.prototype.entries = function () {
    var self = this;
    return this._props.map(function (p) { return [p, self._values(p)]; })[Symbol.iterator]();
  };
  InputStyleMap.prototype.keys = function () { return this._props.slice()[Symbol.iterator](); };
  InputStyleMap.prototype.values = function () {
    var self = this;
    return this._props.map(function (p) { return self._values(p); })[Symbol.iterator]();
  };
  InputStyleMap.prototype[Symbol.iterator] = InputStyleMap.prototype.entries;
  Object.defineProperty(InputStyleMap.prototype, Symbol.toStringTag, { value: 'StylePropertyMapReadOnly', configurable: true });

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
      parts.push(styleMap.getAll(def.inputProperties[i]).map(String).join('\u0003'));
    }
    return parts.join('\u0002');
  }

  function renderOne(req) {
    var def = defs.get(req.name);
    if (!def || def.invalid) return;
    var el = _lumen_make_element(req.node);
    if (!el || typeof el.computedStyleMap !== 'function') return;
    var styleMap = new InputStyleMap(el.computedStyleMap(), def.inputProperties, el);
    var sig = signature(req, def, styleMap);
    if (rendered.get(req.key) === sig) return;

    var dpr = req.dpr > 0 ? req.dpr : 1;
    var pxW = Math.max(1, Math.min(4096, Math.ceil(req.w * dpr)));
    var pxH = Math.max(1, Math.min(4096, Math.ceil(req.h * dpr)));
    var canvas = new OffscreenCanvas(pxW, pxH);
    // `contextOptions.alpha: false` — an opaque bitmap that starts black.
    var ctx = canvas.getContext('2d', { alpha: def.alpha });
    ctx.scale(pxW / req.w, pxH / req.h);
    function publish() {
      _lumen_paint_publish(req.key, canvas.__canvas_id__);
    }
    // Paint API §4.4: a throwing (or rejecting) paint() leaves the image invalid — nothing is drawn.
    function fail() {
      def.instance = null;
    }
    rendered.set(req.key, sig);
    try {
      if (!def.instance) def.instance = new def.cls();
      var args = req.args.map(argValue);
      var done = def.instance.paint(ctx, new PaintSize(req.w, req.h), styleMap, args);
      if (done && typeof done.then === 'function') {
        // An `async paint()` finishes drawing in a microtask; the host runs them
        // before it reads the published bitmaps.
        done.then(publish, fail);
        return;
      }
    } catch (e) {
      fail();
      return;
    }
    publish();
  }

  __lumen_C.__lumen_paint_set_requests = function (json) {
    requests = JSON.parse(json);
    var live = new Set();
    for (var i = 0; i < requests.length; i++) live.add(requests[i].key);
    rendered.forEach(function (_v, k) { if (!live.has(k)) rendered.delete(k); });
  };
  __lumen_C.__lumen_paint_flush = function () {
    for (var i = 0; i < requests.length; i++) renderOne(requests[i]);
  };
})(globalThis)
