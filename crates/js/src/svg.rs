/// SVG DOM API stubs (W3C SVG 2 §3, §10, §11)
/// Phase 0: SVGElement/SVGSVGElement class hierarchy, getBBox() → DOMRect(zeros),
/// document.createElementNS('http://www.w3.org/2000/svg', ...) patched to return
/// typed SVG element instances. SVGRect/SVGPoint/SVGLength/SVGAnimatedLength types.
use rquickjs::Ctx;

/// Install SVG DOM API bindings into the JS context.
pub fn install_svg_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SVG_SHIM)?;
    Ok(())
}

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

  // ── Base element classes ──────────────────────────────────────────────────

  // SVGElement — base for all SVG elements (W3C SVG 2 §4.3)
  class SVGElement extends Element {
    constructor() {
      super();
      this.namespaceURI = SVG_NS;
      this.ownerSVGElement = null;
      this.viewportElement = null;
      this.style = {};
      this.id = '';
      this.className = new SVGAnimatedString('');
    }
    get dataset() { return {}; }
    focus() {}
    blur() {}
  }
  window.SVGElement = SVGElement;

  // SVGGraphicsElement — adds transform, getBBox, getScreenCTM, getCTM
  class SVGGraphicsElement extends SVGElement {
    constructor() {
      super();
      this.transform = new SVGAnimatedTransformList();
    }

    // Phase 0: returns zero bounding rect; Phase 1 will read layout geometry
    getBBox(options) {
      return new SVGRect(0, 0, 0, 0);
    }

    // Phase 0: returns identity matrix
    getCTM() { return new SVGMatrix(); }
    getScreenCTM() { return new SVGMatrix(); }

    getTransformToElement(element) { return new SVGMatrix(); }
  }
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
      this.x = new SVGAnimatedLength(0);
      this.y = new SVGAnimatedLength(0);
      this.width = new SVGAnimatedLength(300);
      this.height = new SVGAnimatedLength(150);
      this.viewBox = new SVGAnimatedRect();
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
      this.viewBox = new SVGAnimatedRect();
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
    }
  }
  window.SVGSymbolElement = SVGSymbolElement;

  // SVGUseElement — <use>
  class SVGUseElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'use';
      this.x = new SVGAnimatedLength(0);
      this.y = new SVGAnimatedLength(0);
      this.width = new SVGAnimatedLength(0);
      this.height = new SVGAnimatedLength(0);
      this.href = new SVGAnimatedString('');
    }
  }
  window.SVGUseElement = SVGUseElement;

  // SVGImageElement — <image>
  class SVGImageElement extends SVGGraphicsElement {
    constructor() {
      super(); this.tagName = 'image';
      this.x = new SVGAnimatedLength(0);
      this.y = new SVGAnimatedLength(0);
      this.width = new SVGAnimatedLength(0);
      this.height = new SVGAnimatedLength(0);
      this.href = new SVGAnimatedString('');
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
    }
  }
  window.SVGImageElement = SVGImageElement;

  // SVGRectElement — <rect>
  class SVGRectElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'rect';
      this.x  = new SVGAnimatedLength(0);
      this.y  = new SVGAnimatedLength(0);
      this.width  = new SVGAnimatedLength(0);
      this.height = new SVGAnimatedLength(0);
      this.rx = new SVGAnimatedLength(0);
      this.ry = new SVGAnimatedLength(0);
    }
  }
  window.SVGRectElement = SVGRectElement;

  // SVGCircleElement — <circle>
  class SVGCircleElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'circle';
      this.cx = new SVGAnimatedLength(0);
      this.cy = new SVGAnimatedLength(0);
      this.r  = new SVGAnimatedLength(0);
    }
  }
  window.SVGCircleElement = SVGCircleElement;

  // SVGEllipseElement — <ellipse>
  class SVGEllipseElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'ellipse';
      this.cx = new SVGAnimatedLength(0);
      this.cy = new SVGAnimatedLength(0);
      this.rx = new SVGAnimatedLength(0);
      this.ry = new SVGAnimatedLength(0);
    }
  }
  window.SVGEllipseElement = SVGEllipseElement;

  // SVGLineElement — <line>
  class SVGLineElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'line';
      this.x1 = new SVGAnimatedLength(0);
      this.y1 = new SVGAnimatedLength(0);
      this.x2 = new SVGAnimatedLength(0);
      this.y2 = new SVGAnimatedLength(0);
    }
  }
  window.SVGLineElement = SVGLineElement;

  // SVGPolylineElement — <polyline>
  class SVGPolylineElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'polyline';
      this.points = new SVGPointList();
      this.animatedPoints = new SVGPointList();
    }
  }
  window.SVGPolylineElement = SVGPolylineElement;

  // SVGPolygonElement — <polygon>
  class SVGPolygonElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'polygon';
      this.points = new SVGPointList();
      this.animatedPoints = new SVGPointList();
    }
  }
  window.SVGPolygonElement = SVGPolygonElement;

  // SVGPathElement — <path>
  class SVGPathElement extends SVGGeometryElement {
    constructor() {
      super(); this.tagName = 'path';
      this.d = '';
    }
    // Phase 1: will parse SVGPathData
    getPathData() { return []; }
    setPathData(data) {}
  }
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
      this.x  = new SVGAnimatedLength(0);
      this.y  = new SVGAnimatedLength(0);
      this.dx = new SVGAnimatedLength(0);
      this.dy = new SVGAnimatedLength(0);
      this.rotate = new SVGAnimatedInteger(0);
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
      this.startOffset = new SVGAnimatedLength(0);
      this.method = new SVGAnimatedEnumeration(1);
      this.spacing = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
    }
  }
  window.SVGTextPathElement = SVGTextPathElement;

  // SVGClipPathElement — <clipPath>
  class SVGClipPathElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'clipPath';
      this.clipPathUnits = new SVGAnimatedEnumeration(1);
      this.transform = new SVGAnimatedTransformList();
    }
  }
  window.SVGClipPathElement = SVGClipPathElement;

  // SVGMaskElement — <mask>
  class SVGMaskElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'mask';
      this.x = new SVGAnimatedLength(-10);
      this.y = new SVGAnimatedLength(-10);
      this.width = new SVGAnimatedLength(120);
      this.height = new SVGAnimatedLength(120);
      this.maskUnits = new SVGAnimatedEnumeration(2);
      this.maskContentUnits = new SVGAnimatedEnumeration(1);
    }
  }
  window.SVGMaskElement = SVGMaskElement;

  // SVGGradientElement — base for gradient elements
  class SVGGradientElement extends SVGElement {
    constructor() {
      super();
      this.gradientUnits = new SVGAnimatedEnumeration(2);
      this.gradientTransform = new SVGAnimatedTransformList();
      this.spreadMethod = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
    }
  }
  SVGGradientElement.SVG_SPREADMETHOD_UNKNOWN = 0;
  SVGGradientElement.SVG_SPREADMETHOD_PAD     = 1;
  SVGGradientElement.SVG_SPREADMETHOD_REFLECT = 2;
  SVGGradientElement.SVG_SPREADMETHOD_REPEAT  = 3;
  window.SVGGradientElement = SVGGradientElement;

  // SVGLinearGradientElement — <linearGradient>
  class SVGLinearGradientElement extends SVGGradientElement {
    constructor() {
      super(); this.tagName = 'linearGradient';
      this.x1 = new SVGAnimatedLength(0);
      this.y1 = new SVGAnimatedLength(0);
      this.x2 = new SVGAnimatedLength(100);
      this.y2 = new SVGAnimatedLength(0);
    }
  }
  window.SVGLinearGradientElement = SVGLinearGradientElement;

  // SVGRadialGradientElement — <radialGradient>
  class SVGRadialGradientElement extends SVGGradientElement {
    constructor() {
      super(); this.tagName = 'radialGradient';
      this.cx = new SVGAnimatedLength(50);
      this.cy = new SVGAnimatedLength(50);
      this.r  = new SVGAnimatedLength(50);
      this.fx = new SVGAnimatedLength(50);
      this.fy = new SVGAnimatedLength(50);
      this.fr = new SVGAnimatedLength(0);
    }
  }
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
      this.x = new SVGAnimatedLength(0);
      this.y = new SVGAnimatedLength(0);
      this.width = new SVGAnimatedLength(0);
      this.height = new SVGAnimatedLength(0);
      this.patternUnits = new SVGAnimatedEnumeration(2);
      this.patternContentUnits = new SVGAnimatedEnumeration(1);
      this.patternTransform = new SVGAnimatedTransformList();
      this.viewBox = new SVGAnimatedRect();
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
      this.href = new SVGAnimatedString('');
    }
  }
  window.SVGPatternElement = SVGPatternElement;

  // SVGMarkerElement — <marker>
  class SVGMarkerElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'marker';
      this.refX = new SVGAnimatedLength(0);
      this.refY = new SVGAnimatedLength(0);
      this.markerUnits = new SVGAnimatedEnumeration(2);
      this.markerWidth = new SVGAnimatedLength(3);
      this.markerHeight = new SVGAnimatedLength(3);
      this.orientType = new SVGAnimatedEnumeration(1);
      this.orientAngle = new SVGAnimatedNumber(0);
      this.viewBox = new SVGAnimatedRect();
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
    }
    setOrientToAuto() { this.orientType.baseVal = 1; }
    setOrientToAngle(angle) { this.orientType.baseVal = 2; this.orientAngle.baseVal = angle; }
  }
  window.SVGMarkerElement = SVGMarkerElement;

  // SVGFilterElement — <filter>
  class SVGFilterElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'filter';
      this.x = new SVGAnimatedLength(-10);
      this.y = new SVGAnimatedLength(-10);
      this.width = new SVGAnimatedLength(120);
      this.height = new SVGAnimatedLength(120);
      this.filterUnits = new SVGAnimatedEnumeration(2);
      this.primitiveUnits = new SVGAnimatedEnumeration(1);
      this.href = new SVGAnimatedString('');
    }
  }
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
    constructor() {
      super(); this.tagName = 'foreignObject';
      this.x = new SVGAnimatedLength(0);
      this.y = new SVGAnimatedLength(0);
      this.width = new SVGAnimatedLength(0);
      this.height = new SVGAnimatedLength(0);
    }
  }
  window.SVGForeignObjectElement = SVGForeignObjectElement;

  // SVGAnimateElement — <animate> (stub)
  class SVGAnimateElement extends SVGElement {
    constructor() { super(); this.tagName = 'animate'; }
    beginElement() {}
    endElement() {}
    beginElementAt(offset) {}
    endElementAt(offset) {}
  }
  window.SVGAnimateElement = SVGAnimateElement;

  // SVGAnimateTransformElement — <animateTransform> (stub)
  class SVGAnimateTransformElement extends SVGElement {
    constructor() { super(); this.tagName = 'animateTransform'; }
    beginElement() {}
    endElement() {}
  }
  window.SVGAnimateTransformElement = SVGAnimateTransformElement;

  // SVGAnimateMotionElement — <animateMotion> (stub)
  class SVGAnimateMotionElement extends SVGElement {
    constructor() { super(); this.tagName = 'animateMotion'; }
    beginElement() {}
    endElement() {}
  }
  window.SVGAnimateMotionElement = SVGAnimateMotionElement;

  // SVGSetElement — <set> (stub)
  class SVGSetElement extends SVGElement {
    constructor() { super(); this.tagName = 'set'; }
    beginElement() {}
    endElement() {}
  }
  window.SVGSetElement = SVGSetElement;

  // SVGViewElement — <view>
  class SVGViewElement extends SVGElement {
    constructor() {
      super(); this.tagName = 'view';
      this.viewBox = new SVGAnimatedRect();
      this.preserveAspectRatio = new SVGAnimatedPreserveAspectRatio();
      this.zoomAndPan = 2; // SVG_ZOOMANDPAN_MAGNIFY
    }
  }
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

  // Patch document.createElementNS: for SVG namespace return typed SVG element.
  // For all other namespaces delegate to the original implementation.
  if (typeof document !== 'undefined' && typeof document.createElementNS === 'function') {
    const _origCreateElementNS = document.createElementNS.bind(document);
    document.createElementNS = function createElementNS(ns, qualifiedName) {
      if (ns === SVG_NS) {
        // Strip namespace prefix (e.g. "svg:rect" → "rect")
        const localName = (qualifiedName || '').replace(/^[^:]+:/, '').toLowerCase();
        // Canonically keep original casing for case-sensitive tags (e.g. linearGradient)
        const tagLC = localName;
        const Ctor = SVG_TAG_MAP[qualifiedName.replace(/^[^:]+:/, '')] ||
                     SVG_TAG_MAP[tagLC] ||
                     SVGElement;
        const el = new Ctor();
        // Store qualified name for serialisation compatibility
        el._qualifiedName = qualifiedName;
        el.namespaceURI = SVG_NS;
        return el;
      }
      return _origCreateElementNS(ns, qualifiedName);
    };
  }

  // Expose SVG namespace constant
  window.SVG_NAMESPACE = SVG_NS;
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

    /// Install minimal DOM stubs then SVG bindings.
    fn with_svg(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal browser globals used by the SVG shim
            ctx.eval::<(), _>(r#"
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
            install_svg_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn svg_element_class_exists() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval("typeof window.SVGElement === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_svg_element_class_exists() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval("typeof window.SVGSVGElement === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_graphics_element_get_bbox_returns_rect() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const el = new SVGGraphicsElement();
                const bb = el.getBBox();
                bb instanceof SVGRect && bb.x === 0 && bb.y === 0 &&
                bb.width === 0 && bb.height === 0
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_rect_element_has_dimensions() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const r = new SVGRectElement();
                r.x instanceof SVGAnimatedLength &&
                r.y instanceof SVGAnimatedLength &&
                r.width instanceof SVGAnimatedLength &&
                r.height instanceof SVGAnimatedLength
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_circle_element_has_cx_cy_r() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const c = new SVGCircleElement();
                c.cx instanceof SVGAnimatedLength &&
                c.cy instanceof SVGAnimatedLength &&
                c.r  instanceof SVGAnimatedLength
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_path_element_has_get_total_length() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const p = new SVGPathElement();
                typeof p.getTotalLength === 'function' && p.getTotalLength() === 0
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_svg_element_create_svg_rect() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const svg = new SVGSVGElement();
                const r = svg.createSVGRect();
                r instanceof SVGRect
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_svg_element_create_svg_point() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const svg = new SVGSVGElement();
                const p = svg.createSVGPoint();
                p instanceof SVGPoint && p.x === 0 && p.y === 0
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_matrix_multiply_identity() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const a = new SVGMatrix();
                const b = new SVGMatrix();
                const c = a.multiply(b);
                c instanceof SVGMatrix && c.a === 1 && c.d === 1 &&
                c.b === 0 && c.c === 0 && c.e === 0 && c.f === 0
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_transform_set_translate() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const t = new SVGTransform();
                t.setTranslate(10, 20);
                t.type === 2 && t.matrix.e === 10 && t.matrix.f === 20
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_create_element_ns_returns_typed_element() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const el = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
                el instanceof SVGCircleElement
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_create_element_ns_svg_returns_svg_svg_element() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const el = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
                el instanceof SVGSVGElement
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_create_element_ns_unknown_tag_returns_svg_element() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const el = document.createElementNS('http://www.w3.org/2000/svg', 'unknown-tag');
                el instanceof SVGElement
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_point_matrix_transform() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const p = new SVGPoint(3, 4);
                const m = new SVGMatrix(2, 0, 0, 2, 1, 1); // scale(2) + translate(1,1)
                const p2 = p.matrixTransform(m);
                p2.x === 7 && p2.y === 9
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_length_unit_types() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                SVGLength.SVG_LENGTHTYPE_NUMBER === 1 &&
                SVGLength.SVG_LENGTHTYPE_PX     === 5 &&
                SVGLength.SVG_LENGTHTYPE_PERCENTAGE === 2
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_animated_transform_list_consolidate() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const atl = new SVGAnimatedTransformList();
                const t = new SVGTransform();
                t.setTranslate(5, 0);
                atl.baseVal.appendItem(t);
                const c = atl.baseVal.consolidate();
                c instanceof SVGTransform && c.matrix.e === 5
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_linear_gradient_element_x1_x2() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const lg = new SVGLinearGradientElement();
                lg.x1 instanceof SVGAnimatedLength &&
                lg.x2 instanceof SVGAnimatedLength &&
                lg.x2.baseVal.value === 100
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_filter_element_exists() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const f = new SVGFilterElement();
                typeof f.filterUnits === 'object' && f.tagName === 'filter'
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_text_element_get_number_of_chars() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
                const t = new SVGTextElement();
                t.getNumberOfChars() === 0 && t.getComputedTextLength() === 0
            "#).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn svg_classes_on_window() {
        with_svg(|ctx| {
            let ok: bool = ctx.eval(r#"
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
            "#).unwrap();
            assert!(ok);
        });
    }
}
