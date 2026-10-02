//! SVG DOM API stubs (W3C SVG 2 §3, §10, §11)
//! Phase 0: SVGElement/SVGSVGElement class hierarchy, getBBox() → DOMRect(zeros),
//! document.createElementNS('http://www.w3.org/2000/svg', ...) patched to return
//! typed SVG element instances. SVGRect/SVGPoint/SVGLength/SVGAnimatedLength types.

/// Install SVG DOM API bindings into a V8 runtime (Ph3 V8 migration S5-S7;
/// the rquickjs twin was removed in S12b-B25).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_svg_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SVG_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SVG_SHIM: &str = concat!(r#"
(function() {
  'use strict';

  const SVG_NS = 'http://www.w3.org/2000/svg';

  // ── Value types ──────────────────────────────────────────────────────────

  // ── WebIDL slots (BUG-1093) ──────────────────────────────────────────────
  // Interface state lives in a WeakMap slot, never in own properties of the
  // instance: every IDL attribute is an accessor on the interface PROTOTYPE
  // (WebIDL §3.7.6) that brand-checks `this` through `_slot`, so reading it
  // off the prototype itself throws `TypeError` and no state leaks as
  // enumerable/writable own fields.
  const _slots = new WeakMap();
  // Interfaces without a WebIDL constructor throw when a page calls `new` on
  // them; the shim itself builds instances through `_new`, which opens the gate.
  var _ctor_gate = 0;
  function _gate() { if (!_ctor_gate) throw new TypeError('Illegal constructor'); }
  function _new(Cls) {
    _ctor_gate++;
    try { return Reflect.construct(Cls, Array.prototype.slice.call(arguments, 1)); }
    finally { _ctor_gate--; }
  }
  function _rect(x, y, w, h) { return new SVGRect(x, y, w, h); }
  function _pt(x, y) { return new SVGPoint(x, y); }
  function _mat(a, b, c, d, e, f) { return new SVGMatrix(a, b, c, d, e, f); }
  function _fn_name(f, n) {
    if (typeof f === 'function') Object.defineProperty(f, 'name', { value: n, configurable: true });
    return f;
  }
  function _slot(o) {
    var s = (o !== null && typeof o === 'object') ? _slots.get(o) : undefined;
    if (!s) throw new TypeError('Illegal invocation');
    return s;
  }
  function _mk(Cls, state) {
    var o = Object.create(Cls.prototype);
    _slots.set(o, state);
    return o;
  }
  function _def(Cls, name, get, set) {
    Object.defineProperty(Cls.prototype, name, {
      get: _fn_name(function() { return get(_slot(this), this); }, 'get ' + name),
      set: set ? _fn_name(function(v) { set(_slot(this), v, this); }, 'set ' + name) : undefined,
      enumerable: true, configurable: true,
    });
  }
  // `baseVal`/`animVal` pair of an SVGAnimated* wrapper over object values.
  function _def_pair(Cls) {
    _def(Cls, 'baseVal', function(s) { return s.baseVal; });
    _def(Cls, 'animVal', function(s) { return s.animVal; });
  }
  function _pair(o, base, anim) { _gate(); _slots.set(o, { baseVal: base, animVal: anim }); }

  // Scalar SVGAnimated{String,Boolean,Enumeration,Integer,Number}: `baseVal`
  // reads/writes through `get`/`set`, `animVal` through the optional `anim`.
  const _str = function(v) { return String(v); };
  const _bool = function(v) { return !!v; };
  const _int = function(v) { return (+v) | 0; };
  const _num = function(v) {
    v = +v;
    if (!isFinite(v)) throw new TypeError('The provided float value is non-finite');
    return v;
  };
  function _init_scalar(o, coerce, dflt, v) {
    _gate();
    var c = coerce(v === undefined ? dflt : v);
    _slots.set(o, { get: function() { return c; }, set: function(x) { c = x; }, anim: null });
  }
  function _def_scalar(Cls, coerce) {
    _def(Cls, 'baseVal', function(s) { return s.get(); }, function(s, v) { s.set(coerce(v)); });
    _def(Cls, 'animVal', function(s) { return s.anim ? s.anim() : s.get(); });
  }
  function _mk_scalar(Cls, get, set, anim) {
    return _mk(Cls, { get: get, set: set, anim: anim || null });
  }

  // SVGRect / SVGPoint / SVGMatrix — legacy SVG 1.1 value types. They extend the
  // geometry interfaces (`DOMRect`/`DOMPoint`/`DOMMatrix`, the SVG 2 types of
  // `getBBox()`, `createSVGPoint()`, `SVGTransform.matrix`, ...) so one object is
  // both, which keeps `instanceof SVGRect` working next to the IDL's DOMRect.
  const _DR = typeof DOMRect === 'function' ? DOMRect : Object;
  const _DP = typeof DOMPoint === 'function' ? DOMPoint : Object;
  const _DM = typeof DOMMatrix === 'function' ? DOMMatrix : Object;
  class SVGRect extends _DR {
    constructor(x, y, w, h) {
      super(x || 0, y || 0, w || 0, h || 0);
      if (_DR === Object) { this.x = x || 0; this.y = y || 0; this.width = w || 0; this.height = h || 0; }
    }
  }
  window.SVGRect = SVGRect;

  // SVGPoint — 2-D point; matrixTransform() returns a new SVGPoint
  class SVGPoint extends _DP {
    constructor(x, y) {
      super(x || 0, y || 0, 0, 1);
      if (_DP === Object) { this.x = x || 0; this.y = y || 0; }
    }
    matrixTransform(matrix) {
      const m = matrix || {};
      return new SVGPoint(
        (m.a || 1) * this.x + (m.c || 0) * this.y + (m.e || 0),
        (m.b || 0) * this.x + (m.d || 1) * this.y + (m.f || 0)
      );
    }
  }
  window.SVGPoint = SVGPoint;

  // SVGLength (SVG 2 §5.4) — state is `{u, v}` (unit type, value in those
  // units) or, for a length reflected from a content attribute, an `io`
  // `{get, set, dflt}` triple re-read/re-written on every access.
  const _LEN_SUFFIX = ['', '', '%', 'em', 'ex', 'px', 'cm', 'mm', 'in', 'pt', 'pc'];
  const _LEN_PX = [0, 1, 0, 0, 0, 1, 96 / 2.54, 96 / 25.4, 96, 4 / 3, 16];
  function _len_parse(str) {
    var m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)(%|em|ex|px|cm|mm|in|pt|pc)?\s*$/.exec(String(str));
    if (!m) return null;
    return { u: m[2] ? _LEN_SUFFIX.indexOf(m[2]) : 1, v: parseFloat(m[1]) };
  }
  function _len_cur(s) {
    if (s.io) {
      var raw = s.io.get();
      var p = raw == null ? null : _len_parse(raw);
      return p || { u: 1, v: s.io.dflt };
    }
    return s;
  }
  function _len_put(s, u, v) {
    if (s.io) s.io.set(String(v) + _LEN_SUFFIX[u]);
    else { s.u = u; s.v = v; }
  }
  function _len_px(c) { return _LEN_PX[c.u] ? c.v * _LEN_PX[c.u] : c.v; }
  function _len_unit(u) {
    u = +u;
    if (!(u >= 1 && u <= 10)) throw new DOMException('Invalid unit type', 'NotSupportedError');
    return u;
  }
  class SVGLength {
    constructor() { _gate(); _slots.set(this, { u: 1, v: +arguments[0] || 0, io: null }); }
    newValueSpecifiedUnits(unitType, valueInSpecifiedUnits) {
      var s = _slot(this);
      _len_put(s, _len_unit(unitType), _num(valueInSpecifiedUnits));
    }
    convertToSpecifiedUnits(unitType) {
      var s = _slot(this), u = _len_unit(unitType);
      var c = _len_cur(s);
      // Relative units (%, em, ex) need layout context the shim does not have:
      // the unit changes and the number is kept, as before BUG-1093.
      if (!_LEN_PX[u] || (c.u !== 1 && !_LEN_PX[c.u])) _len_put(s, u, c.v);
      else _len_put(s, u, _len_px(c) / _LEN_PX[u]);
    }
  }
  _def(SVGLength, 'unitType', function(s) { return _len_cur(s).u; });
  _def(SVGLength, 'value', function(s) { return _len_px(_len_cur(s)); }, function(s, v) {
    v = _num(v);
    var c = _len_cur(s);
    if (_LEN_PX[c.u]) _len_put(s, c.u, v / _LEN_PX[c.u]);
    else _len_put(s, 1, v);
  });
  _def(SVGLength, 'valueInSpecifiedUnits', function(s) { return _len_cur(s).v; }, function(s, v) {
    _len_put(s, _len_cur(s).u, _num(v));
  });
  _def(SVGLength, 'valueAsString', function(s) {
    var c = _len_cur(s);
    return String(c.v) + _LEN_SUFFIX[c.u];
  }, function(s, v) {
    var p = _len_parse(v);
    if (!p) throw new DOMException('Invalid length', 'SyntaxError');
    _len_put(s, p.u, p.v);
  });
  // Unit type constants (W3C SVG §5.4.1)
  SVGLength.SVG_LENGTHTYPE_UNKNOWN    = 0;
  SVGLength.SVG_LENGTHTYPE_NUMBER     = 1;
  SVGLength.SVG_LENGTHTYPE_PERCENTAGE = 2;
  SVGLength.SVG_LENGTHTYPE_EMS        = 3;
  SVGLength.SVG_LENGTHTYPE_EXS        = 4;
  SVGLength.SVG_LENGTHTYPE_PX         = 5;
  SVGLength.SVG_LENGTHTYPE_CM         = 6;
  SVGLength.SVG_LENGTHTYPE_MM         = 7;
  SVGLength.SVG_LENGTHTYPE_IN         = 8;
  SVGLength.SVG_LENGTHTYPE_PT         = 9;
  SVGLength.SVG_LENGTHTYPE_PC         = 10;
  window.SVGLength = SVGLength;

  // SVGAnimatedLength — pair of base/animated SVGLength values
  class SVGAnimatedLength {
    constructor() { _pair(this, new SVGLength(arguments[0]), new SVGLength(arguments[0])); }
  }
  _def_pair(SVGAnimatedLength);
  window.SVGAnimatedLength = SVGAnimatedLength;

  // SVGAnimatedString — pair of base/animated string values
  class SVGAnimatedString {
    constructor() { _init_scalar(this, _str, '', arguments[0]); }
  }
  _def_scalar(SVGAnimatedString, _str);
  window.SVGAnimatedString = SVGAnimatedString;

  // Shared list state: `{items}` in a slot, operations on each interface's own
  // prototype (`_def_list`) — WebIDL puts them on every interface, not on a
  // common base. `check` validates/coerces an incoming item.
  function _list_index(i, len) {
    i = i >>> 0;
    if (i >= len) throw new DOMException('Index out of range', 'IndexSizeError');
    return i;
  }
  function _def_list(Cls, check, extra) {
    var P = Cls.prototype;
    function op(name, fn) {
      Object.defineProperty(P, name, { value: fn, writable: true, enumerable: true, configurable: true });
    }
    _def(Cls, 'length', function(s) { return s.items.length; });
    _def(Cls, 'numberOfItems', function(s) { return s.items.length; });
    op('clear', function() { _slot(this).items.length = 0; });
    op('initialize', function(x) {
      var s = _slot(this); x = check(x); s.items.length = 0; s.items.push(x); return x;
    });
    op('getItem', function(i) { var s = _slot(this); return s.items[_list_index(i, s.items.length)]; });
    op('insertItemBefore', function(x, i) {
      var s = _slot(this); x = check(x); i = i >>> 0;
      s.items.splice(Math.min(i, s.items.length), 0, x); return x;
    });
    op('replaceItem', function(x, i) {
      var s = _slot(this); x = check(x);
      s.items[_list_index(i, s.items.length)] = x; return x;
    });
    op('removeItem', function(i) {
      var s = _slot(this);
      return s.items.splice(_list_index(i, s.items.length), 1)[0];
    });
    op('appendItem', function(x) { var s = _slot(this); x = check(x); s.items.push(x); return x; });
    if (extra) extra(op);
  }
  function _init_list(o) { _gate(); _slots.set(o, { items: [] }); }

  // SVGStringList — ordered list of strings
  class SVGStringList {
    constructor() { _init_list(this); }
  }
  _def_list(SVGStringList, _str);
  window.SVGStringList = SVGStringList;

  // SVGAnimatedBoolean
  class SVGAnimatedBoolean {
    constructor() { _init_scalar(this, _bool, false, arguments[0]); }
  }
  _def_scalar(SVGAnimatedBoolean, _bool);
  window.SVGAnimatedBoolean = SVGAnimatedBoolean;

  // SVGAnimatedEnumeration
  class SVGAnimatedEnumeration {
    constructor() { _init_scalar(this, _int, 0, arguments[0]); }
  }
  _def_scalar(SVGAnimatedEnumeration, _int);
  window.SVGAnimatedEnumeration = SVGAnimatedEnumeration;

  // SVGAnimatedInteger
  class SVGAnimatedInteger {
    constructor() { _init_scalar(this, _int, 0, arguments[0]); }
  }
  _def_scalar(SVGAnimatedInteger, _int);
  window.SVGAnimatedInteger = SVGAnimatedInteger;

  // SVGAnimatedNumber
  class SVGAnimatedNumber {
    constructor() { _init_scalar(this, _num, 0, arguments[0]); }
  }
  _def_scalar(SVGAnimatedNumber, _num);
  window.SVGAnimatedNumber = SVGAnimatedNumber;

  // SVGAnimatedRect — pair of base/animated SVGRect values
  class SVGAnimatedRect {
    constructor() { _pair(this, _rect(), _rect()); }
  }
  _def_pair(SVGAnimatedRect);
  window.SVGAnimatedRect = SVGAnimatedRect;

  // SVGMatrix (legacy, before DOMMatrix) — 2-D affine transform [a b c d e f]
  class SVGMatrix extends _DM {
    constructor(a,b,c,d,e,f) {
      super([a!=null?a:1, b!=null?b:0, c!=null?c:0, d!=null?d:1, e!=null?e:0, f!=null?f:0]);
      if (_DM === Object) {
        this.a = a!=null?a:1; this.b = b!=null?b:0;
        this.c = c!=null?c:0; this.d = d!=null?d:1;
        this.e = e!=null?e:0; this.f = f!=null?f:0;
      }
    }
    multiply(m) {
      return new SVGMatrix(
        this.a*m.a+this.c*m.b, this.b*m.a+this.d*m.b,
        this.a*m.c+this.c*m.d, this.b*m.c+this.d*m.d,
        this.a*m.e+this.c*m.f+this.e, this.b*m.e+this.d*m.f+this.f
      );
    }
    translate(x,y) { return new SVGMatrix(this.a,this.b,this.c,this.d,this.e+x,this.f+y); }
    scale(s) { return new SVGMatrix(this.a*s,this.b*s,this.c*s,this.d*s,this.e,this.f); }
    scaleNonUniform(sx,sy) { return new SVGMatrix(this.a*sx,this.b*sx,this.c*sy,this.d*sy,this.e,this.f); }
    rotate(a) {
      const r=a*Math.PI/180, cos=Math.cos(r), sin=Math.sin(r);
      return this.multiply(new SVGMatrix(cos,sin,-sin,cos,0,0));
    }
    rotateFromVector(x,y) { return this.rotate(Math.atan2(y,x)*180/Math.PI); }
    flipX() { return this.multiply(new SVGMatrix(-1,0,0,1,0,0)); }
    flipY() { return this.multiply(new SVGMatrix(1,0,0,-1,0,0)); }
    skewX(a) { return this.multiply(new SVGMatrix(1,0,Math.tan(a*Math.PI/180),1,0,0)); }
    skewY(a) { return this.multiply(new SVGMatrix(1,Math.tan(a*Math.PI/180),0,1,0,0)); }
  }
  window.SVGMatrix = SVGMatrix;

  // SVGTransform — single transform component. `type`/`matrix`/`angle` are
  // read-only attributes (SVG 2 §5.13): only the set*() operations (and the
  // transform-list parser, via `_lumen_svg_transform_set`) change them.
  class SVGTransform {
    constructor() { _gate(); _slots.set(this, { type: 1, matrix: _mat(), angle: 0 }); }
    setMatrix(m) {
      var s = _slot(this), mm = m || {};
      s.type = 1; s.angle = 0;
      s.matrix = _mat(mm.a, mm.b, mm.c, mm.d, mm.e, mm.f);
    }
    setTranslate(tx, ty) {
      var s = _slot(this);
      s.type = 2; s.angle = 0;
      s.matrix = _mat(1, 0, 0, 1, _num(tx), _num(ty));
    }
    setScale(sx, sy) {
      var s = _slot(this);
      s.type = 3; s.angle = 0;
      s.matrix = _mat(_num(sx), 0, 0, _num(sy), 0, 0);
    }
    setRotate(a, cx, cy) {
      var s = _slot(this);
      a = _num(a);
      s.type = 4; s.angle = a;
      const r = a * Math.PI / 180, cos = Math.cos(r), sin = Math.sin(r);
      cx = +cx || 0; cy = +cy || 0;
      s.matrix = _mat(cos, sin, -sin, cos,
        (1 - cos) * cx + sin * cy, (1 - cos) * cy - sin * cx);
    }
    setSkewX(a) {
      var s = _slot(this);
      a = _num(a);
      s.type = 5; s.angle = a;
      s.matrix = _mat(1, 0, Math.tan(a * Math.PI / 180), 1, 0, 0);
    }
    setSkewY(a) {
      var s = _slot(this);
      a = _num(a);
      s.type = 6; s.angle = a;
      s.matrix = _mat(1, Math.tan(a * Math.PI / 180), 0, 1, 0, 0);
    }
  }
  _def(SVGTransform, 'type', function(s) { return s.type; });
  _def(SVGTransform, 'matrix', function(s) { return s.matrix; });
  _def(SVGTransform, 'angle', function(s) { return s.angle; });
  // Internal setter for the transform-list parser / consolidate().
  function _lumen_svg_transform_set(t, type, matrix, angle) {
    var s = _slot(t);
    s.type = type; s.matrix = matrix; s.angle = angle || 0;
  }
  SVGTransform.SVG_TRANSFORM_UNKNOWN   = 0;
  SVGTransform.SVG_TRANSFORM_MATRIX    = 1;
  SVGTransform.SVG_TRANSFORM_TRANSLATE = 2;
  SVGTransform.SVG_TRANSFORM_SCALE     = 3;
  SVGTransform.SVG_TRANSFORM_ROTATE    = 4;
  SVGTransform.SVG_TRANSFORM_SKEWX     = 5;
  SVGTransform.SVG_TRANSFORM_SKEWY     = 6;
  window.SVGTransform = SVGTransform;

  // SVGTransformList — ordered list of SVGTransform
  class SVGTransformList {
    constructor() { _init_list(this); }
  }
  _def_list(SVGTransformList, function(t) {
    _slot(t);
    return t;
  }, function(op) {
    op('createSVGTransformFromMatrix', function(m) {
      const t = _new(SVGTransform); t.setMatrix(m); return t;
    });
    op('consolidate', function() {
      var s = _slot(this);
      if (s.items.length === 0) return null;
      const t = _new(SVGTransform);
      _lumen_svg_transform_set(t, 1,
        s.items.reduce(function(acc, x) { return acc.multiply(_slot(x).matrix); }, _mat()), 0);
      s.items.length = 0; s.items.push(t);
      return t;
    });
  });
  window.SVGTransformList = SVGTransformList;

  // SVGAnimatedTransformList
  class SVGAnimatedTransformList {
    constructor() { _pair(this, new SVGTransformList(), new SVGTransformList()); }
  }
  _def_pair(SVGAnimatedTransformList);
  window.SVGAnimatedTransformList = SVGAnimatedTransformList;

  // SVGPointList
  class SVGPointList {
    constructor() { _init_list(this); }
  }
  _def_list(SVGPointList, function(p) {
    if (p === null || typeof p !== 'object') throw new TypeError('Not a point');
    return p;
  });
  window.SVGPointList = SVGPointList;

  // SVGNumber (SVG 2 §5.8) — `value` is a coerced double.
  class SVGNumber {
    constructor() { _gate(); this._v = +arguments[0] || 0; }
    get value() { return this._v; }
    set value(v) { this._v = +v; }
  }
  window.SVGNumber = SVGNumber;

  // SVGAngle (SVG 2 §5.10). Units: 1 unspecified (= deg), 2 deg, 3 rad, 4 grad.
  const _LUMEN_ANGLE_TO_DEG = [0, 1, 1, 180 / Math.PI, 0.9];
  const _LUMEN_ANGLE_SUFFIX = ['', '', 'deg', 'rad', 'grad'];
  class SVGAngle {
    constructor() { _gate(); this._unit = 1; this._value = 0; }
    get unitType() { return this._unit; }
    get value() { return this._value * _LUMEN_ANGLE_TO_DEG[this._unit]; }
    set value(v) { this._value = +v / _LUMEN_ANGLE_TO_DEG[this._unit]; }
    get valueInSpecifiedUnits() { return this._value; }
    set valueInSpecifiedUnits(v) { this._value = +v; }
    get valueAsString() { return String(this._value) + _LUMEN_ANGLE_SUFFIX[this._unit]; }
    set valueAsString(s) {
      var m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)(deg|grad|rad)?\s*$/.exec(String(s));
      if (!m) throw new DOMException('Invalid angle', 'SyntaxError');
      this._value = parseFloat(m[1]);
      this._unit = m[2] === 'deg' ? 2 : m[2] === 'rad' ? 3 : m[2] === 'grad' ? 4 : 1;
    }
    newValueSpecifiedUnits(unitType, v) {
      if (unitType < 1 || unitType > 4) throw new DOMException('Invalid unit type', 'NotSupportedError');
      this._unit = unitType; this._value = +v;
    }
    convertToSpecifiedUnits(unitType) {
      if (unitType < 1 || unitType > 4) throw new DOMException('Invalid unit type', 'NotSupportedError');
      var deg = this.value; this._unit = unitType; this.value = deg;
    }
  }
  SVGAngle.SVG_ANGLETYPE_UNKNOWN     = 0;
  SVGAngle.SVG_ANGLETYPE_UNSPECIFIED = 1;
  SVGAngle.SVG_ANGLETYPE_DEG         = 2;
  SVGAngle.SVG_ANGLETYPE_RAD         = 3;
  SVGAngle.SVG_ANGLETYPE_GRAD        = 4;
  window.SVGAngle = SVGAngle;

  class SVGNumberList {
    constructor() { _init_list(this); }
  }
  _def_list(SVGNumberList, function(x) {
    if (!(x instanceof SVGNumber)) throw new TypeError('Not an SVGNumber');
    return x;
  });
  window.SVGNumberList = SVGNumberList;
  class SVGLengthList {
    constructor() { _init_list(this); }
  }
  _def_list(SVGLengthList, function(x) {
    if (!(x instanceof SVGLength)) throw new TypeError('Not an SVGLength');
    return x;
  });
  window.SVGLengthList = SVGLengthList;

  class SVGAnimatedAngle {
    constructor() { _pair(this, new SVGAngle(), new SVGAngle()); }
  }
  _def_pair(SVGAnimatedAngle);
  window.SVGAnimatedAngle = SVGAnimatedAngle;
  class SVGAnimatedNumberList {
    constructor() { _pair(this, new SVGNumberList(), new SVGNumberList()); }
  }
  _def_pair(SVGAnimatedNumberList);
  window.SVGAnimatedNumberList = SVGAnimatedNumberList;
  class SVGAnimatedLengthList {
    constructor() { _pair(this, new SVGLengthList(), new SVGLengthList()); }
  }
  _def_pair(SVGAnimatedLengthList);
  window.SVGAnimatedLengthList = SVGAnimatedLengthList;

  // SVGUnitTypes (SVG 2 §5.11) — constants-only interface.
  class SVGUnitTypes { constructor() { _gate(); } }
  SVGUnitTypes.SVG_UNIT_TYPE_UNKNOWN           = 0;
  SVGUnitTypes.SVG_UNIT_TYPE_USERSPACEONUSE    = 1;
  SVGUnitTypes.SVG_UNIT_TYPE_OBJECTBOUNDINGBOX = 2;
  window.SVGUnitTypes = SVGUnitTypes;

  // ── Attribute reflection (GAP-SVGDOM) ───────────────────────────────────
  // A real SVG element (parser-built or from `createElementNS`) never runs
  // an ES class constructor — `_lumen_build_element` (web_api_shim_mid.js)
  // makes it with `Object.create(prototype)`, only re-pointing at the typed
  // `SVG*Element` prototype (BUG-889). Every field the constructors below
  // used to set (`this.x = _new(SVGAnimatedLength, 0)`, …) was therefore dead
  // for every element a page can actually touch — only the synthetic
  // `new SVGRectElement()` in this file's own tests ran it. The helpers here
  // replace those fields with PROTOTYPE accessors that read/write the live
  // content attribute on every access, so `rect.width.baseVal.value` reflects
  // `rect.getAttribute('width')` both ways, same as a real browser.

  // Parses a bare number, ignoring any unit suffix (`10px`, `50%`, `10lh`) —
  // Phase 1 resolves only the numeric magnitude; unit-aware resolution
  // (viewport/font-relative units) is out of scope here, tracked by the
  // WPT `SVGLength-*` cases GAP-SVGDOM's ROADMAP note lists as known-open.
  function _lumen_svg_parse_number(str, dflt) {
    if (str == null) return dflt;
    var n = parseFloat(str);
    return isNaN(n) ? dflt : n;
  }

  function _lumen_svg_length_value(nid, attr, dflt) {
    if (nid == null) return dflt;
    return _lumen_svg_parse_number(_lumen_u2n(_lumen_get_attr(nid, attr)), dflt);
  }

  // A live SVGLength whose `value`/`valueInSpecifiedUnits`/`valueAsString`
  // re-read the attribute on every get and write it back on every set.
  function _lumen_svg_reflected_length(nid, attr, dflt) {
    var length = _new(SVGLength, 0);
    _slot(length).io = {
      dflt: dflt,
      get: function() { return nid != null ? _lumen_u2n(_lumen_get_attr(nid, attr)) : null; },
      set: function(v) { if (nid != null) _lumen_set_attr(nid, attr, v); },
    };
    return length;
  }

  function _lumen_svg_animated_length(nid, attr, dflt) {
    var base = _lumen_svg_reflected_length(nid, attr, dflt);
    // GAP-SMIL: `animVal` is `baseVal` unless a running `<animate>`/`<set>`
    // targeting this exact attribute has a value queued in the SMIL override
    // map (`_lumen_smil_overrides`, populated by `_lumen_tick_smil`) — the
    // override never touches the content attribute, so `getAttribute`/
    // `baseVal` stay unaffected, matching the animVal/baseVal split SVG 2 §3
    // requires. No override map yet (SMIL never ticked) reads as `undefined`.
    return _mk(SVGAnimatedLength, {
      baseVal: base,
      get animVal() {
        var ov = (typeof _lumen_smil_overrides !== 'undefined')
          ? _lumen_smil_overrides[nid + '|' + attr] : undefined;
        // A non-numeric animated value is not a length: animation has no effect.
        if (ov !== undefined && !isNaN(parseFloat(ov))) return _new(SVGLength, _lumen_svg_parse_number(ov, dflt));
        return base;
      },
    });
  }

  // Defines a live SVGAnimatedLength getter for each `prop -> [attr, default]`
  // entry in `specs` on `Ctor.prototype`.
  function _lumen_def_svg_lengths(Ctor, specs) {
    Object.keys(specs).forEach(function(prop) {
      var attr = specs[prop][0];
      var dflt = specs[prop][1];
      Object.defineProperty(Ctor.prototype, prop, {
        get: function() { return _lumen_svg_animated_length(this.__nid__, attr, dflt); },
        enumerable: true, configurable: true,
      });
    });
  }

  function _lumen_svg_parse_viewbox(nid) {
    var s = nid != null ? _lumen_u2n(_lumen_get_attr(nid, 'viewBox')) : null;
    if (!s) return [0, 0, 0, 0];
    var parts = s.trim().split(/[\s,]+/).map(function(p) { return parseFloat(p); });
    return [parts[0] || 0, parts[1] || 0, parts[2] || 0, parts[3] || 0];
  }

  function _lumen_svg_animated_rect_viewbox(nid) {
    var v = _lumen_svg_parse_viewbox(nid);
    var rect = _rect(v[0], v[1], v[2], v[3]);
    return _mk(SVGAnimatedRect, { baseVal: rect, animVal: rect });
  }

  function _lumen_def_svg_viewbox(Ctor) {
    Object.defineProperty(Ctor.prototype, 'viewBox', {
      get: function() { return _lumen_svg_animated_rect_viewbox(this.__nid__); },
      enumerable: true, configurable: true,
    });
  }

  // Parses `transform`/`gradientTransform`/`patternTransform` (SVG L1 §7.6
  // `<transform-list>`) into an `SVGTransformList`. Same function-name set the
  // layout side's `parse_svg_transform` (`box_tree/svg.rs`) reads for paint —
  // this is a JS-side twin kept independent since the two never share state.
  function _lumen_svg_parse_transform_list(nid, attr) {
    var list = _new(SVGTransformList);
    var s = nid != null ? _lumen_u2n(_lumen_get_attr(nid, attr)) : null;
    if (!s) return list;
    var re = /(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^)]*)\)/g;
    var m;
    while ((m = re.exec(s))) {
      var fn = m[1];
      var args = m[2].trim().split(/[\s,]+/).filter(function(x) { return x !== ''; })
        .map(function(x) { return parseFloat(x); });
      var t = _new(SVGTransform);
      if (fn === 'matrix' && args.length === 6) {
        _lumen_svg_transform_set(t, SVGTransform.SVG_TRANSFORM_MATRIX,
          _mat(args[0], args[1], args[2], args[3], args[4], args[5]), 0);
      } else if (fn === 'translate') {
        t.setTranslate(args[0] || 0, args[1] || 0);
      } else if (fn === 'scale') {
        var sx = args[0] != null ? args[0] : 1;
        var sy = args.length > 1 ? args[1] : sx;
        _lumen_svg_transform_set(t, SVGTransform.SVG_TRANSFORM_SCALE, _mat(sx, 0, 0, sy, 0, 0), 0);
      } else if (fn === 'rotate') {
        t.setRotate(args[0] || 0, args[1], args[2]);
      } else if (fn === 'skewX') {
        _lumen_svg_transform_set(t, SVGTransform.SVG_TRANSFORM_SKEWX,
          _mat(1, 0, Math.tan((args[0] || 0) * Math.PI / 180), 1, 0, 0), args[0] || 0);
      } else if (fn === 'skewY') {
        _lumen_svg_transform_set(t, SVGTransform.SVG_TRANSFORM_SKEWY,
          _mat(1, Math.tan((args[0] || 0) * Math.PI / 180), 0, 1, 0, 0), args[0] || 0);
      } else {
        continue;
      }
      list.appendItem(t);
    }
    return list;
  }

  function _lumen_svg_animated_transform_list(nid, attr) {
    var list = _lumen_svg_parse_transform_list(nid, attr);
    return _mk(SVGAnimatedTransformList, { baseVal: list, animVal: list });
  }

  function _lumen_def_svg_transform(Ctor, prop, attr) {
    Object.defineProperty(Ctor.prototype, prop, {
      get: function() { return _lumen_svg_animated_transform_list(this.__nid__, attr); },
      enumerable: true, configurable: true,
    });
  }

  // `points` (SVG L1 §9.7.1 `<list-of-points>`): "x1,y1 x2,y2 …".
  function _lumen_svg_parse_points(nid) {
    var list = _new(SVGPointList);
    var s = nid != null ? _lumen_u2n(_lumen_get_attr(nid, 'points')) : null;
    if (!s) return list;
    var nums = s.trim().split(/[\s,]+/).filter(function(x) { return x !== ''; })
      .map(function(x) { return parseFloat(x); });
    for (var i = 0; i + 1 < nums.length; i += 2) {
      list.appendItem(_pt(nums[i], nums[i + 1]));
    }
    return list;
  }

  function _lumen_def_svg_points(Ctor) {
    Object.defineProperty(Ctor.prototype, 'points', {
      get: function() { return _lumen_svg_parse_points(this.__nid__); },
      enumerable: true, configurable: true,
    });
    Object.defineProperty(Ctor.prototype, 'animatedPoints', {
      get: function() { return _lumen_svg_parse_points(this.__nid__); },
      enumerable: true, configurable: true,
    });
  }

  // ── Attribute-reflection kit (BUG-1093) ─────────────────────────────────
  // Live SVGAnimated*/list objects over one content attribute of element `el`
  // (reached through its native node id `__nid__`). The WebIDL shape pass at
  // the end of this file installs them as prototype accessors for every IDL
  // attribute no hand-written definition above covers.
  function _attr_get(el, name) {
    var n = el.__nid__;
    return n == null ? null : _lumen_u2n(_lumen_get_attr(n, name));
  }
  function _attr_set(el, name, v) {
    var n = el.__nid__;
    if (n != null) _lumen_set_attr(n, name, String(v));
  }
  function _attr_remove(el, name) {
    var n = el.__nid__;
    if (n != null) _lumen_remove_attr(n, name);
  }
  var _XLINK_NS = 'http://www.w3.org/1999/xlink';
  function _href_get(el) {
    var v = _attr_get(el, 'href');
    if (v === null && typeof el.getAttributeNS === 'function') v = el.getAttributeNS(_XLINK_NS, 'href');
    return v === null || v === undefined ? '' : v;
  }

  function _anim_string(el, attr) {
    var get = attr === 'href' ? function() { return _href_get(el); }
                              : function() { var v = _attr_get(el, attr); return v === null ? '' : v; };
    return _mk_scalar(SVGAnimatedString, get, function(v) { _attr_set(el, attr, v); });
  }
  function _anim_enum(el, attr, keys, dflt) {
    return _mk_scalar(SVGAnimatedEnumeration, function() {
      var i = keys.indexOf(_attr_get(el, attr));
      return i < 0 ? dflt : i + 1;
    }, function(v) {
      if (v < 1 || v > keys.length) throw new TypeError('The enumeration value is out of range');
      _attr_set(el, attr, keys[v - 1]);
    });
  }
  function _anim_bool(el, attr, dflt) {
    return _mk_scalar(SVGAnimatedBoolean, function() {
      var v = _attr_get(el, attr);
      return v === null ? dflt : v === 'true';
    }, function(v) { _attr_set(el, attr, v ? 'true' : 'false'); });
  }
  function _anim_int(el, attr, dflt) {
    return _mk_scalar(SVGAnimatedInteger, function() {
      var n = parseInt(_attr_get(el, attr), 10);
      return isNaN(n) ? dflt : n;
    }, function(v) { _attr_set(el, attr, v); });
  }
  function _anim_number(el, attr, dflt) {
    return _mk_scalar(SVGAnimatedNumber, function() {
      var raw = _attr_get(el, attr);
      var n = parseFloat(raw);
      if (isNaN(n)) return dflt;
      return raw.trim().slice(-1) === '%' ? n / 100 : n;
    }, function(v) { _attr_set(el, attr, v); });
  }
  function _split_list(raw) {
    return raw == null ? [] : String(raw).trim().split(/[\s,]+/).filter(function(x) { return x !== ''; });
  }
  function _anim_number_list(el, attr) {
    var list = _new(SVGNumberList);
    _split_list(_attr_get(el, attr)).forEach(function(t) {
      var n = parseFloat(t);
      if (!isNaN(n)) _slot(list).items.push(_new(SVGNumber, n));
    });
    return _mk(SVGAnimatedNumberList, { baseVal: list, animVal: list });
  }
  function _anim_length_list(el, attr) {
    var list = _new(SVGLengthList);
    _split_list(_attr_get(el, attr)).forEach(function(t) {
      var p = _len_parse(t);
      if (!p) return;
      var l = _new(SVGLength, 0);
      _slot(l).u = p.u; _slot(l).v = p.v;
      _slot(list).items.push(l);
    });
    return _mk(SVGAnimatedLengthList, { baseVal: list, animVal: list });
  }
  // `orient` (SVG 2 §11.10): "auto" | "auto-start-reverse" | <angle>.
  function _marker_orient_type(el) {
    var o = _attr_get(el, 'orient');
    return o === 'auto' ? 1 : o === 'auto-start-reverse' ? 3 : 2;
  }
  function _anim_orient_type(el) {
    return _mk_scalar(SVGAnimatedEnumeration, function() { return _marker_orient_type(el); },
      function(v) {
        if (v === 1) _attr_set(el, 'orient', 'auto');
        else if (v === 2) _attr_set(el, 'orient', '0');
        else if (v === 3) _attr_set(el, 'orient', 'auto-start-reverse');
        else throw new TypeError('The enumeration value is out of range');
      });
  }
  function _anim_orient_angle(el) {
    var a = _new(SVGAngle);
    if (_marker_orient_type(el) === 2) {
      try { a.valueAsString = _attr_get(el, 'orient') || '0'; } catch (e) { /* keep 0 */ }
    }
    return _mk(SVGAnimatedAngle, { baseVal: a, animVal: a });
  }
  // `requiredExtensions` (space-separated) / `systemLanguage` (comma-separated).
  function _string_list(el, attr) {
    var list = _new(SVGStringList);
    var raw = _attr_get(el, attr);
    if (raw !== null) {
      var parts = attr === 'systemLanguage' ? raw.split(',') : raw.split(/\s+/);
      parts.forEach(function(p) { p = p.trim(); if (p) _slot(list).items.push(p); });
    }
    return list;
  }

  // Enumerated attributes: IDL name → [keyword list (1-based enum values), default].
  const _ENUM_ATTRS = {
    lengthAdjust: [['spacing', 'spacingAndGlyphs'], 1],
    method: [['align', 'stretch'], 1],
    spacing: [['auto', 'exact'], 1],
    markerUnits: [['userSpaceOnUse', 'strokeWidth'], 2],
    gradientUnits: [['userSpaceOnUse', 'objectBoundingBox'], 2],
    patternUnits: [['userSpaceOnUse', 'objectBoundingBox'], 2],
    patternContentUnits: [['userSpaceOnUse', 'objectBoundingBox'], 1],
    spreadMethod: [['pad', 'reflect', 'repeat'], 1],
  };
  const _CONTENT_ATTR = { className: 'class', crossOrigin: 'crossorigin', referrerPolicy: 'referrerpolicy' };
  const _LIVE_TYPES = {
    SVGAnimatedLength: 1, SVGAnimatedString: 1, SVGAnimatedEnumeration: 1, SVGAnimatedBoolean: 1,
    SVGAnimatedInteger: 1, SVGAnimatedNumber: 1, SVGAnimatedPreserveAspectRatio: 1,
  };
  const _same_object = new WeakMap();

  // Builds the object an IDL attribute of `type` returns for element `el`.
  // `opts` = `{dflt, keys}` overrides the per-name defaults.
  function _animated_for(type, name, el, opts) {
    var attr = _CONTENT_ATTR[name] || name;
    var dflt = opts && opts.dflt !== undefined ? opts.dflt : undefined;
    switch (type) {
      case 'SVGAnimatedLength':
        return _lumen_svg_animated_length(el.__nid__, attr,
          dflt !== undefined ? dflt : (name === 'markerWidth' || name === 'markerHeight') ? 3 : 0);
      case 'SVGAnimatedString': return _anim_string(el, attr);
      case 'SVGAnimatedEnumeration':
        if (name === 'orientType') return _anim_orient_type(el);
        if (opts && opts.keys) return _anim_enum(el, attr, opts.keys, dflt);
        var e = _ENUM_ATTRS[name] || [[], 0];
        return _anim_enum(el, attr, e[0], e[1]);
      case 'SVGAnimatedBoolean': return _anim_bool(el, attr, !!dflt);
      case 'SVGAnimatedInteger': return _anim_int(el, attr, dflt || 0);
      case 'SVGAnimatedNumber': return _anim_number(el, attr, dflt || 0);
      case 'SVGAnimatedNumberList': return _anim_number_list(el, attr);
      case 'SVGAnimatedLengthList': return _anim_length_list(el, attr);
      case 'SVGAnimatedAngle': return _anim_orient_angle(el);
      case 'SVGAnimatedTransformList': return _lumen_svg_animated_transform_list(el.__nid__, attr);
      case 'SVGAnimatedRect': return _lumen_svg_animated_rect_viewbox(el.__nid__);
      case 'SVGAnimatedPreserveAspectRatio': return _lumen_svg_animated_par(el.__nid__);
      case 'SVGStringList': return _string_list(el, attr);
      case 'SVGPointList': return _lumen_svg_parse_points(el.__nid__);
    }
    return undefined;
  }
  // Same object on every read (WebIDL [SameObject]). The live kinds read the
  // attribute on each access, so one object serves forever; the list kinds are
  // parsed snapshots, so the cached one is kept only while the content
  // attribute is unchanged and rebuilt (new identity) once it differs.
  const _RAW_ATTR = { orientAngle: 'orient', animatedPoints: 'points' };
  function _same_cached(el, name, type, make) {
    var m = _same_object.get(el);
    if (!m) { m = Object.create(null); _same_object.set(el, m); }
    var rec = m[name];
    if (_LIVE_TYPES[type]) return (rec || (m[name] = { obj: make() })).obj;
    var raw = _attr_get(el, _RAW_ATTR[name] || _CONTENT_ATTR[name] || name);
    if (rec && rec.raw === raw) return rec.obj;
    rec = m[name] = { raw: raw, obj: make() };
    return rec.obj;
  }
  function _cached_animated(type, name, el, opts) {
    return _same_cached(el, name, type, function() { return _animated_for(type, name, el, opts); });
  }

  // Installs live prototype accessors for `spec` entries
  // `[name, 'Integer'|'Number'|'Enumeration'|'Boolean'|'String'|'NumberList', default, keywords?]`.
  function _lumen_def_attrs(C, spec) {
    spec.forEach(function(s) {
      var name = s[0], type = 'SVGAnimated' + s[1], opts = { dflt: s[2], keys: s[3] };
      Object.defineProperty(C.prototype, name, {
        get: _fn_name(function() {
          if (!(this instanceof C) || this === C.prototype) throw new TypeError('Illegal invocation');
          return _cached_animated(type, name, this, opts);
        }, 'get ' + name),
        enumerable: true, configurable: true,
      });
    });
  }

  // ── Base element classes ──────────────────────────────────────────────────

  // SVGElement — base for all SVG elements (W3C SVG 2 §4.3)
  class SVGElement extends (typeof Element !== 'undefined' ? Element : Object) {
    constructor() {
      super();
      this.namespaceURI = SVG_NS;
      this.style = {};
      this.id = '';
    }
    // BUG-414: no `dataset` stub here. Real SVG elements come from the native
    // `createElementNS` and carry the shared wrapper's own live DOMStringMap
    // (`dom.rs`); a prototype stub returning a fresh `{}` only shadowed it for
    // hand-constructed instances and silently dropped writes.
    focus() {}
    blur() {}
  }
  // `className` is already reflected for every element (SVG included) by the
  // shared `Element` wrapper descriptor (SVG2 §4.3 folded `SVGElement`'s own
  // `SVGAnimatedString className` into plain `Element.className`) — no
  // override needed here.
  // `ownerSVGElement`/`viewportElement` (SVG2 §4.3): nearest ancestor that
  // establishes a viewport, found by walking the live `parentNode` chain —
  // a constructor field, unlike this, never sees an update after the element
  // moves in the tree.
  Object.defineProperty(SVGElement.prototype, 'ownerSVGElement', {
    get: function() {
      var p = this.parentNode;
      while (p) {
        if (p instanceof SVGSVGElement) return p;
        p = p.parentNode;
      }
      return null;
    },
    enumerable: true, configurable: true,
  });
  Object.defineProperty(SVGElement.prototype, 'viewportElement', {
    get: function() { return this.ownerSVGElement; },
    enumerable: true, configurable: true,
  });
  window.SVGElement = SVGElement;

  // SVGGraphicsElement — adds transform, getBBox, getScreenCTM, getCTM
  class SVGGraphicsElement extends SVGElement {
    // Generic fallback: geometry-specific subclasses (rect/circle/ellipse/
    // line/polyline/polygon below) override with their own shape formula.
    // `<path>`, `<text>`, `<g>`, … still get the zero rect — deriving a bbox
    // from path data or laid-out glyphs needs real geometry the JS shim does
    // not have access to; left for follow-up.
    getBBox(options) {
      return _rect(0, 0, 0, 0);
    }

    // Phase 0: returns identity matrix
    getCTM() { return _mat(); }
    getScreenCTM() { return _mat(); }

    getTransformToElement(element) { return _mat(); }
  }
  _lumen_def_svg_transform(SVGGraphicsElement, 'transform', 'transform');
  window.SVGGraphicsElement = SVGGraphicsElement;

  // SVGGeometryElement — adds pathLength, getTotalLength, getPointAtLength, isPointInFill/Stroke
  class SVGGeometryElement extends SVGGraphicsElement {
    constructor() {
      super();
    }
    getTotalLength() { return 0; }
    getPointAtLength(distance) { return _pt(0, 0); }
    isPointInFill(point) { return false; }
    isPointInStroke(point) { return false; }
  }
  window.SVGGeometryElement = SVGGeometryElement;

  // ── Concrete element classes ──────────────────────────────────────────────

  // SVGSVGElement — the root <svg> element (W3C SVG 2 §5.1)
  class SVGSVGElement extends SVGGraphicsElement {
    constructor() {
      super();
      this.tagName = 'svg';
      this.contentScriptType = 'text/ecmascript';
      this.contentStyleType = 'text/css';
    }

    createSVGRect()   { return _rect(); }
    createSVGPoint()  { return _pt(); }
    createSVGLength() { return _new(SVGLength); }
    createSVGMatrix() { return _mat(); }
    createSVGTransform() { return _new(SVGTransform); }
    createSVGTransformFromMatrix(m) {
      const t = _new(SVGTransform); t.setMatrix(m); return t;
    }
    createSVGNumber() { return _new(SVGNumber); }
    createSVGAngle()  { return _new(SVGAngle); }

    getElementById(id) { return null; }
    getIntersectionList(rect, referenceElement) { return []; }
    getEnclosureList(rect, referenceElement) { return []; }
    checkIntersection(element, rect) { return false; }
    checkEnclosure(element, rect) { return false; }
    deselectAll() {}
    suspendRedraw(maxWaitMilliseconds) { return 0; }
    unsuspendRedraw(suspendHandleID) {}
    unsuspendRedrawAll() {}
    forceRedraw() {}
    pauseAnimations() { _lumen_smil_paused = true; }
    unpauseAnimations() { _lumen_smil_paused = false; _lumen_smil_wake(); }
    animationsPaused() { return _lumen_smil_paused; }
    getCurrentTime() { return _lumen_smil_timeline_now(); }
    setCurrentTime(seconds) {
      var n = Number(seconds);
      _lumen_smil_pending_seek = isFinite(n) ? Math.max(0, n) : 0;
      _lumen_smil_wake();
    }
  }
  _lumen_def_svg_lengths(SVGSVGElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 300], height: ['height', 150],
  });
  _lumen_def_svg_viewbox(SVGSVGElement);
  window.SVGSVGElement = SVGSVGElement;

  // SVGPreserveAspectRatio (SVG 2 §8.4) — `{align, meet}` slot, or an `io`
  // `{get, set}` pair over the `preserveAspectRatio` content attribute.
  const _PAR_ALIGN = ['', 'none', 'xMinYMin', 'xMidYMin', 'xMaxYMin', 'xMinYMid', 'xMidYMid',
                      'xMaxYMid', 'xMinYMax', 'xMidYMax', 'xMaxYMax'];
  function _par_parse(str) {
    var out = { align: 6, meet: 1 };
    if (str == null) return out;
    var toks = String(str).trim().split(/\s+/);
    if (toks[0] === 'defer') toks.shift();
    var a = _PAR_ALIGN.indexOf(toks[0]);
    if (a < 1) return out;
    out.align = a;
    if (toks[1] === 'slice') out.meet = 2;
    return out;
  }
  function _par_cur(s) { return s.io ? _par_parse(s.io.get()) : s; }
  function _par_put(s, align, meet) {
    if (s.io) s.io.set(_PAR_ALIGN[align] + (align === 1 || meet !== 2 ? '' : ' slice'));
    else { s.align = align; s.meet = meet; }
  }
  class SVGPreserveAspectRatio {
    constructor() { _gate(); _slots.set(this, { align: 6, meet: 1, io: null }); }
  }
  _def(SVGPreserveAspectRatio, 'align', function(s) { return _par_cur(s).align; }, function(s, v) {
    v = (+v) | 0;
    if (v < 1 || v > 10) throw new TypeError('Invalid alignment');
    _par_put(s, v, _par_cur(s).meet);
  });
  _def(SVGPreserveAspectRatio, 'meetOrSlice', function(s) { return _par_cur(s).meet; }, function(s, v) {
    v = (+v) | 0;
    if (v < 1 || v > 2) throw new TypeError('Invalid meetOrSlice');
    _par_put(s, _par_cur(s).align, v);
  });
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_UNKNOWN  = 0;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_NONE     = 1;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMINYMIN = 2;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMIDYMIN = 3;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMAXYMIN = 4;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMINYMID = 5;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMIDYMID = 6;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMAXYMID = 7;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMINYMAX = 8;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMIDYMAX = 9;
  SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_XMAXYMAX = 10;
  SVGPreserveAspectRatio.SVG_MEETORSLICE_UNKNOWN = 0;
  SVGPreserveAspectRatio.SVG_MEETORSLICE_MEET    = 1;
  SVGPreserveAspectRatio.SVG_MEETORSLICE_SLICE   = 2;
  window.SVGPreserveAspectRatio = SVGPreserveAspectRatio;

  class SVGAnimatedPreserveAspectRatio {
    constructor() { _pair(this, _new(SVGPreserveAspectRatio), _new(SVGPreserveAspectRatio)); }
  }
  _def_pair(SVGAnimatedPreserveAspectRatio);
  window.SVGAnimatedPreserveAspectRatio = SVGAnimatedPreserveAspectRatio;

  // Live `preserveAspectRatio` over the content attribute of node `nid`.
  function _lumen_svg_animated_par(nid) {
    var par = _new(SVGPreserveAspectRatio);
    _slot(par).io = {
      get: function() { return nid != null ? _lumen_u2n(_lumen_get_attr(nid, 'preserveAspectRatio')) : null; },
      set: function(v) { if (nid != null) _lumen_set_attr(nid, 'preserveAspectRatio', v); },
    };
    return _mk(SVGAnimatedPreserveAspectRatio, { baseVal: par, animVal: par });
  }

  // SVGGElement — <g> grouping container
  class SVGGElement extends SVGGraphicsElement {
    constructor() { super(); this.tagName = 'g'; }
  }
  window.SVGGElement = SVGGElement;

  // SVGDefsElement — <defs> container
  class SVGDefsElement extends SVGGraphicsElement {
    constructor() { super(); this.tagName = 'defs'; }
  }
  window.SVGDefsElement = SVGDefsElement;

  // SVGSymbolElement — <symbol>
  class SVGSymbolElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'symbol';
    }
  }
  _lumen_def_svg_viewbox(SVGSymbolElement);
  window.SVGSymbolElement = SVGSymbolElement;

  // SVGUseElement — <use>
  class SVGUseElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'use';
    }
  }
  _lumen_def_svg_lengths(SVGUseElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
  });
  window.SVGUseElement = SVGUseElement;

  // SVGAElement — <a> (SVG 2 §16.2). Carries `interestForElement` from the
  // Interest Invokers `InterestInvokerElement` mixin (GAP-INTERESTINVOKER),
  // installed by the page shim's shared helper.
  class SVGAElement extends SVGGraphicsElement {
    constructor() { super(); this.tagName = 'a'; }
  }
  window.SVGAElement = SVGAElement;
  // SVGURIReference (SVG 2 §5.7) on <a>: `href` is an SVGAnimatedString over
  // the element's attributes — `baseVal` reads `href`, falling back to
  // `xlink:href`, and writes `href` (GAP-ORIGIN: `Origin.from(svgA)` reads the
  // URL the page set through it). One object per element, so `a.href ===
  // a.href`.
  (function() {
    var XLINK_NS = 'http://www.w3.org/1999/xlink';
    var animated = new WeakMap();
    function current(el) {
      var v = el.getAttribute('href');
      if (v === null) v = el.getAttributeNS(XLINK_NS, 'href');
      return v === null ? '' : v;
    }
    Object.defineProperty(SVGAElement.prototype, 'href', {
      get: function() {
        var s = animated.get(this);
        if (s) return s;
        var el = this;
        s = Object.create(SVGAnimatedString.prototype);
        Object.defineProperty(s, 'baseVal', {
          get: function() { return current(el); },
          set: function(v) { el.setAttribute('href', String(v)); },
          enumerable: true, configurable: true,
        });
        Object.defineProperty(s, 'animVal', {
          get: function() { return current(el); },
          enumerable: true, configurable: true,
        });
        animated.set(this, s);
        return s;
      },
      enumerable: true, configurable: true,
    });
  })();
  if (typeof _lumen_install_interest_for === 'function') {
    _lumen_install_interest_for(SVGAElement.prototype, 'SVGAElement');
  }
  // `ping` (HTML LS §4.6.9 hyperlink auditing applies to "a hyperlink",
  // which SVG's <a> is too) — plain DOMString reflection, same as
  // `HTMLAnchorElement.prototype.ping`; no `HTMLHyperlinkElementUtils`.
  if (typeof _lumen_install_reflection === 'function') {
    _lumen_install_reflection(SVGAElement.prototype, [['ping', 'ping', 'string']]);
  }

  // SVGImageElement — <image>
  class SVGImageElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'image';
    }
  }
  _lumen_def_svg_lengths(SVGImageElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
  });
  window.SVGImageElement = SVGImageElement;

  // SVGRectElement — <rect>
  class SVGRectElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'rect'; }
    // W3C SVG 2 §10.6.2: the untransformed bounding box of a <rect> is its
    // geometry rect itself.
    getBBox(options) {
      return _rect(this.x.baseVal.value, this.y.baseVal.value,
        this.width.baseVal.value, this.height.baseVal.value);
    }
  }
  _lumen_def_svg_lengths(SVGRectElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
    rx: ['rx', 0], ry: ['ry', 0],
  });
  window.SVGRectElement = SVGRectElement;

  // SVGCircleElement — <circle>
  class SVGCircleElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'circle'; }
    getBBox(options) {
      var cx = this.cx.baseVal.value, cy = this.cy.baseVal.value, r = this.r.baseVal.value;
      return _rect(cx - r, cy - r, 2 * r, 2 * r);
    }
  }
  _lumen_def_svg_lengths(SVGCircleElement, { cx: ['cx', 0], cy: ['cy', 0], r: ['r', 0] });
  window.SVGCircleElement = SVGCircleElement;

  // SVGEllipseElement — <ellipse>
  class SVGEllipseElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'ellipse'; }
    getBBox(options) {
      var cx = this.cx.baseVal.value, cy = this.cy.baseVal.value;
      var rx = this.rx.baseVal.value, ry = this.ry.baseVal.value;
      return _rect(cx - rx, cy - ry, 2 * rx, 2 * ry);
    }
  }
  _lumen_def_svg_lengths(SVGEllipseElement, {
    cx: ['cx', 0], cy: ['cy', 0], rx: ['rx', 0], ry: ['ry', 0],
  });
  window.SVGEllipseElement = SVGEllipseElement;

  // SVGLineElement — <line>
  class SVGLineElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'line'; }
    getBBox(options) {
      var x1 = this.x1.baseVal.value, y1 = this.y1.baseVal.value;
      var x2 = this.x2.baseVal.value, y2 = this.y2.baseVal.value;
      var x = Math.min(x1, x2), y = Math.min(y1, y2);
      return _rect(x, y, Math.abs(x2 - x1), Math.abs(y2 - y1));
    }
  }
  _lumen_def_svg_lengths(SVGLineElement, {
    x1: ['x1', 0], y1: ['y1', 0], x2: ['x2', 0], y2: ['y2', 0],
  });
  window.SVGLineElement = SVGLineElement;

  // Shared `points`-based bbox (SVG 2 §10.6.2) for polyline/polygon.
  function _lumen_svg_points_bbox(el) {
    var pts = _slot(el.points).items;
    if (!pts.length) return _rect(0, 0, 0, 0);
    var minX = pts[0].x, maxX = pts[0].x, minY = pts[0].y, maxY = pts[0].y;
    for (var i = 1; i < pts.length; i++) {
      minX = Math.min(minX, pts[i].x); maxX = Math.max(maxX, pts[i].x);
      minY = Math.min(minY, pts[i].y); maxY = Math.max(maxY, pts[i].y);
    }
    return _rect(minX, minY, maxX - minX, maxY - minY);
  }

  // SVGPolylineElement — <polyline>
  class SVGPolylineElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'polyline'; }
    getBBox(options) { return _lumen_svg_points_bbox(this); }
  }
  _lumen_def_svg_points(SVGPolylineElement);
  window.SVGPolylineElement = SVGPolylineElement;

  // SVGPolygonElement — <polygon>
  class SVGPolygonElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'polygon'; }
    getBBox(options) { return _lumen_svg_points_bbox(this); }
  }
  _lumen_def_svg_points(SVGPolygonElement);
  window.SVGPolygonElement = SVGPolygonElement;

  // SVGPathElement — <path>
  class SVGPathElement extends SVGGeometryElement {
    constructor() { super(); this.tagName = 'path'; }
    // Phase 1: will parse SVGPathData
    getPathData() { return []; }
    setPathData(data) {}
  }
  // SVG2 §9.3.9 reflects `d` as a plain string (the pre-SVG2 `SVGAnimatedString`
  // wrapper was dropped) — same live-attribute shape as `className`.
  Object.defineProperty(SVGPathElement.prototype, 'd', {
    get: function() { var nid = this.__nid__; return nid != null ? (_lumen_u2n(_lumen_get_attr(nid, 'd')) || '') : ''; },
    set: function(v) { var nid = this.__nid__; if (nid != null) _lumen_set_attr(nid, 'd', String(v)); },
    enumerable: true, configurable: true,
  });
  window.SVGPathElement = SVGPathElement;

  // SVGTextContentElement — base for text elements, adds text-length queries
  class SVGTextContentElement extends SVGGraphicsElement {
    constructor() {
      super();
    }
    getNumberOfChars() { return 0; }
    getComputedTextLength() { return 0; }
    getSubStringLength(charNum, nChars) { return 0; }
    getStartPositionOfChar(charNum) { return _pt(); }
    getEndPositionOfChar(charNum) { return _pt(); }
    getExtentOfChar(charNum) { return _rect(); }
    getRotationOfChar(charNum) { return 0; }
    getCharNumAtPosition(point) { return -1; }
    selectSubString(charNum, nChars) {}
  }
  window.SVGTextContentElement = SVGTextContentElement;

  // SVGTextPositioningElement — adds x/y/dx/dy/rotate
  class SVGTextPositioningElement extends SVGTextContentElement {
    constructor() {
      super();
    }
  }
  window.SVGTextPositioningElement = SVGTextPositioningElement;

  // SVGTextElement — <text>
  class SVGTextElement extends SVGTextPositioningElement {
    constructor() { super(); this.tagName = 'text'; }
  }
  window.SVGTextElement = SVGTextElement;

  // SVGTSpanElement — <tspan>
  class SVGTSpanElement extends SVGTextPositioningElement {
    constructor() { super(); this.tagName = 'tspan'; }
  }
  window.SVGTSpanElement = SVGTSpanElement;

  // SVGTextPathElement — <textPath>
  class SVGTextPathElement extends SVGTextContentElement {
    constructor() {
      super(); this.tagName = 'textPath';
    }
  }
  _lumen_def_svg_lengths(SVGTextPathElement, { startOffset: ['startOffset', 0] });
  window.SVGTextPathElement = SVGTextPathElement;

  // SVGClipPathElement — <clipPath>
  class SVGClipPathElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'clipPath';
    }
  }
  _lumen_def_svg_transform(SVGClipPathElement, 'transform', 'transform');
  _lumen_def_attrs(SVGClipPathElement, [['clipPathUnits', 'Enumeration', 1, ['userSpaceOnUse', 'objectBoundingBox']]]);
  window.SVGClipPathElement = SVGClipPathElement;

  // SVGMaskElement — <mask>
  class SVGMaskElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'mask';
    }
  }
  _lumen_def_svg_lengths(SVGMaskElement, {
    x: ['x', -10], y: ['y', -10], width: ['width', 120], height: ['height', 120],
  });
  _lumen_def_attrs(SVGMaskElement, [
    ['maskUnits', 'Enumeration', 2, ['userSpaceOnUse', 'objectBoundingBox']], ['maskContentUnits', 'Enumeration', 1, ['userSpaceOnUse', 'objectBoundingBox']],
  ]);
  window.SVGMaskElement = SVGMaskElement;

  // SVGGradientElement — base for gradient elements
  class SVGGradientElement extends SVGElement {
    constructor() {
      super();
    }
  }
  SVGGradientElement.SVG_SPREADMETHOD_UNKNOWN = 0;
  SVGGradientElement.SVG_SPREADMETHOD_PAD     = 1;
  SVGGradientElement.SVG_SPREADMETHOD_REFLECT = 2;
  SVGGradientElement.SVG_SPREADMETHOD_REPEAT  = 3;
  _lumen_def_svg_transform(SVGGradientElement, 'gradientTransform', 'gradientTransform');
  window.SVGGradientElement = SVGGradientElement;

  // SVGLinearGradientElement — <linearGradient>
  class SVGLinearGradientElement extends SVGGradientElement {
    constructor() { super(); this.tagName = 'linearGradient'; }
  }
  _lumen_def_svg_lengths(SVGLinearGradientElement, {
    x1: ['x1', 0], y1: ['y1', 0], x2: ['x2', 100], y2: ['y2', 0],
  });
  window.SVGLinearGradientElement = SVGLinearGradientElement;

  // SVGRadialGradientElement — <radialGradient>
  class SVGRadialGradientElement extends SVGGradientElement {
    constructor() { super(); this.tagName = 'radialGradient'; }
  }
  _lumen_def_svg_lengths(SVGRadialGradientElement, {
    cx: ['cx', 50], cy: ['cy', 50], r: ['r', 50], fx: ['fx', 50], fy: ['fy', 50], fr: ['fr', 0],
  });
  window.SVGRadialGradientElement = SVGRadialGradientElement;

  // SVGStopElement — <stop>
  class SVGStopElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'stop';
    }
  }
  window.SVGStopElement = SVGStopElement;

  // SVGPatternElement — <pattern>
  class SVGPatternElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'pattern';
    }
  }
  _lumen_def_svg_lengths(SVGPatternElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
  });
  _lumen_def_svg_viewbox(SVGPatternElement);
  _lumen_def_svg_transform(SVGPatternElement, 'patternTransform', 'patternTransform');
  window.SVGPatternElement = SVGPatternElement;

  // SVGMarkerElement — <marker>
  class SVGMarkerElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'marker';
    }
    setOrientToAuto() { this.orientType.baseVal = 1; }
    setOrientToAngle(angle) { this.orientType.baseVal = 2; this.orientAngle.baseVal = angle; }
  }
  _lumen_def_svg_lengths(SVGMarkerElement, {
    refX: ['refX', 0], refY: ['refY', 0], markerWidth: ['markerWidth', 3], markerHeight: ['markerHeight', 3],
  });
  _lumen_def_svg_viewbox(SVGMarkerElement);
  window.SVGMarkerElement = SVGMarkerElement;

  // SVGFilterElement — <filter>
  class SVGFilterElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'filter';
    }
  }
  _lumen_def_svg_lengths(SVGFilterElement, {
    x: ['x', -10], y: ['y', -10], width: ['width', 120], height: ['height', 120],
  });
  _lumen_def_attrs(SVGFilterElement, [
    ['filterUnits', 'Enumeration', 2, ['userSpaceOnUse', 'objectBoundingBox']], ['primitiveUnits', 'Enumeration', 1, ['userSpaceOnUse', 'objectBoundingBox']], ['href', 'String'],
  ]);
  window.SVGFilterElement = SVGFilterElement;

  // SVGFEBlendElement — <feBlend>
  class SVGFEBlendElement extends SVGElement {
    constructor() { super(); this.tagName = 'feBlend'; }
  }
  window.SVGFEBlendElement = SVGFEBlendElement;

  // SVGFEColorMatrixElement — <feColorMatrix>
  class SVGFEColorMatrixElement extends SVGElement {
    constructor() { super(); this.tagName = 'feColorMatrix'; }
  }
  window.SVGFEColorMatrixElement = SVGFEColorMatrixElement;

  // SVGFECompositeElement — <feComposite>
  class SVGFECompositeElement extends SVGElement {
    constructor() { super(); this.tagName = 'feComposite'; }
  }
  window.SVGFECompositeElement = SVGFECompositeElement;

  // SVGFEGaussianBlurElement — <feGaussianBlur>
  class SVGFEGaussianBlurElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'feGaussianBlur';
    }
    setStdDeviation(sdx, sdy) {
      _attr_set(this, 'stdDeviation', _num(sdx) + ' ' + _num(sdy != null ? sdy : sdx));
    }
  }
  _lumen_def_attrs(SVGFEGaussianBlurElement, [['in1', 'String']]);
  // `stdDeviation` is "<number> [<number>]": X is the first token, Y the second (or X).
  [['stdDeviationX', 0], ['stdDeviationY', 1]].forEach(function(d) {
    Object.defineProperty(SVGFEGaussianBlurElement.prototype, d[0], {
      get: _fn_name(function() {
        if (!(this instanceof SVGFEGaussianBlurElement) || this === SVGFEGaussianBlurElement.prototype) {
          throw new TypeError('Illegal invocation');
        }
        var el = this;
        function toks() {
          var n = _split_list(_attr_get(el, 'stdDeviation')).map(parseFloat).filter(function(x) { return !isNaN(x); });
          return n.length ? n : [0];
        }
        return _mk_scalar(SVGAnimatedNumber, function() {
          var n = toks();
          return d[1] === 1 && n.length > 1 ? n[1] : n[0];
        }, function(v) {
          var n = toks();
          var x = d[1] === 0 ? v : n[0], y = d[1] === 1 ? v : (n.length > 1 ? n[1] : n[0]);
          _attr_set(el, 'stdDeviation', x + ' ' + y);
        });
      }, 'get ' + d[0]),
      enumerable: true, configurable: true,
    });
  });
  window.SVGFEGaussianBlurElement = SVGFEGaussianBlurElement;

  // SVGFEOffsetElement — <feOffset>
  class SVGFEOffsetElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'feOffset';
    }
  }
  _lumen_def_attrs(SVGFEOffsetElement, [['dx', 'Number', 0], ['dy', 'Number', 0]]);
  window.SVGFEOffsetElement = SVGFEOffsetElement;

  // SVGFEMergeElement / SVGFEMergeNodeElement
  class SVGFEMergeElement extends SVGElement {
    constructor() { super(); this.tagName = 'feMerge'; }
  }
  window.SVGFEMergeElement = SVGFEMergeElement;

  class SVGFEMergeNodeElement extends SVGElement {
    constructor() { super(); this.tagName = 'feMergeNode'; }
  }
  window.SVGFEMergeNodeElement = SVGFEMergeNodeElement;

  // SVGSwitchElement — <switch>
  class SVGSwitchElement extends SVGGraphicsElement {
    constructor() { super(); this.tagName = 'switch'; }
  }
  window.SVGSwitchElement = SVGSwitchElement;

  // SVGForeignObjectElement — <foreignObject>
  class SVGForeignObjectElement extends SVGGraphicsElement {
    constructor() { super(); this.tagName = 'foreignObject'; }
  }
  _lumen_def_svg_lengths(SVGForeignObjectElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
  });
  window.SVGForeignObjectElement = SVGForeignObjectElement;

  // ── SMIL timing model (GAP-SMIL, BUG-806) ───────────────────────────────
  // Minimal slice: numeric-offset `begin`/`end` (`0s`, `500ms`, `indefinite`
  // + explicit `beginElement()`/`endElement()`), `dur` in the same grammar
  // (`indefinite`/`media` → no natural end), integer/fractional/`indefinite`
  // `repeatCount`, `fill="remove"|"freeze"`, and `to`/`from`/`values`
  // applied into a shadow "animVal" override — never the content attribute,
  // so `getAttribute()` stays truthful (SVG 2 §3 animVal/baseVal split).
  // BUG-1095: begin/end are `;` lists of offsets, syncbase (`id.begin|end`)
  // and event-base terms feeding an interval model (restart, several
  // intervals, cyclic syncbase); seeking replays it silently.
  // Deliberately out of scope: <animateMotion> path following, <animateTransform> matrix composition —
  // those two still fire correct begin/repeat/end events and IDL methods,
  // they just don't change what paints. All state lives in JS: the timing
  // model is DOM-structural (target = parentNode), not CSS-cascade-derived,
  // so there is nothing for the Rust layout scheduler to own (unlike
  // `TransitionScheduler`/`AnimationScheduler` for CSS transitions and
  // animations, in `crates/engine/layout/src/animation.rs`).

  // `true` once any `<animate>`/`<set>`/`<animateTransform>`/
  // `<animateMotion>` element has ever been constructed — lets
  // `_lumen_tick_smil` (called from Rust every frame, see
  // `crates/shell/src/lumen/smil.rs`) no-op in one boolean check on the
  // overwhelming majority of pages that have no SMIL at all.
  var _lumen_smil_seen = false;
  // Document-timeline zero point (seconds, same domain as the rAF timestamp
  // `_lumen_tick_smil` receives) — the `now_s` of the first tick that finds
  // any SMIL element. `begin="0s"` resolves relative to this, not to
  // wall-clock zero.
  var _lumen_smil_doc_epoch = null;
  // Virtual timeline clock (same domain as `now_s`): advances with the real
  // clock unless `pauseAnimations()` froze it; `setCurrentTime` queues a seek
  // that the next tick applies (`_lumen_smil_seeked` for that whole tick).
  var _lumen_smil_last_now = 0;
  var _lumen_smil_last_real = 0;
  var _lumen_smil_paused = false;
  var _lumen_smil_pending_seek = null;
  var _lumen_smil_seeked = false;
  // The shell only ticks SMIL on a redraw, and nothing else asks for one on a
  // static page: an outstanding no-op rAF is what keeps frames coming while an
  // animation is active/pending, a seek is queued, or a script/listener
  // just started one. Not re-armed once everything is idle.
  var _lumen_smil_raf = false;
  var _lumen_smil_wanted = false;
  function _lumen_smil_wake() {
    if (typeof requestAnimationFrame !== 'function') return;
    // A wake during the frame's own tick lands while the previous rAF is
    // still outstanding: remember it so that callback re-arms.
    if (_lumen_smil_raf) { _lumen_smil_wanted = true; return; }
    _lumen_smil_raf = true;
    requestAnimationFrame(function() {
      _lumen_smil_raf = false;
      if (_lumen_smil_wanted) { _lumen_smil_wanted = false; _lumen_smil_wake(); }
    });
  }
  function _lumen_smil_timeline_now() {
    if (_lumen_smil_pending_seek !== null) return _lumen_smil_pending_seek;
    // epoch + s - epoch is not exactly s in floating point; µs is plenty.
    return _lumen_smil_doc_epoch === null ? 0
      : Math.round((_lumen_smil_last_now - _lumen_smil_doc_epoch) * 1e6) / 1e6;
  }
  // Per-node (`__nid__`) timing state — begin/end instance times, repeat
  // count fired so far, one-shot latches so begin/end fire exactly once.
  var _lumen_smil_states = {};
  // `"<nid>|<attributeName>"` → applied value string, consulted by
  // `_lumen_svg_animated_length`'s `animVal` getter. Cleared on `fill:
  // "remove"` (the default); left in place on `fill: "freeze"`. Exposed on
  // `window` (same object, not a copy) purely so the crate's own unit tests
  // can assert on it from a separate `rt.eval()` call — production code
  // only ever reaches it as the closed-over `_lumen_smil_overrides` above.
  var _lumen_smil_overrides = {};
  // Key → nid of the animation that last wrote it (two animations of one
  // attribute must not clear each other's value).
  var _lumen_smil_owner = {};
  __lumen_C._lumen_smil_overrides = _lumen_smil_overrides;

  // SMIL clock-value grammar (Timing §Clock values): full (`h:mm:ss[.f]`),
  // partial (`mm:ss[.f]`, minutes/seconds exactly two digits and < 60) and
  // timecount (`n[.f][h|min|s|ms]`, default metric `s`).
  function _lumen_smil_parse_clock(tok) {
    if (tok == null) return null;
    var t = String(tok).trim();
    var m = /^([+-]?)([0-9]+(?:\.[0-9]+)?|\.[0-9]+)(h|min|s|ms)?$/.exec(t);
    if (m) {
      var n = parseFloat(m[2]);
      if (isNaN(n)) return null;
      var mul = m[3] === 'h' ? 3600 : m[3] === 'min' ? 60 : m[3] === 'ms' ? 0.001 : 1;
      return (m[1] === '-' ? -n : n) * mul;
    }
    m = /^([0-9]+):([0-9]{2}):([0-9]{2}(?:\.[0-9]+)?)$/.exec(t);
    if (m && +m[2] < 60 && parseFloat(m[3]) < 60) return +m[1] * 3600 + +m[2] * 60 + parseFloat(m[3]);
    m = /^([0-9]{2}):([0-9]{2}(?:\.[0-9]+)?)$/.exec(t);
    if (m && +m[1] < 60 && parseFloat(m[2]) < 60) return +m[1] * 60 + parseFloat(m[2]);
    return null;
  }

  // `begin`/`end` — list of `;`-separated terms (SMIL Timing §begin-value-list).
  // Supported: clock offsets (`2s`), `indefinite` (no term — only
  // `beginElement()` starts it), syncbase (`id.begin`/`id.end` ± offset) and
  // event-base (`id.eventname` ± offset, id optional → parent element).
  // Anything unparseable is dropped. Event terms collect their instance
  // times in `times` (absolute timeline instants, filled by the listener).
  function _lumen_smil_parse_terms(nid, attr) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, attr));
    var terms = [];
    if (raw == null) return terms;
    raw.split(';').forEach(function(part) {
      var tok = part.trim();
      if (tok === '' || tok === 'indefinite') return;
      var c = _lumen_smil_parse_clock(tok);
      if (c !== null) { terms.push({ kind: 'offset', off: c }); return; }
      var off = 0;
      var head = tok;
      var om = /\s*([+-])\s*([0-9.][^\s]*)$/.exec(tok);
      if (om) {
        var ov = _lumen_smil_parse_clock(om[2]);
        if (ov !== null) { off = om[1] === '-' ? -ov : ov; head = tok.slice(0, om.index); }
      }
      // `[id.]name` — the first unescaped dot splits id from name.
      var dot = -1;
      for (var k = 0; k < head.length; k++) {
        if (head[k] === '\\') { k++; continue; }
        if (head[k] === '.') { dot = k; break; }
      }
      var unesc = function(x) { return x.replace(/\\(.)/g, '$1'); };
      var id = dot >= 0 ? unesc(head.slice(0, dot)) : null;
      var name = unesc(dot >= 0 ? head.slice(dot + 1) : head).trim();
      if (!name || (dot >= 0 && id === '')) return;
      var rep = /^repeat\(\s*([0-9]+)\s*\)$/.exec(name);
      if (rep && id !== null) {
        terms.push({ kind: 'event', id: id, name: 'repeatEvent', iter: +rep[1], off: off, bound: false, times: [] });
      } else if (id !== null && (name === 'begin' || name === 'end')) {
        terms.push({ kind: 'sync', id: id, which: name, off: off });
      } else {
        terms.push({ kind: 'event', id: id, name: name, off: off, bound: false, times: [] });
      }
    });
    return terms;
  }

  function _lumen_smil_parse_dur(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'dur'));
    if (raw == null || raw.trim() === '' || raw === 'indefinite' || raw === 'media') return Infinity;
    var v = _lumen_smil_parse_clock(raw.trim());
    return (v === null || v <= 0) ? Infinity : v;
  }

  function _lumen_smil_parse_repeat_count(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'repeatCount'));
    if (raw == null || raw.trim() === '') return 1;
    if (raw.trim() === 'indefinite') return Infinity;
    var n = parseFloat(raw);
    // Beyond single-float range the value is treated as unspecified (WPT
    // `repeatcount-numeric-limit`), not as `indefinite`.
    return (isNaN(n) || n <= 0 || n > 3.4028235e38) ? 1 : n;
  }

  function _lumen_smil_parse_fill(nid) {
    return _lumen_u2n(_lumen_get_attr(nid, 'fill')) === 'freeze' ? 'freeze' : 'remove';
  }

  // Builds the value list to animate across from `values` (semicolon list)
  // or `from`/`to` (falling back to a single-value `to`-only list).
  function _lumen_smil_value_list(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'values'));
    var items;
    if (raw != null) {
      items = raw.split(';');
      // One trailing `;` is a list terminator, not an empty last value.
      if (items.length > 1 && items[items.length - 1].trim() === '') items.pop();
    } else {
      var toRaw = _lumen_u2n(_lumen_get_attr(nid, 'to'));
      var fromRaw = _lumen_u2n(_lumen_get_attr(nid, 'from'));
      if (toRaw == null) return null;
      items = fromRaw != null ? [fromRaw, toRaw] : [toRaw];
    }
    // SMIL Animation §ToAttribute: an illegal value makes the whole
    // animation a no-op. A value that looks numeric must be one length
    // (`100px`, no inner/outer whitespace) or a whitespace/comma list.
    var NUM = '[+-]?(?:[0-9]+\\.?[0-9]*|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?';
    var one = new RegExp('^' + NUM + '(?:px|em|ex|%|cm|mm|in|pt|pc)?$');
    var many = new RegExp('^' + NUM + '(?:[\\s,]+' + NUM + ')+$');
    for (var i = 0; i < items.length; i++) {
      if (/^\s*[+-]?[0-9.]/.test(items[i]) && !one.test(items[i]) && !many.test(items[i])) return null;
    }
    return items.map(function(s) { return s.trim(); });
  }

  // Numeric attribute types get linear interpolation across the value list;
  // anything else (colors, keywords) steps discretely, holding each value
  // for an equal fraction of the simple duration (SMIL Animation §3.2.1
  // default `calcMode` behaviour, simplified to equal-length steps).
  function _lumen_smil_compute_value(nid, fraction) {
    var list = _lumen_smil_value_list(nid);
    if (!list || list.length === 0) return null;
    if (list.length === 1) return list[0];
    var allNumeric = list.every(function(v) {
      return v !== '' && isFinite(parseFloat(v)) && /^[+-]?[0-9]*\.?[0-9]+$/.test(v);
    });
    if (allNumeric) {
      var nums = list.map(parseFloat);
      var seg = fraction * (nums.length - 1);
      var i0 = Math.min(Math.floor(seg), nums.length - 2);
      var t = seg - i0;
      return String(nums[i0] + (nums[i0 + 1] - nums[i0]) * t);
    }
    var idx = Math.min(Math.floor(fraction * list.length), list.length - 1);
    return list[idx];
  }

  function _lumen_smil_get_state(nid) {
    var st = _lumen_smil_states[nid];
    if (!st) {
      st = {
        // The interval model: `cur` is the active interval `{b}` (its end is
        // re-derived every tick, so a late `endElement()`/restart still
        // lands); `begins`/`ends` are the instants of every interval so far,
        // which is what `id.begin`/`id.end` syncbases of other elements read.
        cur: null, lastBegin: null, lastEnd: null, begins: [], ends: [],
        beginTime: null, endTime: null, ended: false, cycle: 0,
        manualBegin: null, manualEnd: null, mbTimes: [], meTimes: [],
        terms: null, endTerms: null, endSpec: false,
      };
      _lumen_smil_states[nid] = st;
    }
    return st;
  }

  // `beginElementAt`/`endElementAt` (SVG SMIL Animation §3.4): queue an
  // instance time resolved against "now" on the *next* tick, matching the
  // spec's "current time + offset" semantics closely enough for the
  // explicit-trigger case (`begin="indefinite"` + a script call).
  function _lumen_smil_begin_now(nid, offset) { _lumen_smil_get_state(nid).manualBegin = offset || 0; _lumen_smil_wake(); }
  function _lumen_smil_end_now(nid, offset) { _lumen_smil_get_state(nid).manualEnd = offset || 0; _lumen_smil_wake(); }

  function _lumen_smil_dispatch(nid, type, detail) {
    var ev = new Event(type, { bubbles: false, cancelable: false });
    // `repeatEvent.detail` is the iteration number (SVG Animations §Event).
    if (detail !== undefined) ev.detail = detail;
    _lumen_dispatch(nid, ev);
  }

  function _lumen_smil_bind_event(el, tm) {
    if (tm.bound) return;
    var tgt = tm.id === null ? el.parentNode : document.getElementById(tm.id);
    if (tgt && typeof tgt.addEventListener === 'function') {
      tm.bound = true;
      tgt.addEventListener(tm.name, function(ev) {
        if (tm.iter !== undefined && (ev.detail | 0) !== tm.iter) return;
        tm.times.push(_lumen_smil_last_now + tm.off);
        _lumen_smil_wake();
      });
    }
  }

  // Parses `begin`/`end` once per element (a missing `begin` means `0s`).
  function _lumen_smil_ensure_terms(nid, st) {
    if (st.terms !== null) return;
    st.terms = _lumen_smil_parse_terms(nid, 'begin');
    var erw = _lumen_u2n(_lumen_get_attr(nid, 'end'));
    st.endSpec = erw != null && erw.trim() !== '';
    st.endTerms = _lumen_smil_parse_terms(nid, 'end');
    var braw = _lumen_u2n(_lumen_get_attr(nid, 'begin'));
    if (braw == null || braw.trim() === '') st.terms.push({ kind: 'offset', off: 0 });
  }

  // Event-base listeners must exist before the event happens — not only
  // from the first frame — so every animation element gets its terms bound
  // eagerly: at the start of each tick, and (via `_lumen_smil_prebind`, hooked
  // into event propagation) before a script-dispatched event is delivered.
  var _lumen_smil_dirty = true;
  function _lumen_smil_bind_all(all) {
    for (var i = 0; i < all.length; i++) {
      var el = all[i];
      if (!(el instanceof SVGAnimationElement) || el.__nid__ == null) continue;
      var st = _lumen_smil_get_state(el.__nid__);
      _lumen_smil_ensure_terms(el.__nid__, st);
      var lists = [st.terms, st.endTerms];
      for (var l = 0; l < 2; l++) {
        for (var t = 0; t < lists[l].length; t++) {
          if (lists[l][t].kind === 'event') _lumen_smil_bind_event(el, lists[l][t]);
        }
      }
    }
    _lumen_smil_dirty = false;
  }
  __lumen_C._lumen_smil_prebind = function() {
    if (!_lumen_smil_seen || !_lumen_smil_dirty || typeof document === 'undefined'
        || typeof document.getElementsByTagName !== 'function') return;
    _lumen_smil_bind_all(document.getElementsByTagName('*'));
  };

  // Instance times of one term list (SMIL Timing §instance-times): offsets
  // are document-timeline instants, syncbase terms read the referenced
  // element's finished intervals, event terms the instants their listener
  // recorded. Event listeners are attached lazily on the first look.
  function _lumen_smil_instances(el, terms, epoch, extra) {
    var out = extra.slice();
    for (var i = 0; i < terms.length; i++) {
      var tm = terms[i];
      if (tm.kind === 'offset') { out.push(epoch + tm.off); continue; }
      if (tm.kind === 'sync') {
        var ref = document.getElementById(tm.id);
        var rs = ref && ref.__nid__ != null ? _lumen_smil_states[ref.__nid__] : null;
        if (rs) {
          var src = tm.which === 'begin' ? rs.begins : rs.ends;
          for (var j = 0; j < src.length; j++) out.push(src[j] + tm.off);
        }
        continue;
      }
      _lumen_smil_bind_event(el, tm);
      for (var q = 0; q < tm.times.length; q++) out.push(tm.times[q]);
    }
    out.sort(function(a, b) { return a - b; });
    return out;
  }

  // End of the interval that begins at `b`: earliest `end` instance ≥ `b`
  // capped by the active duration (`dur`×`repeatCount`) and — with
  // `restart="always"` — by the next begin instance. `null`: `end` is given
  // and fully resolved but every instant precedes `b` (no interval).
  function _lumen_smil_interval_end(el, nid, st, b, epoch, beginInst) {
    var dur = _lumen_smil_parse_dur(nid);
    var rc = _lumen_smil_parse_repeat_count(nid);
    var activeDur = (dur === Infinity || rc === Infinity) ? Infinity : dur * rc;
    var ee = Infinity, found = false;
    var ends = _lumen_smil_instances(el, st.endTerms, epoch, st.meTimes);
    for (var i = 0; i < ends.length; i++) {
      if (ends[i] >= b) { ee = ends[i]; found = true; break; }
    }
    if (st.endSpec && !found && st.endTerms.length > 0
        && st.endTerms.every(function(t) { return t.kind === 'offset'; })) return null;
    var e = Math.min(activeDur === Infinity ? Infinity : b + activeDur, ee);
    // `min`/`max` clamp the active duration (an invalid pair — max < min —
    // disables both, SMIL Timing §min-max).
    var mn = _lumen_smil_parse_clock(_lumen_u2n(_lumen_get_attr(nid, 'min')));
    var mx = _lumen_smil_parse_clock(_lumen_u2n(_lumen_get_attr(nid, 'max')));
    if (mn === null || mn < 0) mn = 0;
    if (mx === null || mx < 0) mx = Infinity;
    if (mx < mn) { mn = 0; mx = Infinity; }
    if (e - b > mx) e = b + mx;
    else if (e - b < mn) e = b + mn;
    var restart = _lumen_u2n(_lumen_get_attr(nid, 'restart'));
    if (restart !== 'never' && restart !== 'whenNotActive') {
      for (var k = 0; k < beginInst.length; k++) {
        if (beginInst[k] > b && beginInst[k] < e) { e = beginInst[k]; break; }
      }
    }
    return e;
  }

  // Advances one animation to `now_s`: opens/closes as many intervals as
  // fit (SMIL Timing §interval-timing), raising begin/repeat/end events
  // unless `quiet` (seek replay). Returns whether any interval changed.
  function _lumen_smil_advance(el, now_s, epoch, quiet) {
    var nid = el.__nid__;
    var st = _lumen_smil_get_state(nid);
    _lumen_smil_ensure_terms(nid, st);
    if (st.manualBegin !== null) { st.mbTimes.push(now_s + st.manualBegin); st.manualBegin = null; }
    var dur = _lumen_smil_parse_dur(nid);
    var rc = _lumen_smil_parse_repeat_count(nid);
    var changed = false;
    var endApplied = false;
    for (var guard = 0; guard < 64; guard++) {
      var bi = _lumen_smil_instances(el, st.terms, epoch, st.mbTimes);
      if (st.cur === null) {
        var minB = st.lastEnd === null ? -Infinity : st.lastEnd;
        var pick = null;
        for (var i = 0; i < bi.length; i++) {
          var b = bi[i];
          if (b > now_s) break;
          if (b < minB || (st.lastBegin !== null && b <= st.lastBegin)) continue;
          if (_lumen_smil_interval_end(el, nid, st, b, epoch, bi) === null) continue;
          pick = b; break;
        }
        if (pick === null) break;
        st.cur = { b: pick };
        st.lastBegin = pick; st.beginTime = pick; st.ended = false; st.cycle = 0;
        st.begins.push(pick);
        changed = true;
        if (!quiet) _lumen_smil_dispatch(nid, 'beginEvent');
        continue;
      }
      if (st.manualEnd !== null && !endApplied) {
        st.meTimes.push(now_s + st.manualEnd); st.manualEnd = null; endApplied = true;
      }
      var e = _lumen_smil_interval_end(el, nid, st, st.cur.b, epoch, bi);
      if (e === null) e = Infinity;
      if (dur !== Infinity) {
        for (;;) {
          var rep = st.cur.b + dur * (st.cycle + 1);
          if (rep < e && rep <= now_s && (rc === Infinity || st.cycle + 1 < Math.ceil(rc))) {
            st.cycle++;
            if (!quiet) _lumen_smil_dispatch(nid, 'repeatEvent', st.cycle);
          } else break;
        }
      }
      if (e > now_s) break;
      st.lastEnd = e; st.endTime = e; st.ends.push(e); st.ended = true; st.cur = null;
      changed = true;
      if (!quiet) _lumen_smil_dispatch(nid, 'endEvent');
    }
    if (st.manualEnd !== null && st.cur === null) st.manualEnd = null;
    var attrName = _lumen_u2n(_lumen_get_attr(nid, 'attributeName'));
    if (attrName) {
      // `animVal` reads the animated *target* (the parent element), so the
      // override is keyed by it; a detached node keys by itself.
      var tgt = el.parentNode;
      var key = ((tgt && tgt.__nid__ != null) ? tgt.__nid__ : nid) + '|' + attrName;
      var frac = null;
      if (st.cur !== null) {
        var elapsed = now_s - st.cur.b;
        frac = 0;
        if (dur !== Infinity) {
          frac = Math.max(0, Math.min(1, (elapsed - dur * Math.floor(elapsed / dur)) / dur));
        }
      } else if (st.lastEnd !== null && _lumen_smil_parse_fill(nid) === 'freeze') {
        frac = 1;
        if (dur !== Infinity) {
          var span = st.lastEnd - st.lastBegin;
          var rem = span - dur * Math.floor(span / dur);
          frac = (rem < 1e-9 && span > 0) ? 1 : rem / dur;
        }
      }
      var v = frac === null ? null : _lumen_smil_compute_value(nid, frac);
      if (v !== null) { _lumen_smil_overrides[key] = v; _lumen_smil_owner[key] = nid; }
      else if (_lumen_smil_owner[key] === nid || _lumen_smil_owner[key] === undefined) {
        delete _lumen_smil_overrides[key]; delete _lumen_smil_owner[key];
      }
    }
    return changed;
  }

  // One document-order sweep, repeated while intervals still change so that
  // syncbase chains (`a.end` → `b.begin` → `c.begin`) settle in one tick.
  function _lumen_smil_sweep(all, now_s, epoch, quiet) {
    _lumen_smil_bind_all(all);
    for (var pass = 0; pass < 8; pass++) {
      var any = false;
      for (var i = 0; i < all.length; i++) {
        if (all[i] instanceof SVGAnimationElement && all[i].__nid__ != null
            && _lumen_smil_advance(all[i], now_s, epoch, quiet)) any = true;
      }
      if (!any) break;
    }
  }

  // A seek re-derives every state as a pure function of the new time: the
  // jump replays silently from scratch, then only active↔inactive
  // transitions (or a different active interval) raise begin/end events.
  function _lumen_smil_seek_all(all, now_s, epoch) {
    var snap = [];
    for (var i = 0; i < all.length; i++) {
      if (!(all[i] instanceof SVGAnimationElement) || all[i].__nid__ == null) continue;
      var st = _lumen_smil_get_state(all[i].__nid__);
      snap.push({ el: all[i], st: st, was: st.cur === null ? null : st.cur.b });
      st.cur = null; st.lastBegin = null; st.lastEnd = null; st.begins = []; st.ends = [];
      st.cycle = 0; st.ended = false; st.beginTime = null;
    }
    _lumen_smil_sweep(all, now_s, epoch, true);
    for (var j = 0; j < snap.length; j++) {
      var s = snap[j], nb = s.st.cur === null ? null : s.st.cur.b;
      if (s.was === nb) continue;
      if (s.was !== null) _lumen_smil_dispatch(s.el.__nid__, 'endEvent');
      if (nb !== null) _lumen_smil_dispatch(s.el.__nid__, 'beginEvent');
    }
  }

  // Called once per rendering frame from the Rust shell
  // (`PersistentJs::tick_smil`, `crates/shell/src/lumen/smil.rs`), in the
  // same spec step CSS transitions/animations tick, before rAF callbacks.
  __lumen_C._lumen_tick_smil = function(now_s) {
    if (!_lumen_smil_seen) return;
    if (_lumen_smil_doc_epoch === null) {
      // The document timeline starts once loading is done (SVG Animations
      // §Timing: begins at the document's `load`), so animations don't run
      // their first interval before the page's own scripts could listen.
      if (typeof document !== 'undefined'
          && (document.readyState === 'loading' || document.readyState === 'interactive')) {
        _lumen_smil_wake();
        return;
      }
      _lumen_smil_doc_epoch = now_s;
      _lumen_smil_last_now = now_s;
    } else if (!_lumen_smil_paused) {
      _lumen_smil_last_now += now_s - _lumen_smil_last_real;
    }
    _lumen_smil_last_real = now_s;
    if (_lumen_smil_pending_seek !== null) {
      _lumen_smil_last_now = _lumen_smil_doc_epoch + _lumen_smil_pending_seek;
      _lumen_smil_pending_seek = null;
      _lumen_smil_seeked = true;
    }
    now_s = _lumen_smil_last_now;
    if (typeof document === 'undefined' || typeof document.getElementsByTagName !== 'function') return;
    var all = document.getElementsByTagName('*');
    if (_lumen_smil_seeked) _lumen_smil_seek_all(all, now_s, _lumen_smil_doc_epoch);
    else _lumen_smil_sweep(all, now_s, _lumen_smil_doc_epoch, false);
    _lumen_smil_seeked = false;
    if (!_lumen_smil_paused) {
      for (var j = 0; j < all.length; j++) {
        var sst = all[j] instanceof SVGAnimationElement ? _lumen_smil_states[all[j].__nid__] : null;
        if (!sst) continue;
        var pending = sst.cur !== null || sst.manualBegin !== null
          || (sst.terms !== null && _lumen_smil_instances(all[j], sst.terms, _lumen_smil_doc_epoch, sst.mbTimes)
                .some(function(t) { return t > now_s; }));
        if (pending) { _lumen_smil_wake(); break; }
      }
    }
  };

  // SVGAnimationElement — shared base of the four SMIL element interfaces
  // (SVG2 §3: SVGAnimateElement/SVGSetElement/SVGAnimateMotionElement/
  // SVGAnimateTransformElement all implement it).
  class SVGAnimationElement extends SVGElement {
    beginElement() { _lumen_smil_begin_now(this.__nid__, 0); }
    endElement() { _lumen_smil_end_now(this.__nid__, 0); }
    beginElementAt(offset) { _lumen_smil_begin_now(this.__nid__, offset || 0); }
    endElementAt(offset) { _lumen_smil_end_now(this.__nid__, offset || 0); }
    getStartTime() {
      var st = _lumen_smil_states[this.__nid__];
      if (st && st.beginTime !== null && _lumen_smil_doc_epoch !== null) {
        return st.beginTime - _lumen_smil_doc_epoch;
      }
      // Not started yet: a resolved clock-offset begin is still a start time.
      var offs = _lumen_smil_parse_terms(this.__nid__, 'begin')
        .filter(function(t) { return t.kind === 'offset'; })
        .map(function(t) { return t.off; });
      if (offs.length) return Math.min.apply(null, offs);
      if (_lumen_u2n(_lumen_get_attr(this.__nid__, 'begin')) == null) return 0;
      throw new DOMException('The element has no resolved begin time', 'InvalidStateError');
    }
    getCurrentTime() { return _lumen_smil_timeline_now(); }
    getSimpleDuration() {
      var d = _lumen_smil_parse_dur(this.__nid__);
      if (d === Infinity) throw new DOMException('The simple duration is not defined', 'NotSupportedError');
      return d;
    }
    get targetElement() { return this.parentNode; }
  }
  // Guarded: the crate's own unit tests install this shim over a minimal
  // `Element`/`document` stub without the full `web_api_shim_mid.js` (which
  // defines this helper) — only the real browser runtime needs onbegin/
  // onend/onrepeat to be live IDL accessors.
  if (typeof _lumen_define_on_handler_prop === 'function') {
    _lumen_define_on_handler_prop(SVGAnimationElement.prototype, 'onbegin');
    _lumen_define_on_handler_prop(SVGAnimationElement.prototype, 'onend');
    _lumen_define_on_handler_prop(SVGAnimationElement.prototype, 'onrepeat');
  }
  window.SVGAnimationElement = SVGAnimationElement;

  // SVGAnimateElement — <animate>
  class SVGAnimateElement extends SVGAnimationElement {}
  window.SVGAnimateElement = SVGAnimateElement;

  // SVGAnimateTransformElement — <animateTransform> (events/timing only —
  // matrix composition onto the `transform` attribute is out of scope).
  class SVGAnimateTransformElement extends SVGAnimationElement {}
  window.SVGAnimateTransformElement = SVGAnimateTransformElement;

  // SVGAnimateMotionElement — <animateMotion> (events/timing only — path
  // following is out of scope).
  class SVGAnimateMotionElement extends SVGAnimationElement {}
  window.SVGAnimateMotionElement = SVGAnimateMotionElement;

  // SVGSetElement — <set>
  class SVGSetElement extends SVGAnimationElement {}
  window.SVGSetElement = SVGSetElement;

  // SVGMPathElement — <mpath> (SVGURIReference)
  class SVGMPathElement extends SVGElement {
    constructor() { super(); this.tagName = 'mpath'; }
  }
  window.SVGMPathElement = SVGMPathElement;

  // TimeEvent (SMIL Animation §6) — begin/end/repeat events.
  if (typeof Event === 'function') {
    // No WebIDL constructor: only the UA creates TimeEvents, so `new` throws.
    class TimeEvent extends Event {
      constructor() { throw new TypeError('Illegal constructor'); }
      get view() { return null; }
      get detail() { return 0; }
      initTimeEvent(type, view, detail) {}
    }
    window.TimeEvent = TimeEvent;
  }

  // ShadowAnimation — an Animation mirrored into a <use> instance. Lumen has
  // no per-instance animation mirroring; the interface exists for feature
  // detection and extends Animation.
  if (typeof Animation === 'function') {
    class ShadowAnimation extends Animation {
      constructor(source, target) {
        super();
        this._source = source || null;
        this._target = target || null;
      }
      get sourceAnimation() { return this._source; }
    }
    window.ShadowAnimation = ShadowAnimation;
  }

  // SVGUseElementShadowRoot — the shadow root of a <use> instance tree.
  if (typeof ShadowRoot === 'function') {
    class SVGUseElementShadowRoot extends ShadowRoot {}
    window.SVGUseElementShadowRoot = SVGUseElementShadowRoot;
  }

  // Remaining filter primitives (Filter Effects §15): typed prototypes with
  // their SVGAnimated* attribute surface. `spec` entries are
  // `[name, 'Integer'|'Number'|'Enumeration'|'Boolean'|'NumberList', default, keywords?]`,
  // installed as live prototype accessors over the content attribute.
  function _lumen_def_fe(ctorName, tag, spec) {
    var C = class extends SVGElement {
      constructor() { super(); this.tagName = tag; }
    };
    Object.defineProperty(C, 'name', { value: ctorName });
    _lumen_def_attrs(C, spec || []);
    window[ctorName] = C;
    return C;
  }
  const _KUL = [['kernelUnitLengthX', 'Number', 0], ['kernelUnitLengthY', 'Number', 0]];
  var SVGFEComponentTransferElement = _lumen_def_fe('SVGFEComponentTransferElement', 'feComponentTransfer');
  var SVGFEConvolveMatrixElement = _lumen_def_fe('SVGFEConvolveMatrixElement', 'feConvolveMatrix', [
    ['orderX', 'Integer', 3], ['orderY', 'Integer', 3], ['kernelMatrix', 'NumberList'],
    ['divisor', 'Number', 1], ['bias', 'Number', 0], ['targetX', 'Integer', 0],
    ['targetY', 'Integer', 0], ['edgeMode', 'Enumeration', 1, ['duplicate', 'wrap', 'none']],
    ['preserveAlpha', 'Boolean', false],
  ].concat(_KUL));
  var SVGFEDiffuseLightingElement = _lumen_def_fe('SVGFEDiffuseLightingElement', 'feDiffuseLighting', [
    ['surfaceScale', 'Number', 1], ['diffuseConstant', 'Number', 1],
  ].concat(_KUL));
  var SVGFEDisplacementMapElement = _lumen_def_fe('SVGFEDisplacementMapElement', 'feDisplacementMap', [
    ['scale', 'Number', 0],
    ['xChannelSelector', 'Enumeration', 4, ['R', 'G', 'B', 'A']],
    ['yChannelSelector', 'Enumeration', 4, ['R', 'G', 'B', 'A']],
  ]);
  var SVGFEDistantLightElement = _lumen_def_fe('SVGFEDistantLightElement', 'feDistantLight', [
    ['azimuth', 'Number', 0], ['elevation', 'Number', 0],
  ]);
  var SVGFEDropShadowElement = _lumen_def_fe('SVGFEDropShadowElement', 'feDropShadow', [
    ['dx', 'Number', 2], ['dy', 'Number', 2],
    ['stdDeviationX', 'Number', 2], ['stdDeviationY', 'Number', 2],
  ]);
  var SVGFEMorphologyElement = _lumen_def_fe('SVGFEMorphologyElement', 'feMorphology', [
    ['operator', 'Enumeration', 1, ['erode', 'dilate']],
    ['radiusX', 'Number', 0], ['radiusY', 'Number', 0],
  ]);
  var SVGFEPointLightElement = _lumen_def_fe('SVGFEPointLightElement', 'fePointLight', [
    ['x', 'Number', 0], ['y', 'Number', 0], ['z', 'Number', 0],
  ]);
  var SVGFESpecularLightingElement = _lumen_def_fe('SVGFESpecularLightingElement', 'feSpecularLighting', [
    ['surfaceScale', 'Number', 1], ['specularConstant', 'Number', 1], ['specularExponent', 'Number', 1],
  ].concat(_KUL));
  var SVGFESpotLightElement = _lumen_def_fe('SVGFESpotLightElement', 'feSpotLight', [
    ['x', 'Number', 0], ['y', 'Number', 0], ['z', 'Number', 0],
    ['pointsAtX', 'Number', 0], ['pointsAtY', 'Number', 0], ['pointsAtZ', 'Number', 0],
    ['specularExponent', 'Number', 1], ['limitingConeAngle', 'Number', 0],
  ]);
  var SVGFETurbulenceElement = _lumen_def_fe('SVGFETurbulenceElement', 'feTurbulence', [
    ['baseFrequencyX', 'Number', 0], ['baseFrequencyY', 'Number', 0],
    ['numOctaves', 'Integer', 1], ['seed', 'Number', 0],
    ['stitchTiles', 'Enumeration', 2, ['stitch', 'noStitch']],
    ['type', 'Enumeration', 2, ['fractalNoise', 'turbulence']],
  ]);

  // SVGViewElement — <view>
  class SVGViewElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'view';
      this.zoomAndPan = 2; // SVG_ZOOMANDPAN_MAGNIFY
    }
  }
  _lumen_def_svg_viewbox(SVGViewElement);
  window.SVGViewElement = SVGViewElement;

  // SVGScriptElement — <script> inside SVG
  class SVGScriptElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'script';
      this.type = 'text/ecmascript';
    }
  }
  window.SVGScriptElement = SVGScriptElement;

  // SVGStyleElement — <style> inside SVG
  class SVGStyleElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'style';
      this.type = 'text/css';
      this.media = '';
      this.title = '';
    }
  }
  window.SVGStyleElement = SVGStyleElement;

  // SVGDescElement — <desc>
  class SVGDescElement extends SVGElement {
    constructor() { super(); this.tagName = 'desc'; }
  }
  window.SVGDescElement = SVGDescElement;

  // SVGTitleElement — <title>
  class SVGTitleElement extends SVGElement {
    constructor() { super(); this.tagName = 'title'; }
  }
  window.SVGTitleElement = SVGTitleElement;

  // SVGMetadataElement — <metadata>
  class SVGMetadataElement extends SVGElement {
    constructor() { super(); this.tagName = 'metadata'; }
  }
  window.SVGMetadataElement = SVGMetadataElement;

  // ── createElementNS SVG namespace wiring ─────────────────────────────────

  // Map SVG tag names to constructors. Used in the patched createElementNS.
  const SVG_TAG_MAP = {
    'svg':              SVGSVGElement,
    'g':                SVGGElement,
    'defs':             SVGDefsElement,
    'symbol':           SVGSymbolElement,
    'use':              SVGUseElement,
    'a':                SVGAElement,
    'image':            SVGImageElement,
    'switch':           SVGSwitchElement,
    'rect':             SVGRectElement,
    'circle':           SVGCircleElement,
    'ellipse':          SVGEllipseElement,
    'line':             SVGLineElement,
    'polyline':         SVGPolylineElement,
    'polygon':          SVGPolygonElement,
    'path':             SVGPathElement,
    'text':             SVGTextElement,
    'tspan':            SVGTSpanElement,
    'textPath':         SVGTextPathElement,
    'textpath':         SVGTextPathElement,
    'clipPath':         SVGClipPathElement,
    'clippath':         SVGClipPathElement,
    'mask':             SVGMaskElement,
    'linearGradient':   SVGLinearGradientElement,
    'lineargradient':   SVGLinearGradientElement,
    'radialGradient':   SVGRadialGradientElement,
    'radialgradient':   SVGRadialGradientElement,
    'stop':             SVGStopElement,
    'pattern':          SVGPatternElement,
    'marker':           SVGMarkerElement,
    'filter':           SVGFilterElement,
    'feBlend':          SVGFEBlendElement,
    'feColorMatrix':    SVGFEColorMatrixElement,
    'feComposite':      SVGFECompositeElement,
    'feGaussianBlur':   SVGFEGaussianBlurElement,
    'feOffset':         SVGFEOffsetElement,
    'feMerge':          SVGFEMergeElement,
    'feMergeNode':      SVGFEMergeNodeElement,
    'feComponentTransfer': SVGFEComponentTransferElement,
    'feConvolveMatrix': SVGFEConvolveMatrixElement,
    'feDiffuseLighting': SVGFEDiffuseLightingElement,
    'feDisplacementMap': SVGFEDisplacementMapElement,
    'feDistantLight':   SVGFEDistantLightElement,
    'feDropShadow':     SVGFEDropShadowElement,
    'feMorphology':     SVGFEMorphologyElement,
    'fePointLight':     SVGFEPointLightElement,
    'feSpecularLighting': SVGFESpecularLightingElement,
    'feSpotLight':      SVGFESpotLightElement,
    'feTurbulence':     SVGFETurbulenceElement,
    'mpath':            SVGMPathElement,
    'foreignObject':    SVGForeignObjectElement,
    'foreignobject':    SVGForeignObjectElement,
    'animate':          SVGAnimateElement,
    'animateTransform': SVGAnimateTransformElement,
    'animatetransform': SVGAnimateTransformElement,
    'animateMotion':    SVGAnimateMotionElement,
    'animatemotion':    SVGAnimateMotionElement,
    'set':              SVGSetElement,
    'view':             SVGViewElement,
    'script':           SVGScriptElement,
    'style':            SVGStyleElement,
    'desc':             SVGDescElement,
    'title':            SVGTitleElement,
    'metadata':         SVGMetadataElement,
  };

  // GAP-XMLDOC срез 4: lookup shared with `_lumen_element_prototype_for`
  // (web_api_shim_mid.js) below, so a parser-created `<rect>` gets the same
  // typed prototype as one from `createElementNS('rect')` — both fall back to
  // the bare `SVGElement` for a tag `SVG_TAG_MAP` does not know.
  __lumen_C._lumen_svg_ctor_for_local = function(local) {
    var ctor = SVG_TAG_MAP[local] || SVG_TAG_MAP[local.toLowerCase()] || SVGElement;
    // GAP-SMIL perf gate: flip once, the first time any SMIL element (of
    // either markup or `createElementNS` origin) is resolved, so
    // `_lumen_tick_smil` can no-op in one boolean check on every other page.
    if (ctor === SVGAnimationElement || ctor.prototype instanceof SVGAnimationElement) {
      _lumen_smil_dirty = true;
      if (!_lumen_smil_seen) { _lumen_smil_seen = true; _lumen_smil_wake(); }
    }
    return ctor;
  };

  // Decorate document.createElementNS: keep the AUTHORITATIVE native implementation
  // (crates/js/src/dom.rs) — it returns a real arena node carrying __nid__ so that
  // appendChild attaches it and layout/paint render it (BUG-243). For the SVG
  // namespace we additionally re-point the node's prototype at the matching typed
  // SVG*Element class so `instanceof SVGCircleElement` and getBBox()/getCTM() work.
  // Since BUG-849 the native methods live on a shared per-interface prototype, not
  // on the instance, so a bare `setPrototypeOf(el, Ctor.prototype)` would drop every
  // one of them — `_lumen_retarget_wrapper` re-points the wrapper at the shared
  // prototype built for `Ctor.prototype` instead, which keeps both halves.
  if (typeof document !== 'undefined' && typeof document.createElementNS === 'function') {
    const _origCreateElementNS = document.createElementNS.bind(document);
    document.createElementNS = function(ns, qualifiedName) {
      if (ns === SVG_NS) {
        const local = (qualifiedName || '').replace(/^[^:]+:/, '');
        const Ctor = SVG_TAG_MAP[qualifiedName] || _lumen_svg_ctor_for_local(local);
        const el = _origCreateElementNS(ns, qualifiedName);
        try {
          if (typeof _lumen_retarget_wrapper === 'function') {
            _lumen_retarget_wrapper(el, Ctor.prototype);
          } else {
            Object.setPrototypeOf(el, Ctor.prototype);
          }
        } catch (e) {
          // ignore prototype assignment failure
        }
        return el;
      }
      return _origCreateElementNS(ns, qualifiedName);
    };
  }

  // Expose SVG namespace constant
  window.SVG_NAMESPACE = SVG_NS;
"#,
  "
  const _IDL = ",
  include_str!("shim/svg_idl_table.js"),
  ";
",
  include_str!("shim/svg_idl_shape.js"),
  r#"})();
"#
);

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    use crate::v8_runtime::V8JsRuntime;

    /// Install minimal DOM stubs then SVG bindings.
    fn with_svg() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(r#"
            var window = globalThis;
            // Minimal Element stub (SVGElement extends it)
            class Element {
                constructor() {
                    this.attributes = {};
                    this.children = [];
                    this.childNodes = [];
                }
                getAttribute(n) { return this.attributes[n] || null; }
                setAttribute(n, v) { this.attributes[n] = v; }
                removeAttribute(n) { delete this.attributes[n]; }
                hasAttribute(n) { return n in this.attributes; }
                appendChild(c) { this.children.push(c); return c; }
                addEventListener() {}
                removeEventListener() {}
                dispatchEvent() { return true; }
            }
            window.Element = Element;
            // Minimal document with createElementNS
            var document = {
                createElementNS: function(ns, tag) { return new Element(); }
            };
            globalThis.document = document;
            window.document = document;
        "#).unwrap();
        super::install_svg_bindings_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn svg_element_class_exists() {
        let rt = with_svg();
        assert!(bool_eval(&rt, "typeof window.SVGElement === 'function'"));
    }

    #[test]
    fn bug_1092_missing_svg_globals_exist() {
        let rt = with_svg();
        for n in [
            "SVGAngle", "SVGNumber", "SVGNumberList", "SVGLengthList", "SVGAnimatedAngle",
            "SVGAnimatedNumberList", "SVGAnimatedLengthList", "SVGUnitTypes", "SVGAElement",
            "SVGMPathElement", "SVGFETurbulenceElement", "SVGFEDropShadowElement",
        ] {
            assert!(bool_eval(&rt, &format!("typeof window.{n} === 'function'")), "{n}");
        }
    }

    #[test]
    fn bug_1093_value_types_have_webidl_shape() {
        let rt = with_svg();
        assert!(bool_eval(&rt, r#"
            const throwsType = f => { try { f(); return false; } catch (e) { return e instanceof TypeError; } };
            const len = new SVGSVGElement().createSVGLength();
            const d = Object.getOwnPropertyDescriptor(SVGLength.prototype, 'value');
            len.valueAsString = '2cm';
            const list = new SVGSVGElement().createSVGTransform();
            Object.keys(globalThis).indexOf('SVGLength') < 0
              && d.enumerable && d.get.name === 'get value' && d.set.name === 'set value'
              && throwsType(() => d.get.call(SVGLength.prototype))
              && !Object.prototype.hasOwnProperty.call(len, 'value')
              && SVGLength.SVG_LENGTHTYPE_PX === 5 && SVGLength.prototype.SVG_LENGTHTYPE_PX === 5
              && Object.getOwnPropertyDescriptor(SVGLength.prototype, 'newValueSpecifiedUnits').enumerable
              && SVGLength.prototype.newValueSpecifiedUnits.length === 2
              && throwsType(() => len.newValueSpecifiedUnits())
              && throwsType(() => new SVGLength())
              && len.unitType === SVGLength.SVG_LENGTHTYPE_CM
              && Math.abs(len.value - 96 / 2.54 * 2) < 1e-9
              && Object.prototype.toString.call(len) === '[object SVGLength]'
              && Object.getOwnPropertyDescriptor(SVGTransform.prototype, 'type').set === undefined
              && list.type === SVGTransform.SVG_TRANSFORM_MATRIX
        "#));
    }

    #[test]
    fn bug_1093_list_operations_live_on_each_interface_prototype() {
        let rt = with_svg();
        assert!(bool_eval(&rt, r#"
            const sl = new SVGSVGElement().createSVGTransform();
            const names = ['SVGNumberList', 'SVGLengthList', 'SVGStringList', 'SVGPointList', 'SVGTransformList'];
            names.every(n => ['getItem', 'appendItem', 'clear', 'numberOfItems', 'length'].every(
              m => Object.prototype.hasOwnProperty.call(window[n].prototype, m)))
              && SVGStringList.prototype.getItem.length === 1
              && SVGStringList.prototype.insertItemBefore.length === 2
              && SVGTransformList.prototype.consolidate.length === 0
        "#));
    }

    #[test]
    fn bug_1092_factories_return_typed_instances() {
        let rt = with_svg();
        assert!(bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const n = svg.createSVGNumber(), a = svg.createSVGAngle();
            n.value = '2.5';
            a.valueAsString = '1rad';
            n instanceof SVGNumber && n.value === 2.5 && a instanceof SVGAngle
              && a.unitType === SVGAngle.SVG_ANGLETYPE_RAD
              && Math.abs(a.value - 180 / Math.PI) < 1e-9
              && SVGUnitTypes.SVG_UNIT_TYPE_OBJECTBOUNDINGBOX === 2
        "#));
    }

    #[test]
    fn svg_svg_element_class_exists() {
        let rt = with_svg();
        assert!(bool_eval(&rt, "typeof window.SVGSVGElement === 'function'"));
    }

    #[test]
    fn svg_graphics_element_get_bbox_returns_rect() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const el = new SVGGraphicsElement();
            const bb = el.getBBox();
            bb instanceof SVGRect && bb.x === 0 && bb.y === 0 &&
            bb.width === 0 && bb.height === 0
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_rect_element_has_dimensions() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const r = new SVGRectElement();
            r.x instanceof SVGAnimatedLength &&
            r.y instanceof SVGAnimatedLength &&
            r.width instanceof SVGAnimatedLength &&
            r.height instanceof SVGAnimatedLength
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_circle_element_has_cx_cy_r() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const c = new SVGCircleElement();
            c.cx instanceof SVGAnimatedLength &&
            c.cy instanceof SVGAnimatedLength &&
            c.r  instanceof SVGAnimatedLength
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_path_element_has_get_total_length() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const p = new SVGPathElement();
            typeof p.getTotalLength === 'function' && p.getTotalLength() === 0
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_svg_element_create_svg_rect() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const r = svg.createSVGRect();
            r instanceof SVGRect
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_svg_element_create_svg_point() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const p = svg.createSVGPoint();
            p instanceof SVGPoint && p.x === 0 && p.y === 0
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_matrix_multiply_identity() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const a = new SVGMatrix();
            const b = new SVGMatrix();
            const c = a.multiply(b);
            c instanceof SVGMatrix && c.a === 1 && c.d === 1 &&
            c.b === 0 && c.c === 0 && c.e === 0 && c.f === 0
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_transform_set_translate() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const t = new SVGSVGElement().createSVGTransform();
            t.setTranslate(10, 20);
            t.type === 2 && t.matrix.e === 10 && t.matrix.f === 20
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_create_element_ns_returns_typed_element() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
            el instanceof SVGCircleElement
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_create_element_ns_svg_returns_svg_svg_element() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
            el instanceof SVGSVGElement
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_create_element_ns_unknown_tag_returns_svg_element() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'unknown-tag');
            el instanceof SVGElement
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_point_matrix_transform() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const p = new SVGPoint(3, 4);
            const m = new SVGMatrix(2, 0, 0, 2, 1, 1); // scale(2) + translate(1,1)
            const p2 = p.matrixTransform(m);
            p2.x === 7 && p2.y === 9
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_length_unit_types() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            SVGLength.SVG_LENGTHTYPE_NUMBER === 1 &&
            SVGLength.SVG_LENGTHTYPE_PX     === 5 &&
            SVGLength.SVG_LENGTHTYPE_PERCENTAGE === 2
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_animated_transform_list_consolidate() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const atl = new SVGRectElement().transform;
            const t = new SVGSVGElement().createSVGTransform();
            t.setTranslate(5, 0);
            atl.baseVal.appendItem(t);
            const c = atl.baseVal.consolidate();
            c instanceof SVGTransform && c.matrix.e === 5
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_linear_gradient_element_x1_x2() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const lg = new SVGLinearGradientElement();
            lg.x1 instanceof SVGAnimatedLength &&
            lg.x2 instanceof SVGAnimatedLength &&
            lg.x2.baseVal.value === 100
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_filter_element_exists() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const f = new SVGFilterElement();
            typeof f.filterUnits === 'object' && f.tagName === 'filter'
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_text_element_get_number_of_chars() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            const t = new SVGTextElement();
            t.getNumberOfChars() === 0 && t.getComputedTextLength() === 0
        "#);
        assert!(ok);
    }

    #[test]
    fn svg_classes_on_window() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            typeof window.SVGRectElement       === 'function' &&
            typeof window.SVGCircleElement     === 'function' &&
            typeof window.SVGPathElement       === 'function' &&
            typeof window.SVGLineElement       === 'function' &&
            typeof window.SVGPolygonElement    === 'function' &&
            typeof window.SVGPolylineElement   === 'function' &&
            typeof window.SVGTextElement       === 'function' &&
            typeof window.SVGGElement          === 'function' &&
            typeof window.SVGDefsElement       === 'function' &&
            typeof window.SVGUseElement        === 'function' &&
            typeof window.SVGImageElement      === 'function' &&
            typeof window.SVGClipPathElement   === 'function' &&
            typeof window.SVGMaskElement       === 'function' &&
            typeof window.SVGLinearGradientElement === 'function' &&
            typeof window.SVGRadialGradientElement === 'function' &&
            typeof window.SVGFilterElement     === 'function' &&
            typeof window.SVGMarkerElement     === 'function'
        "#);
        assert!(ok);
    }

    // ── GAP-SMIL ─────────────────────────────────────────────────────────

    #[test]
    fn svg_smil_class_hierarchy() {
        let rt = with_svg();
        let ok = bool_eval(&rt, r#"
            (new SVGAnimateElement()) instanceof SVGAnimationElement &&
            (new SVGSetElement()) instanceof SVGAnimationElement &&
            (new SVGAnimateTransformElement()) instanceof SVGAnimationElement &&
            (new SVGAnimateMotionElement()) instanceof SVGAnimationElement &&
            typeof SVGAnimationElement.prototype.beginElement === 'function' &&
            typeof SVGAnimationElement.prototype.beginElementAt === 'function' &&
            typeof SVGAnimationElement.prototype.endElement === 'function' &&
            typeof SVGAnimationElement.prototype.endElementAt === 'function'
        "#);
        assert!(ok);
    }

    /// Installs the SMIL-capable native stubs `_lumen_tick_smil` needs
    /// (`_lumen_get_attr`/`_lumen_u2n`/`_lumen_dispatch`,
    /// `document.getElementsByTagName('*')`) on top of `with_svg()`, then
    /// builds one `<animate>`-shaped node with `__nid__ = 1` reachable
    /// through that stub. Attributes are set via `_lumen_smil_set_attr`
    /// (a test-only helper, not a real DOM method) to avoid re-implementing
    /// attribute reflection in the stub.
    fn with_smil_node(local: &str, attrs: &[(&str, &str)]) -> V8JsRuntime {
        let rt = with_svg();
        let attrs_js: String = attrs
            .iter()
            .map(|(k, v)| format!("{k:?}:{v:?}"))
            .collect::<Vec<_>>()
            .join(",");
        rt.eval(&format!(
            r#"
            class Event {{
                constructor(type, opts) {{
                    this.type = type;
                    this.bubbles = !!(opts && opts.bubbles);
                    this.cancelable = !!(opts && opts.cancelable);
                }}
            }}
            window.Event = Event;
            __lumen_C._lumen_smil_attrs = {{1: {{{attrs_js}}}}};
            __lumen_C._lumen_get_attr = function(nid, attr) {{
                var a = __lumen_C._lumen_smil_attrs[nid];
                return (a && Object.prototype.hasOwnProperty.call(a, attr)) ? a[attr] : undefined;
            }};
            __lumen_C._lumen_u2n = function(v) {{ return v === undefined ? null : v; }};
            __lumen_C._lumen_dispatch_log = [];
            __lumen_C._lumen_dispatch = function(nid, event) {{ __lumen_C._lumen_dispatch_log.push(event.type); return true; }};
            var node = new (_lumen_svg_ctor_for_local({local:?}))();
            node.__nid__ = 1;
            __lumen_C._lumen_smil_node = node;
            var _allEls = [node];
            document.getElementsByTagName = function(tag) {{ return _allEls; }};
            "#
        ))
        .unwrap();
        rt
    }

    #[test]
    fn svg_smil_full_and_partial_clock_values() {
        let cases = [
            ("00:00:01.50", "1.5"), ("00:01.50", "1.5"), ("00:30:01", "1801"),
            ("101:00:01", "363601"), ("2min", "120"), ("1h", "3600"), ("500ms", "0.5"),
        ];
        for (raw, secs) in cases {
            let rt = with_smil_node("animate", &[("attributeName", "x"), ("dur", raw)]);
            assert!(
                bool_eval(&rt, &format!("_lumen_smil_node.getSimpleDuration() === {secs}")),
                "dur={raw}"
            );
        }
        for bad in ["01:99:01", "99:01", "00:59:59.", "00:59:9.9", "00:59:.9", "00:59:009", ":30:01", "01::01", "5:30"] {
            let rt = with_smil_node("animate", &[("attributeName", "x"), ("dur", bad)]);
            assert!(bool_eval(&rt, "(function(){ try { _lumen_smil_node.getSimpleDuration(); return false; } catch (e) { return true; } })()"), "dur={bad}");
        }
    }

    #[test]
    fn svg_smil_begin_end_events_and_numeric_interpolation() {
        // `<animate attributeName="width" begin="0s" dur="2s" from="0" to="100">`
        let rt = with_smil_node(
            "animate",
            &[
                ("attributeName", "width"),
                ("begin", "0s"),
                ("dur", "2s"),
                ("from", "0"),
                ("to", "100"),
            ],
        );
        // t = 0s: begin fires, value starts at "0".
        rt.eval("_lumen_tick_smil(0.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('beginEvent') !== -1"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '0'"));

        // t = 1s: halfway through the 2s duration, linear interpolation.
        rt.eval("_lumen_tick_smil(1.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '50'"));

        // t = 2s: duration elapsed, endEvent fires, fill="remove" (default)
        // clears the override.
        rt.eval("_lumen_tick_smil(2.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === undefined"));
    }

    #[test]
    fn svg_smil_fill_freeze_keeps_value_after_end() {
        let rt = with_smil_node(
            "set",
            &[("attributeName", "visibility"), ("begin", "0s"), ("to", "visible"), ("end", "1s"), ("fill", "freeze")],
        );
        rt.eval("_lumen_tick_smil(0.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|visibility'] === 'visible'"));
        rt.eval("_lumen_tick_smil(1.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|visibility'] === 'visible'"));
    }

    #[test]
    fn svg_smil_indefinite_begin_waits_for_begin_element_call() {
        let rt = with_smil_node("set", &[("attributeName", "width"), ("begin", "indefinite"), ("to", "100")]);
        rt.eval("_lumen_tick_smil(5.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.length === 0"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === undefined"));
        rt.eval("__lumen_C._lumen_smil_node.beginElement(); _lumen_tick_smil(5.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('beginEvent') !== -1"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '100'"));
    }

    #[test]
    fn svg_smil_repeat_count_fires_repeat_event() {
        let rt = with_smil_node(
            "animate",
            &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("repeatCount", "3"), ("to", "10")],
        );
        rt.eval("_lumen_tick_smil(0.0);").unwrap();
        rt.eval("_lumen_tick_smil(1.5);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('repeatEvent') !== -1"));
        // Still active (2nd of 3 cycles) — no endEvent yet.
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') === -1"));
        rt.eval("_lumen_tick_smil(3.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
    }

    #[test]
    fn svg_smil_seek_silently_skips_past_intervals_and_dispatches_transitions() {
        let rt = with_smil_node(
            "set",
            &[("attributeName", "fill"), ("begin", "5s"), ("dur", "1s"), ("repeatCount", "2"), ("to", "green"), ("fill", "freeze")],
        );
        rt.eval(
            r#"
            var svg = new (_lumen_svg_ctor_for_local("svg"))();
            svg.pauseAnimations();
            _lumen_tick_smil(100.0);
            svg.setCurrentTime(10);
            _lumen_tick_smil(100.1);
            "#,
        )
        .unwrap();
        // Interval 5..7 lies wholly before t=10: no events, freeze value kept.
        assert!(bool_eval(&rt, "_lumen_dispatch_log.length === 0"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|fill'] === 'green'"));
        rt.eval("svg.setCurrentTime(5.5); _lumen_tick_smil(100.2);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent'"));
        // Seeking into the second cycle: no repeatEvent, no new events.
        rt.eval("svg.setCurrentTime(6.5); _lumen_tick_smil(100.3);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent'"));
        rt.eval("svg.setCurrentTime(1); _lumen_tick_smil(100.4);").unwrap();
        assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent,endEvent'"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|fill'] === undefined"));
    }

    #[test]
    fn svg_smil_min_max_clamp_active_duration() {
        // dur 1s, fill freeze: `max=0.5s` ends at 0.5s, `min=3s` at 3s.
        for (attr, v, end_tick, still_active) in [("max", "0.5s", 0.6, false), ("min", "3s", 2.0, true)] {
            let rt = with_smil_node(
                "animate",
                &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("to", "10"), (attr, v)],
            );
            rt.eval("_lumen_tick_smil(0.0); _lumen_tick_smil(0.0);").unwrap();
            rt.eval(&format!("_lumen_tick_smil({end_tick});")).unwrap();
            let ended = bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1");
            assert_eq!(ended, !still_active, "{attr}={v}");
        }
    }

    #[test]
    fn svg_smil_repeat_n_syncbase_reacts_to_that_iteration_only() {
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "indefinite"), ("to", "1")]);
        rt.eval(
            r#"
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.repeat(2)", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("set"))();
            b.__nid__ = 2;
            _allEls.push(b);
            var a = __lumen_C._lumen_smil_node;
            var listeners = {};
            a.addEventListener = function(n, f) { listeners[n] = f; };
            document.getElementById = function(id) { return id === 'a' ? a : null; };
            _lumen_tick_smil(0.0);
            listeners.repeatEvent({ detail: 1 });
            _lumen_tick_smil(1.0);
            "#,
        )
        .unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
        rt.eval("listeners.repeatEvent({ detail: 2 }); _lumen_tick_smil(2.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
    }

    #[test]
    fn svg_smil_syncbase_end_plus_offset_and_event_begin() {
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("to", "10")]);
        rt.eval(
            r#"
            var a = __lumen_C._lumen_smil_node;
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.end+1s", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("set"))();
            b.__nid__ = 2;
            _allEls.push(b);
            __lumen_C._lumen_smil_attrs[3] = {attributeName: "z", begin: "a.fooEvent", to: "9"};
            var c = new (_lumen_svg_ctor_for_local("set"))();
            c.__nid__ = 3;
            _allEls.push(c);
            var listeners = {};
            a.addEventListener = function(n, f) { listeners[n] = f; };
            document.getElementById = function(id) { return id === 'a' ? a : null; };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(1.0);
            "#,
        )
        .unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
        rt.eval("_lumen_tick_smil(1.5);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
        rt.eval("_lumen_tick_smil(2.0);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
        // Event-based begin waits for the listener to fire.
        assert!(bool_eval(&rt, "_lumen_smil_overrides['3|z'] === undefined"));
        rt.eval("listeners.fooEvent(); _lumen_tick_smil(2.5);").unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['3|z'] === '9'"));
    }

    #[test]
    fn svg_smil_restart_end_syncbase_and_cycle() {
        // `begin="0;2s"` dur 5s: the second begin instance restarts the first
        // interval at 2s (restart=always), so a begin/end pair fires at 2s.
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "0;2s"), ("dur", "5s"), ("from", "0"), ("to", "10")]);
        rt.eval(
            r#"
            var a = __lumen_C._lumen_smil_node;
            var log = [];
            a.addEventListener = function(n, f) {};
            _lumen_dispatch = function(nid, ev) { log.push(nid + ':' + ev.type); };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(3.0);
            "#,
        )
        .unwrap();
        assert!(bool_eval(&rt, "log.join() === '1:beginEvent,1:endEvent,1:beginEvent'"));
        assert!(bool_eval(&rt, "_lumen_smil_overrides['1|x'] === '2'"));
        // Cyclic syncbase (`a.begin` ↔ `b.begin`) must not hang the sweep.
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "b.begin; 0s"), ("dur", "1s"), ("to", "1")]);
        rt.eval(
            r#"
            var a = __lumen_C._lumen_smil_node;
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.begin", dur: "1s", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("animate"))();
            b.__nid__ = 2;
            _allEls.push(b);
            document.getElementById = function(id) { return id === 'a' ? a : id === 'b' ? b : null; };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(0.5);
            "#,
        )
        .unwrap();
        assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
    }

    #[test]
    fn svg_smil_huge_repeat_count_is_unspecified() {
        let big = format!("1{}", "0".repeat(300));
        let rt = with_smil_node("animate", &[("attributeName", "fill"), ("begin", "0s"), ("dur", "10ms"), ("from", "#007f00"), ("to", "green"), ("fill", "freeze"), ("repeatCount", big.as_str())]);
        rt.eval(
            r#"
            var log = [];
            _lumen_dispatch = function(nid, ev) { log.push(ev.type); };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(0.5);
            "#,
        )
        .unwrap();
        assert!(bool_eval(&rt, "log.join() === 'beginEvent,endEvent'"));
    }
}
