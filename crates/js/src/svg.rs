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
const SVG_SHIM: &str = r#"
(function() {
  'use strict';

  const SVG_NS = 'http://www.w3.org/2000/svg';

  // ── Value types ──────────────────────────────────────────────────────────

  // SVGRect — axis-aligned bounding box in local coordinate space
  class SVGRect {
    constructor(x, y, w, h) {
      this.x = x || 0; this.y = y || 0;
      this.width = w || 0; this.height = h || 0;
    }
  }
  window.SVGRect = SVGRect;

  // SVGPoint — 2-D point; matrixTransform() returns a new SVGPoint
  class SVGPoint {
    constructor(x, y) { this.x = x || 0; this.y = y || 0; }
    matrixTransform(matrix) {
      const m = matrix || {};
      return new SVGPoint(
        (m.a || 1) * this.x + (m.c || 0) * this.y + (m.e || 0),
        (m.b || 0) * this.x + (m.d || 1) * this.y + (m.f || 0)
      );
    }
  }
  window.SVGPoint = SVGPoint;

  // SVGLength — scalar length with unit type
  class SVGLength {
    constructor(v) {
      this.value = v || 0;
      this.valueInSpecifiedUnits = v || 0;
      this.valueAsString = String(v || 0);
      this.unitType = 1; // SVG_LENGTHTYPE_NUMBER
    }
    convertToSpecifiedUnits(unitType) { this.unitType = unitType; }
    newValueSpecifiedUnits(unitType, value) {
      this.unitType = unitType;
      this.value = value;
      this.valueInSpecifiedUnits = value;
      this.valueAsString = String(value);
    }
  }
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
    constructor(v) {
      this.baseVal = new SVGLength(v);
      this.animVal = new SVGLength(v);
    }
  }
  window.SVGAnimatedLength = SVGAnimatedLength;

  // SVGAnimatedString — pair of base/animated string values
  class SVGAnimatedString {
    constructor(s) { this.baseVal = s || ''; this.animVal = s || ''; }
  }
  window.SVGAnimatedString = SVGAnimatedString;

  // SVGStringList — ordered list of strings
  class SVGStringList {
    constructor() { this._items = []; this.length = 0; }
    initialize(str) { this._items = [str]; this.length = 1; return str; }
    getItem(i) { return this._items[i]; }
    appendItem(str) { this._items.push(str); this.length = this._items.length; return str; }
    removeItem(i) {
      const r = this._items.splice(i, 1)[0];
      this.length = this._items.length;
      return r;
    }
    clear() { this._items = []; this.length = 0; }
  }
  window.SVGStringList = SVGStringList;

  // SVGAnimatedBoolean
  class SVGAnimatedBoolean {
    constructor(v) { this.baseVal = !!v; this.animVal = !!v; }
  }
  window.SVGAnimatedBoolean = SVGAnimatedBoolean;

  // SVGAnimatedEnumeration
  class SVGAnimatedEnumeration {
    constructor(v) { this.baseVal = v || 0; this.animVal = v || 0; }
  }
  window.SVGAnimatedEnumeration = SVGAnimatedEnumeration;

  // SVGAnimatedInteger
  class SVGAnimatedInteger {
    constructor(v) { this.baseVal = v || 0; this.animVal = v || 0; }
  }
  window.SVGAnimatedInteger = SVGAnimatedInteger;

  // SVGAnimatedNumber
  class SVGAnimatedNumber {
    constructor(v) { this.baseVal = v || 0; this.animVal = v || 0; }
  }
  window.SVGAnimatedNumber = SVGAnimatedNumber;

  // SVGAnimatedRect — pair of base/animated SVGRect values
  class SVGAnimatedRect {
    constructor() {
      this.baseVal = new SVGRect(); this.animVal = new SVGRect();
    }
  }
  window.SVGAnimatedRect = SVGAnimatedRect;

  // SVGMatrix (legacy, before DOMMatrix) — 2-D affine transform [a b c d e f]
  class SVGMatrix {
    constructor(a,b,c,d,e,f) {
      this.a = a!=null?a:1; this.b = b!=null?b:0;
      this.c = c!=null?c:0; this.d = d!=null?d:1;
      this.e = e!=null?e:0; this.f = f!=null?f:0;
    }
    multiply(m) {
      return new SVGMatrix(
        this.a*m.a+this.c*m.b, this.b*m.a+this.d*m.b,
        this.a*m.c+this.c*m.d, this.b*m.c+this.d*m.d,
        this.a*m.e+this.c*m.f+this.e, this.b*m.e+this.d*m.f+this.f
      );
    }
    inverse() { return new SVGMatrix(); }
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

  // SVGTransform — single transform component
  class SVGTransform {
    constructor() {
      this.type = 1; // SVG_TRANSFORM_MATRIX
      this.matrix = new SVGMatrix();
      this.angle = 0;
    }
    setMatrix(m) { this.type = 1; this.matrix = m; }
    setTranslate(tx,ty) {
      this.type = 2;
      this.matrix = new SVGMatrix(1,0,0,1,tx,ty);
    }
    setScale(sx,sy) {
      this.type = 3;
      this.matrix = new SVGMatrix(sx,0,0,sy,0,0);
    }
    setRotate(a,cx,cy) {
      this.type = 4; this.angle = a;
      const r=a*Math.PI/180, cos=Math.cos(r), sin=Math.sin(r);
      cx=cx||0; cy=cy||0;
      this.matrix = new SVGMatrix(cos,sin,-sin,cos,
        (1-cos)*cx+sin*cy, (1-cos)*cy-sin*cx);
    }
    setSkewX(a) { this.type = 5; this.angle = a; }
    setSkewY(a) { this.type = 6; this.angle = a; }
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
    constructor() { this._items = []; this.length = 0; }
    get numberOfItems() { return this._items.length; }
    clear() { this._items = []; this.length = 0; }
    initialize(t) { this._items = [t]; this.length = 1; return t; }
    getItem(i) { return this._items[i]; }
    insertItemBefore(t,i) { this._items.splice(i,0,t); this.length=this._items.length; return t; }
    replaceItem(t,i) { this._items[i]=t; return t; }
    removeItem(i) { const r=this._items.splice(i,1)[0]; this.length=this._items.length; return r; }
    appendItem(t) { this._items.push(t); this.length=this._items.length; return t; }
    consolidate() {
      const t = new SVGTransform();
      t.type = 1;
      t.matrix = this._items.reduce((acc, x) => acc.multiply(x.matrix), new SVGMatrix());
      this._items = [t]; this.length = 1;
      return t;
    }
    createSVGTransformFromMatrix(m) { const t=new SVGTransform(); t.setMatrix(m); return t; }
  }
  window.SVGTransformList = SVGTransformList;

  // SVGAnimatedTransformList
  class SVGAnimatedTransformList {
    constructor() {
      this.baseVal = new SVGTransformList();
      this.animVal = new SVGTransformList();
    }
  }
  window.SVGAnimatedTransformList = SVGAnimatedTransformList;

  // SVGPointList
  class SVGPointList {
    constructor() { this._items = []; this.length = 0; }
    get numberOfItems() { return this._items.length; }
    clear() { this._items = []; this.length = 0; }
    initialize(p) { this._items = [p]; this.length = 1; return p; }
    getItem(i) { return this._items[i]; }
    appendItem(p) { this._items.push(p); this.length=this._items.length; return p; }
    removeItem(i) { const r=this._items.splice(i,1)[0]; this.length=this._items.length; return r; }
  }
  window.SVGPointList = SVGPointList;

  // ── Attribute reflection (GAP-SVGDOM) ───────────────────────────────────
  // A real SVG element (parser-built or from `createElementNS`) never runs
  // an ES class constructor — `_lumen_build_element` (web_api_shim_mid.js)
  // makes it with `Object.create(prototype)`, only re-pointing at the typed
  // `SVG*Element` prototype (BUG-889). Every field the constructors below
  // used to set (`this.x = new SVGAnimatedLength(0)`, …) was therefore dead
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
    var length = new SVGLength(_lumen_svg_length_value(nid, attr, dflt));
    Object.defineProperty(length, 'value', {
      get: function() { return _lumen_svg_length_value(nid, attr, dflt); },
      set: function(v) { if (nid != null) _lumen_set_attr(nid, attr, String(v)); },
      enumerable: true, configurable: true,
    });
    Object.defineProperty(length, 'valueAsString', {
      get: function() { return String(this.value); },
      set: function(v) { this.value = parseFloat(v); },
      enumerable: true, configurable: true,
    });
    return length;
  }

  function _lumen_svg_animated_length(nid, attr, dflt) {
    var al = Object.create(SVGAnimatedLength.prototype);
    al.baseVal = _lumen_svg_reflected_length(nid, attr, dflt);
    // GAP-SMIL: `animVal` is `baseVal` unless a running `<animate>`/`<set>`
    // targeting this exact attribute has a value queued in the SMIL override
    // map (`_lumen_smil_overrides`, populated by `_lumen_tick_smil`) — the
    // override never touches the content attribute, so `getAttribute`/
    // `baseVal` stay unaffected, matching the animVal/baseVal split SVG 2 §3
    // requires. No override map yet (SMIL never ticked) reads as `undefined`.
    Object.defineProperty(al, 'animVal', {
      get: function() {
        var ov = (typeof _lumen_smil_overrides !== 'undefined')
          ? _lumen_smil_overrides[nid + '|' + attr] : undefined;
        if (ov !== undefined) return new SVGLength(_lumen_svg_parse_number(ov, dflt));
        return al.baseVal;
      },
      enumerable: true, configurable: true,
    });
    return al;
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
    var ar = Object.create(SVGAnimatedRect.prototype);
    var v = _lumen_svg_parse_viewbox(nid);
    var rect = new SVGRect(v[0], v[1], v[2], v[3]);
    ar.baseVal = rect; ar.animVal = rect;
    return ar;
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
    var list = new SVGTransformList();
    var s = nid != null ? _lumen_u2n(_lumen_get_attr(nid, attr)) : null;
    if (!s) return list;
    var re = /(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^)]*)\)/g;
    var m;
    while ((m = re.exec(s))) {
      var fn = m[1];
      var args = m[2].trim().split(/[\s,]+/).filter(function(x) { return x !== ''; })
        .map(function(x) { return parseFloat(x); });
      var t = new SVGTransform();
      if (fn === 'matrix' && args.length === 6) {
        t.type = SVGTransform.SVG_TRANSFORM_MATRIX;
        t.matrix = new SVGMatrix(args[0], args[1], args[2], args[3], args[4], args[5]);
      } else if (fn === 'translate') {
        t.setTranslate(args[0] || 0, args[1] || 0);
      } else if (fn === 'scale') {
        var sx = args[0] != null ? args[0] : 1;
        var sy = args.length > 1 ? args[1] : sx;
        t.type = SVGTransform.SVG_TRANSFORM_SCALE;
        t.matrix = new SVGMatrix(sx, 0, 0, sy, 0, 0);
      } else if (fn === 'rotate') {
        t.setRotate(args[0] || 0, args[1], args[2]);
      } else if (fn === 'skewX') {
        t.type = SVGTransform.SVG_TRANSFORM_SKEWX; t.angle = args[0] || 0;
        t.matrix = new SVGMatrix(1, 0, Math.tan((args[0] || 0) * Math.PI / 180), 1, 0, 0);
      } else if (fn === 'skewY') {
        t.type = SVGTransform.SVG_TRANSFORM_SKEWY; t.angle = args[0] || 0;
        t.matrix = new SVGMatrix(1, Math.tan((args[0] || 0) * Math.PI / 180), 0, 1, 0, 0);
      } else {
        continue;
      }
      list.appendItem(t);
    }
    return list;
  }

  function _lumen_svg_animated_transform_list(nid, attr) {
    var atl = Object.create(SVGAnimatedTransformList.prototype);
    var list = _lumen_svg_parse_transform_list(nid, attr);
    atl.baseVal = list; atl.animVal = list;
    return atl;
  }

  function _lumen_def_svg_transform(Ctor, prop, attr) {
    Object.defineProperty(Ctor.prototype, prop, {
      get: function() { return _lumen_svg_animated_transform_list(this.__nid__, attr); },
      enumerable: true, configurable: true,
    });
  }

  // `points` (SVG L1 §9.7.1 `<list-of-points>`): "x1,y1 x2,y2 …".
  function _lumen_svg_parse_points(nid) {
    var list = new SVGPointList();
    var s = nid != null ? _lumen_u2n(_lumen_get_attr(nid, 'points')) : null;
    if (!s) return list;
    var nums = s.trim().split(/[\s,]+/).filter(function(x) { return x !== ''; })
      .map(function(x) { return parseFloat(x); });
    for (var i = 0; i + 1 < nums.length; i += 2) {
      list.appendItem(new SVGPoint(nums[i], nums[i + 1]));
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
      return new SVGRect(0, 0, 0, 0);
    }

    // Phase 0: returns identity matrix
    getCTM() { return new SVGMatrix(); }
    getScreenCTM() { return new SVGMatrix(); }

    getTransformToElement(element) { return new SVGMatrix(); }
  }
  _lumen_def_svg_transform(SVGGraphicsElement, 'transform', 'transform');
  window.SVGGraphicsElement = SVGGraphicsElement;

  // SVGGeometryElement — adds pathLength, getTotalLength, getPointAtLength, isPointInFill/Stroke
  class SVGGeometryElement extends SVGGraphicsElement {
    constructor() {
      super();
      this.pathLength = new SVGAnimatedNumber(0);
    }
    getTotalLength() { return 0; }
    getPointAtLength(distance) { return new SVGPoint(0, 0); }
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
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
      this.currentScale = 1;
      this.currentTranslate = new SVGPoint(0, 0);
      this.contentScriptType = 'text/ecmascript';
      this.contentStyleType = 'text/css';
    }

    createSVGRect()   { return new SVGRect(); }
    createSVGPoint()  { return new SVGPoint(); }
    createSVGLength() { return new SVGLength(); }
    createSVGMatrix() { return new SVGMatrix(); }
    createSVGTransform() { return new SVGTransform(); }
    createSVGTransformFromMatrix(m) {
      const t = new SVGTransform(); t.setMatrix(m); return t;
    }
    createSVGNumber() { return { value: 0 }; }
    createSVGAngle()  { return { value: 0, unitType: 1, valueInSpecifiedUnits: 0, valueAsString: '0' }; }

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
    pauseAnimations() {}
    unpauseAnimations() {}
    animationsPaused() { return false; }
    getCurrentTime() { return 0; }
    setCurrentTime(seconds) {}
  }
  _lumen_def_svg_lengths(SVGSVGElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 300], height: ['height', 150],
  });
  _lumen_def_svg_viewbox(SVGSVGElement);
  window.SVGSVGElement = SVGSVGElement;

  // SVGAnimatedPreserveAspectRatio (needed by SVGSVGElement)
  class SVGPreserveAspectRatio {
    constructor() { this.align = 8; this.meetOrSlice = 1; }
  }
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
    constructor() {
      this.baseVal = new SVGPreserveAspectRatio();
      this.animVal = new SVGPreserveAspectRatio();
    }
  }
  window.SVGAnimatedPreserveAspectRatio = SVGAnimatedPreserveAspectRatio;

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
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
    }
  }
  _lumen_def_svg_viewbox(SVGSymbolElement);
  window.SVGSymbolElement = SVGSymbolElement;

  // SVGUseElement — <use>
  class SVGUseElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'use';
      this.href = new SVGAnimatedString('');
    }
  }
  _lumen_def_svg_lengths(SVGUseElement, {
    x: ['x', 0], y: ['y', 0], width: ['width', 0], height: ['height', 0],
  });
  window.SVGUseElement = SVGUseElement;

  // SVGImageElement — <image>
  class SVGImageElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'image';
      this.href = new SVGAnimatedString('');
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
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
      return new SVGRect(this.x.baseVal.value, this.y.baseVal.value,
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
      return new SVGRect(cx - r, cy - r, 2 * r, 2 * r);
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
      return new SVGRect(cx - rx, cy - ry, 2 * rx, 2 * ry);
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
      return new SVGRect(x, y, Math.abs(x2 - x1), Math.abs(y2 - y1));
    }
  }
  _lumen_def_svg_lengths(SVGLineElement, {
    x1: ['x1', 0], y1: ['y1', 0], x2: ['x2', 0], y2: ['y2', 0],
  });
  window.SVGLineElement = SVGLineElement;

  // Shared `points`-based bbox (SVG 2 §10.6.2) for polyline/polygon.
  function _lumen_svg_points_bbox(el) {
    var pts = el.points._items;
    if (!pts.length) return new SVGRect(0, 0, 0, 0);
    var minX = pts[0].x, maxX = pts[0].x, minY = pts[0].y, maxY = pts[0].y;
    for (var i = 1; i < pts.length; i++) {
      minX = Math.min(minX, pts[i].x); maxX = Math.max(maxX, pts[i].x);
      minY = Math.min(minY, pts[i].y); maxY = Math.max(maxY, pts[i].y);
    }
    return new SVGRect(minX, minY, maxX - minX, maxY - minY);
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
      this.textLength = new SVGAnimatedLength(0);
      this.lengthAdjust = new SVGAnimatedEnumeration(1); // spacingAndGlyphs
    }
    getNumberOfChars() { return 0; }
    getComputedTextLength() { return 0; }
    getSubStringLength(charNum, nChars) { return 0; }
    getStartPositionOfChar(charNum) { return new SVGPoint(); }
    getEndPositionOfChar(charNum) { return new SVGPoint(); }
    getExtentOfChar(charNum) { return new SVGRect(); }
    getRotationOfChar(charNum) { return 0; }
    getCharNumAtPosition(point) { return -1; }
    selectSubString(charNum, nChars) {}
  }
  window.SVGTextContentElement = SVGTextContentElement;

  // SVGTextPositioningElement — adds x/y/dx/dy/rotate
  class SVGTextPositioningElement extends SVGTextContentElement {
    constructor() {
      super();
      this.rotate = new SVGAnimatedInteger(0);
    }
  }
  _lumen_def_svg_lengths(SVGTextPositioningElement, {
    x: ['x', 0], y: ['y', 0], dx: ['dx', 0], dy: ['dy', 0],
  });
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
      this.method = new SVGAnimatedEnumeration(1);
      this.spacing = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
    }
  }
  _lumen_def_svg_lengths(SVGTextPathElement, { startOffset: ['startOffset', 0] });
  window.SVGTextPathElement = SVGTextPathElement;

  // SVGClipPathElement — <clipPath>
  class SVGClipPathElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'clipPath';
      this.clipPathUnits = new SVGAnimatedEnumeration(1);
    }
  }
  _lumen_def_svg_transform(SVGClipPathElement, 'transform', 'transform');
  window.SVGClipPathElement = SVGClipPathElement;

  // SVGMaskElement — <mask>
  class SVGMaskElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'mask';
      this.maskUnits = new SVGAnimatedEnumeration(2);
      this.maskContentUnits = new SVGAnimatedEnumeration(1);
    }
  }
  _lumen_def_svg_lengths(SVGMaskElement, {
    x: ['x', -10], y: ['y', -10], width: ['width', 120], height: ['height', 120],
  });
  window.SVGMaskElement = SVGMaskElement;

  // SVGGradientElement — base for gradient elements
  class SVGGradientElement extends SVGElement {
    constructor() {
      super();
      this.gradientUnits = new SVGAnimatedEnumeration(2);
      this.spreadMethod = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
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
      this.offset = new SVGAnimatedNumber(0);
    }
  }
  window.SVGStopElement = SVGStopElement;

  // SVGPatternElement — <pattern>
  class SVGPatternElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'pattern';
      this.patternUnits = new SVGAnimatedEnumeration(2);
      this.patternContentUnits = new SVGAnimatedEnumeration(1);
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
      this.href = new SVGAnimatedString('');
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
      this.markerUnits = new SVGAnimatedEnumeration(2);
      this.orientType = new SVGAnimatedEnumeration(1);
      this.orientAngle = new SVGAnimatedNumber(0);
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
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
      this.filterUnits = new SVGAnimatedEnumeration(2);
      this.primitiveUnits = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
    }
  }
  _lumen_def_svg_lengths(SVGFilterElement, {
    x: ['x', -10], y: ['y', -10], width: ['width', 120], height: ['height', 120],
  });
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
      this.in1 = new SVGAnimatedString('');
      this.stdDeviationX = new SVGAnimatedNumber(0);
      this.stdDeviationY = new SVGAnimatedNumber(0);
    }
    setStdDeviation(sdx, sdy) {
      this.stdDeviationX.baseVal = sdx;
      this.stdDeviationY.baseVal = sdy != null ? sdy : sdx;
    }
  }
  window.SVGFEGaussianBlurElement = SVGFEGaussianBlurElement;

  // SVGFEOffsetElement — <feOffset>
  class SVGFEOffsetElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'feOffset';
      this.dx = new SVGAnimatedNumber(0);
      this.dy = new SVGAnimatedNumber(0);
    }
  }
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
  // Deliberately out of scope: syncbase/event/repeat begin-value forms
  // (`id.end`, `id.repeat(2)`, `click`), `min`/`max`, `restart`,
  // <animateMotion> path following, <animateTransform> matrix composition —
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
  var _lumen_smil_last_now = 0;
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
  window._lumen_smil_overrides = _lumen_smil_overrides;

  // Minimal SMIL clock-value grammar: a plain number (seconds) or one with
  // an `s`/`ms` suffix. `min`/`h`/`:`-clock forms are out of scope.
  function _lumen_smil_parse_clock(tok) {
    if (tok == null) return null;
    var m = /^([+-]?[0-9]*\.?[0-9]+)(ms|s)?$/.exec(String(tok).trim());
    if (!m) return null;
    var n = parseFloat(m[1]);
    if (isNaN(n)) return null;
    return m[2] === 'ms' ? n / 1000 : n;
  }

  // `begin` — first comma-separated token only (multiple begin instances are
  // out of scope). Absent → spec default `0s`. `indefinite` or any
  // unsupported (syncbase/event/repeat) form → `null`, meaning "only
  // `beginElement()`/`beginElementAt()` can start this animation".
  function _lumen_smil_parse_begin_offset(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'begin'));
    if (raw == null || raw.trim() === '') return 0;
    var tok = raw.split(',')[0].trim();
    if (tok === 'indefinite') return null;
    return _lumen_smil_parse_clock(tok);
  }

  // `end` — first token, resolved as an absolute document-timeline instant
  // (not spec-accurate for the general case, but matches the common
  // "explicit cutoff independent of begin" usage, e.g. `end="2s"`).
  function _lumen_smil_parse_end_offset(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'end'));
    if (raw == null || raw.trim() === '') return null;
    return _lumen_smil_parse_clock(raw.split(',')[0].trim());
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
    return (isNaN(n) || n <= 0) ? 1 : n;
  }

  function _lumen_smil_parse_fill(nid) {
    return _lumen_u2n(_lumen_get_attr(nid, 'fill')) === 'freeze' ? 'freeze' : 'remove';
  }

  // Builds the value list to animate across from `values` (semicolon list)
  // or `from`/`to` (falling back to a single-value `to`-only list).
  function _lumen_smil_value_list(nid) {
    var raw = _lumen_u2n(_lumen_get_attr(nid, 'values'));
    if (raw != null) {
      return raw.split(';').map(function(s) { return s.trim(); });
    }
    var toRaw = _lumen_u2n(_lumen_get_attr(nid, 'to'));
    var fromRaw = _lumen_u2n(_lumen_get_attr(nid, 'from'));
    if (toRaw == null) return null;
    return fromRaw != null ? [fromRaw, toRaw] : [toRaw];
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
        beginTime: null, resolvedOnce: false, cycle: 0, ended: false,
        beginFired: false, manualBegin: null, manualEnd: null,
      };
      _lumen_smil_states[nid] = st;
    }
    return st;
  }

  // `beginElementAt`/`endElementAt` (SVG SMIL Animation §3.4): queue an
  // instance time resolved against "now" on the *next* tick, matching the
  // spec's "current time + offset" semantics closely enough for the
  // explicit-trigger case (`begin="indefinite"` + a script call).
  function _lumen_smil_begin_now(nid, offset) { _lumen_smil_get_state(nid).manualBegin = offset || 0; }
  function _lumen_smil_end_now(nid, offset) { _lumen_smil_get_state(nid).manualEnd = offset || 0; }

  function _lumen_smil_dispatch(nid, type) {
    _lumen_dispatch(nid, new Event(type, { bubbles: false, cancelable: false }));
  }

  function _lumen_smil_tick_one(el, now_s, epoch) {
    var nid = el.__nid__;
    if (nid == null) return;
    var st = _lumen_smil_get_state(nid);

    if (st.manualBegin !== null) {
      var mb = now_s + st.manualBegin;
      st.manualBegin = null;
      if (st.beginTime === null || st.ended) {
        st.beginTime = mb; st.ended = false; st.cycle = 0; st.beginFired = false;
      }
    }
    if (st.beginTime === null && !st.resolvedOnce) {
      st.resolvedOnce = true;
      var off = _lumen_smil_parse_begin_offset(nid);
      if (off !== null) st.beginTime = epoch + off;
    }
    var manualEndNow = null;
    if (st.manualEnd !== null) {
      manualEndNow = now_s + st.manualEnd;
      st.manualEnd = null;
    }

    if (st.beginTime === null || st.ended || now_s < st.beginTime) return;

    if (!st.beginFired) {
      st.beginFired = true;
      _lumen_smil_dispatch(nid, 'beginEvent');
    }

    var dur = _lumen_smil_parse_dur(nid);
    var repeatCount = _lumen_smil_parse_repeat_count(nid);
    var endAttrOff = _lumen_smil_parse_end_offset(nid);
    var endAttrAbs = endAttrOff === null ? Infinity : (epoch + endAttrOff);
    var activeDur = (dur === Infinity || repeatCount === Infinity) ? Infinity : dur * repeatCount;
    var naturalEnd = (activeDur === Infinity) ? Infinity : st.beginTime + activeDur;
    var effectiveEnd = Math.min(naturalEnd, endAttrAbs, manualEndNow === null ? Infinity : manualEndNow);
    var elapsed = now_s - st.beginTime;

    if (dur !== Infinity) {
      var completedCycles = Math.floor(elapsed / dur);
      var maxCycles = (repeatCount === Infinity) ? completedCycles : Math.min(completedCycles, Math.ceil(repeatCount) - 1);
      while (st.cycle < maxCycles && (st.beginTime + dur * (st.cycle + 1)) < effectiveEnd) {
        st.cycle++;
        _lumen_smil_dispatch(nid, 'repeatEvent');
      }
    }

    var fraction = 0;
    if (dur !== Infinity) {
      var withinCycle = elapsed - dur * Math.floor(elapsed / dur);
      fraction = Math.max(0, Math.min(1, withinCycle / dur));
    }
    var attrName = _lumen_u2n(_lumen_get_attr(nid, 'attributeName'));
    if (attrName) {
      var valueStr = _lumen_smil_compute_value(nid, fraction);
      if (valueStr !== null) _lumen_smil_overrides[nid + '|' + attrName] = valueStr;
    }

    if (now_s >= effectiveEnd) {
      st.ended = true;
      if (attrName && _lumen_smil_parse_fill(nid) !== 'freeze') {
        delete _lumen_smil_overrides[nid + '|' + attrName];
      }
      _lumen_smil_dispatch(nid, 'endEvent');
    }
  }

  // Called once per rendering frame from the Rust shell
  // (`PersistentJs::tick_smil`, `crates/shell/src/lumen/smil.rs`), in the
  // same spec step CSS transitions/animations tick, before rAF callbacks.
  window._lumen_tick_smil = function(now_s) {
    if (!_lumen_smil_seen) return;
    if (_lumen_smil_doc_epoch === null) _lumen_smil_doc_epoch = now_s;
    _lumen_smil_last_now = now_s;
    if (typeof document === 'undefined' || typeof document.getElementsByTagName !== 'function') return;
    var all = document.getElementsByTagName('*');
    for (var i = 0; i < all.length; i++) {
      if (all[i] instanceof SVGAnimationElement) {
        _lumen_smil_tick_one(all[i], now_s, _lumen_smil_doc_epoch);
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
      return (st && st.beginTime !== null && _lumen_smil_doc_epoch !== null)
        ? st.beginTime - _lumen_smil_doc_epoch : 0;
    }
    getCurrentTime() {
      return _lumen_smil_doc_epoch === null ? 0 : _lumen_smil_last_now - _lumen_smil_doc_epoch;
    }
    getSimpleDuration() {
      var d = _lumen_smil_parse_dur(this.__nid__);
      return d === Infinity ? 0 : d;
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
  class SVGAnimateTransformElement extends SVGAnimateElement {}
  window.SVGAnimateTransformElement = SVGAnimateTransformElement;

  // SVGAnimateMotionElement — <animateMotion> (events/timing only — path
  // following is out of scope).
  class SVGAnimateMotionElement extends SVGAnimationElement {}
  window.SVGAnimateMotionElement = SVGAnimateMotionElement;

  // SVGSetElement — <set>
  class SVGSetElement extends SVGAnimateElement {}
  window.SVGSetElement = SVGSetElement;

  // SVGViewElement — <view>
  class SVGViewElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'view';
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
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
      this.href = new SVGAnimatedString('');
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
  window._lumen_svg_ctor_for_local = function(local) {
    var ctor = SVG_TAG_MAP[local] || SVG_TAG_MAP[local.toLowerCase()] || SVGElement;
    // GAP-SMIL perf gate: flip once, the first time any SMIL element (of
    // either markup or `createElementNS` origin) is resolved, so
    // `_lumen_tick_smil` can no-op in one boolean check on every other page.
    if (!_lumen_smil_seen && (ctor === SVGAnimationElement || ctor.prototype instanceof SVGAnimationElement)) {
      _lumen_smil_seen = true;
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
})();
"#;

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
            const t = new SVGTransform();
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
            const atl = new SVGAnimatedTransformList();
            const t = new SVGTransform();
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
            (new SVGSetElement()) instanceof SVGAnimateElement &&
            (new SVGAnimateTransformElement()) instanceof SVGAnimateElement &&
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
            window._lumen_smil_attrs = {{1: {{{attrs_js}}}}};
            window._lumen_get_attr = function(nid, attr) {{
                var a = window._lumen_smil_attrs[nid];
                return (a && Object.prototype.hasOwnProperty.call(a, attr)) ? a[attr] : undefined;
            }};
            window._lumen_u2n = function(v) {{ return v === undefined ? null : v; }};
            window._lumen_dispatch_log = [];
            window._lumen_dispatch = function(nid, event) {{ window._lumen_dispatch_log.push(event.type); return true; }};
            var node = new (_lumen_svg_ctor_for_local({local:?}))();
            node.__nid__ = 1;
            window._lumen_smil_node = node;
            var _allEls = [node];
            document.getElementsByTagName = function(tag) {{ return _allEls; }};
            "#
        ))
        .unwrap();
        rt
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
        rt.eval("window._lumen_smil_node.beginElement(); _lumen_tick_smil(5.0);").unwrap();
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
}
