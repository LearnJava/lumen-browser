// ── Geometry Interfaces Module (BUG-522) ─────────────────────────────────────
// `DOMPointReadOnly`/`DOMPoint`, `DOMRectReadOnly`/`DOMRect`, `DOMRectList`,
// `DOMMatrixReadOnly`/`DOMMatrix`, `WebKitCSSMatrix` and `DOMQuad` did not
// exist as globals at all — every `new DOMMatrix()`/`new DOMRect()` in the
// wild threw `TypeError`/`ReferenceError`, and `Element.prototype
// .getClientRects()` (BUG-478, defined further down in this file's sibling)
// could not return the `DOMRectList` its own return type demands. This is the
// one gap WPT-RUN-3 found with the largest single-root-cause subtest count
// (720/784 of `css/geometry`) to date.
//
// All five interfaces share one internal-slot convention: the numeric state
// lives in non-enumerable `_`-prefixed own properties (`_x`/_y`/…, `_m`), and
// the spec-visible members are accessor pairs on the prototype so a `for…in`/
// `Object.keys()` over an instance sees exactly the WebIDL attribute list,
// nothing implementation-shaped.
//
// `DOMMatrix`'s 4×4 state is stored row-major, one flat array of 16 numbers
// indexed `m11..m44` in reading order (`_m[0]`=m11, `_m[1]`=m12, …, `_m[15]`=
// m44) — the same order `matrix3d(m11, m12, …, m44)` lists its 16 arguments
// in (verified against `matrix(a,b,c,d,e,f)`'s own expansion, CSS Transforms
// L1 §12.2: `matrix3d(a,b,0,0, c,d,0,0, 0,0,1,0, e,f,0,1)`, which is only
// consistent with a row-major reading if `a=m11 b=m12 c=m21 d=m22 e=m41
// f=m42` — exactly the aliases every engine documents). That mapping in turn
// fixes the vector convention: a point is a row vector transformed as
// `p' = p·M`, so `M`'s last row (`m41,m42,m43`) carries the translation.
// Every helper below (`_dm_translate`, `_dm_rotate_z`, …) and the transform-
// function parser build 4×4 matrices under that same convention; the 2×2
// canvas-CTM helper (`_c2d_compose`, `web_api_shim_mid.js`) uses the
// equivalent 2-D formula independently, ported by hand rather than shared,
// same as other cross-file "same shape, different file" pairs in this shim.

// ── DOMPointReadOnly / DOMPoint (Geometry Interfaces §4) ─────────────────────

function DOMPointReadOnly(x, y, z, w) {
    Object.defineProperty(this, '_x', { value: x !== undefined ? +x : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_y', { value: y !== undefined ? +y : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_z', { value: z !== undefined ? +z : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_w', { value: w !== undefined ? +w : 1, writable: true, enumerable: false, configurable: true });
}

function _dompoint_define_accessors(proto, writable) {
    ['x', 'y', 'z', 'w'].forEach(function(name) {
        var def = { get: function() { return this['_' + name]; }, enumerable: true, configurable: true };
        if (writable) { def.set = function(v) { this['_' + name] = +v; }; }
        Object.defineProperty(proto, name, def);
    });
}
_dompoint_define_accessors(DOMPointReadOnly.prototype, false);

DOMPointReadOnly.prototype.toJSON = function() {
    return { x: this.x, y: this.y, z: this.z, w: this.w };
};
// matrixTransform() always returns a DOMPoint, even called on a
// DOMPointReadOnly (Geometry Interfaces §4.2) — it delegates to
// DOMMatrixReadOnly.transformPoint(), which shares that same asymmetry.
DOMPointReadOnly.prototype.matrixTransform = function(matrixInit) {
    var m = (matrixInit instanceof DOMMatrixReadOnly) ? matrixInit : new DOMMatrix(matrixInit);
    return m.transformPoint(this);
};
DOMPointReadOnly.fromPoint = function(other) {
    other = other || {};
    return new DOMPointReadOnly(other.x, other.y, other.z, other.w);
};

function DOMPoint(x, y, z, w) { DOMPointReadOnly.call(this, x, y, z, w); }
DOMPoint.prototype = Object.create(DOMPointReadOnly.prototype);
DOMPoint.prototype.constructor = DOMPoint;
_dompoint_define_accessors(DOMPoint.prototype, true);
DOMPoint.fromPoint = function(other) {
    other = other || {};
    return new DOMPoint(other.x, other.y, other.z, other.w);
};

// ── DOMRectReadOnly / DOMRect (Geometry Interfaces §5) ───────────────────────

function DOMRectReadOnly(x, y, width, height) {
    Object.defineProperty(this, '_x', { value: x !== undefined ? +x : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_y', { value: y !== undefined ? +y : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_width',  { value: width  !== undefined ? +width  : 0, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_height', { value: height !== undefined ? +height : 0, writable: true, enumerable: false, configurable: true });
}

function _domrect_define_accessors(proto, writable) {
    ['x', 'y', 'width', 'height'].forEach(function(name) {
        var def = { get: function() { return this['_' + name]; }, enumerable: true, configurable: true };
        if (writable) { def.set = function(v) { this['_' + name] = +v; }; }
        Object.defineProperty(proto, name, def);
    });
    // top/right/bottom/left are derived from x/y/width/height on every read
    // (a negative width/height flips which edge is "top"/"left") — never
    // stored, so they can never drift from the box they describe.
    Object.defineProperty(proto, 'top', {
        get: function() { return Math.min(this._y, this._y + this._height); }, enumerable: true, configurable: true,
    });
    Object.defineProperty(proto, 'left', {
        get: function() { return Math.min(this._x, this._x + this._width); }, enumerable: true, configurable: true,
    });
    Object.defineProperty(proto, 'right', {
        get: function() { return Math.max(this._x, this._x + this._width); }, enumerable: true, configurable: true,
    });
    Object.defineProperty(proto, 'bottom', {
        get: function() { return Math.max(this._y, this._y + this._height); }, enumerable: true, configurable: true,
    });
}
_domrect_define_accessors(DOMRectReadOnly.prototype, false);

DOMRectReadOnly.prototype.toJSON = function() {
    return {
        x: this.x, y: this.y, width: this.width, height: this.height,
        top: this.top, right: this.right, bottom: this.bottom, left: this.left,
    };
};
DOMRectReadOnly.fromRect = function(other) {
    other = other || {};
    return new DOMRectReadOnly(other.x, other.y, other.width, other.height);
};

function DOMRect(x, y, width, height) { DOMRectReadOnly.call(this, x, y, width, height); }
DOMRect.prototype = Object.create(DOMRectReadOnly.prototype);
DOMRect.prototype.constructor = DOMRect;
_domrect_define_accessors(DOMRect.prototype, true);
DOMRect.fromRect = function(other) {
    other = other || {};
    return new DOMRect(other.x, other.y, other.width, other.height);
};

// ── DOMRectList (Geometry Interfaces §5.3) ───────────────────────────────────
// Element.prototype.getClientRects() (BUG-478, `web_api_shim_mid.js`) is the
// one caller that matters: `resources/testdriver.js` opens on it, so every
// testdriver-driven action (WPT-RUN-12) was unreachable until this existed.

function DOMRectList(rects) {
    rects = rects || [];
    for (var i = 0; i < rects.length; i++) {
        Object.defineProperty(this, i, { value: rects[i], enumerable: true, configurable: true });
    }
    Object.defineProperty(this, 'length', { value: rects.length, enumerable: false, configurable: true });
}
DOMRectList.prototype.item = function(index) {
    index = index >>> 0;
    return (index < this.length) ? this[index] : null;
};
DOMRectList.prototype[Symbol.iterator] = function() {
    var i = 0, self = this;
    return {
        next: function() {
            return i < self.length ? { value: self[i++], done: false } : { value: undefined, done: true };
        },
    };
};

// ── DOMMatrixReadOnly / DOMMatrix (Geometry Interfaces §6) ───────────────────

function _dm_identity() { return [1, 0, 0, 0,  0, 1, 0, 0,  0, 0, 1, 0,  0, 0, 0, 1]; }

function _dm_mat_equal(a, b) {
    for (var i = 0; i < 16; i++) { if (a[i] !== b[i]) { return false; } }
    return true;
}
function _dm_nan16() {
    var out = [];
    for (var i = 0; i < 16; i++) { out.push(NaN); }
    return out;
}

// Standard 4×4 matrix product `A×B`. Under this file's row-vector convention
// (`p' = p·M`) that product means "A applied first, then B" — used both by
// `multiply()` (this-then-other) and by the transform-function-list parser
// below (later-in-the-list functions apply closer to the raw coordinates).
function _dm_mat4_mul(A, B) {
    var r = new Array(16);
    for (var i = 0; i < 4; i++) {
        for (var j = 0; j < 4; j++) {
            var sum = 0;
            for (var k = 0; k < 4; k++) { sum += A[i * 4 + k] * B[k * 4 + j]; }
            r[i * 4 + j] = sum;
        }
    }
    return r;
}
function _dm_translate(tx, ty, tz) { var m = _dm_identity(); m[12] = tx; m[13] = ty; m[14] = tz; return m; }
function _dm_scale(sx, sy, sz)     { var m = _dm_identity(); m[0]  = sx; m[5]  = sy; m[10] = sz; return m; }
function _dm_rotate_z(deg) {
    var r = deg * Math.PI / 180, c = Math.cos(r), s = Math.sin(r);
    var m = _dm_identity(); m[0] = c; m[1] = s; m[4] = -s; m[5] = c; return m;
}
function _dm_rotate_x(deg) {
    var r = deg * Math.PI / 180, c = Math.cos(r), s = Math.sin(r);
    var m = _dm_identity(); m[5] = c; m[6] = s; m[9] = -s; m[10] = c; return m;
}
function _dm_rotate_y(deg) {
    var r = deg * Math.PI / 180, c = Math.cos(r), s = Math.sin(r);
    var m = _dm_identity(); m[0] = c; m[2] = -s; m[8] = s; m[10] = c; return m;
}
function _dm_skew_x(deg) { var m = _dm_identity(); m[4] = Math.tan(deg * Math.PI / 180); return m; }
function _dm_skew_y(deg) { var m = _dm_identity(); m[1] = Math.tan(deg * Math.PI / 180); return m; }
function _dm_perspective(d) { var m = _dm_identity(); if (d) { m[11] = -1 / d; } return m; }
// Rodrigues' rotation formula for a unit axis, built as the column-vector
// matrix R (`v' = R·v`) and then transposed into this file's row-vector
// storage (`M = Rᵀ`, per the file-header note on the vector convention).
function _dm_rotate_axis_angle(x, y, z, deg) {
    var len = Math.sqrt(x * x + y * y + z * z);
    if (len === 0) { return _dm_identity(); }
    x /= len; y /= len; z /= len;
    var r = deg * Math.PI / 180, c = Math.cos(r), s = Math.sin(r), t = 1 - c;
    var R = [
        t * x * x + c,     t * x * y - s * z, t * x * z + s * y,
        t * x * y + s * z, t * y * y + c,     t * y * z - s * x,
        t * x * z - s * y, t * y * z + s * x, t * z * z + c,
    ];
    var m = _dm_identity();
    m[0] = R[0]; m[1] = R[3]; m[2] = R[6];
    m[4] = R[1]; m[5] = R[4]; m[6] = R[7];
    m[8] = R[2]; m[9] = R[5]; m[10] = R[8];
    return m;
}
// Generic 4×4 inverse via Gauss-Jordan elimination with partial pivoting —
// works regardless of the row/column-vector convention, since it only
// inverts the numeric matrix. Returns null for a singular matrix; callers
// turn that into the spec's all-NaN result (DOMMatrix §6, `inverse()`).
function _dm_invert4x4(flat) {
    var A = [], I = [];
    for (var r = 0; r < 4; r++) { A.push(flat.slice(r * 4, r * 4 + 4)); I.push(_dm_identity().slice(r * 4, r * 4 + 4)); }
    for (var col = 0; col < 4; col++) {
        var pivotRow = col, maxVal = Math.abs(A[col][col]);
        for (r = col + 1; r < 4; r++) {
            if (Math.abs(A[r][col]) > maxVal) { maxVal = Math.abs(A[r][col]); pivotRow = r; }
        }
        if (maxVal < 1e-12) { return null; }
        if (pivotRow !== col) {
            var tmpA = A[col]; A[col] = A[pivotRow]; A[pivotRow] = tmpA;
            var tmpI = I[col]; I[col] = I[pivotRow]; I[pivotRow] = tmpI;
        }
        var pivot = A[col][col], c;
        for (c = 0; c < 4; c++) { A[col][c] /= pivot; I[col][c] /= pivot; }
        for (r = 0; r < 4; r++) {
            if (r === col) { continue; }
            var factor = A[r][col];
            if (factor === 0) { continue; }
            for (c = 0; c < 4; c++) { A[r][c] -= factor * A[col][c]; I[r][c] -= factor * I[col][c]; }
        }
    }
    var out = [];
    for (r = 0; r < 4; r++) { for (c = 0; c < 4; c++) { out.push(I[r][c]); } }
    return out;
}
// toFloat32Array()/toFloat64Array() report column-major regardless of this
// file's row-major internal storage (Geometry Interfaces §6.5) — the same
// layout WebGL's `gl.uniformMatrix4fv` expects, which is the format's whole
// reason to exist.
function _dm_to_col_major(m) {
    var out = new Array(16);
    for (var col = 0; col < 4; col++) { for (var row = 0; row < 4; row++) { out[col * 4 + row] = m[row * 4 + col]; } }
    return out;
}

var _DM_PROPS = ['m11', 'm12', 'm13', 'm14', 'm21', 'm22', 'm23', 'm24', 'm31', 'm32', 'm33', 'm34', 'm41', 'm42', 'm43', 'm44'];
// Setting any of these four keeps is2D as-is; every other m_IJ setter forces
// is2D to false (DOMMatrix §6 attribute setters) — the same distinction the
// 2D vs. 3D transform functions below make when composing a string.
var _DM_2D_CORE_IDX = { 0: 1, 1: 1, 4: 1, 5: 1, 12: 1, 13: 1 };

function _dm_ro_getter(idx) { return function() { return this._m[idx]; }; }
function _dm_rw_setter(idx, isCore) {
    return function(v) {
        if (!isCore) { this._is2D = false; }
        this._m[idx] = +v;
    };
}
function _dm_define_readonly_accessors(proto) {
    for (var i = 0; i < _DM_PROPS.length; i++) {
        Object.defineProperty(proto, _DM_PROPS[i], { get: _dm_ro_getter(i), enumerable: true, configurable: true });
    }
    Object.defineProperty(proto, 'a', { get: _dm_ro_getter(0),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'b', { get: _dm_ro_getter(1),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'c', { get: _dm_ro_getter(4),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'd', { get: _dm_ro_getter(5),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'e', { get: _dm_ro_getter(12), enumerable: true, configurable: true });
    Object.defineProperty(proto, 'f', { get: _dm_ro_getter(13), enumerable: true, configurable: true });
    Object.defineProperty(proto, 'is2D', { get: function() { return this._is2D; }, enumerable: true, configurable: true });
    Object.defineProperty(proto, 'isIdentity', {
        get: function() { return _dm_mat_equal(this._m, _dm_identity()); }, enumerable: true, configurable: true,
    });
}
function _dm_define_mutable_accessors(proto) {
    for (var i = 0; i < _DM_PROPS.length; i++) {
        Object.defineProperty(proto, _DM_PROPS[i], {
            get: _dm_ro_getter(i), set: _dm_rw_setter(i, !!_DM_2D_CORE_IDX[i]), enumerable: true, configurable: true,
        });
    }
    Object.defineProperty(proto, 'a', { get: _dm_ro_getter(0),  set: _dm_rw_setter(0, true),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'b', { get: _dm_ro_getter(1),  set: _dm_rw_setter(1, true),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'c', { get: _dm_ro_getter(4),  set: _dm_rw_setter(4, true),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'd', { get: _dm_ro_getter(5),  set: _dm_rw_setter(5, true),  enumerable: true, configurable: true });
    Object.defineProperty(proto, 'e', { get: _dm_ro_getter(12), set: _dm_rw_setter(12, true), enumerable: true, configurable: true });
    Object.defineProperty(proto, 'f', { get: _dm_ro_getter(13), set: _dm_rw_setter(13, true), enumerable: true, configurable: true });
    Object.defineProperty(proto, 'is2D', { get: function() { return this._is2D; }, enumerable: true, configurable: true });
    Object.defineProperty(proto, 'isIdentity', {
        get: function() { return _dm_mat_equal(this._m, _dm_identity()); }, enumerable: true, configurable: true,
    });
}

// CSS Transforms L1/L2 `<transform-list>` parser, used by the `DOMMatrix
// (DOMString transform)` constructor and `setMatrixValue()`. Functions are
// applied in the order they appear in the string (CSS's own semantics: the
// last-listed function acts on the raw coordinates first, the first-listed
// wraps everything else) — see the "applied first" note on `_dm_mat4_mul`.
function _dm_parse_len(s) {
    var m = /^\s*(-?[\d.eE+-]+)(px)?\s*$/.exec(String(s));
    return m ? parseFloat(m[1]) : NaN;
}
function _dm_parse_angle(s) {
    var m = /^\s*(-?[\d.eE+-]+)(deg|grad|rad|turn)?\s*$/.exec(String(s));
    if (!m) { return NaN; }
    var v = parseFloat(m[1]);
    if (m[2] === 'rad') { return v * 180 / Math.PI; }
    if (m[2] === 'grad') { return v * 0.9; }
    if (m[2] === 'turn') { return v * 360; }
    return v;
}
function _dm_func_to_matrix(name, argStr) {
    var args = argStr.trim() === '' ? [] : argStr.split(',').map(function(s) { return s.trim(); });
    switch (name) {
        case 'matrix': {
            var n = args.map(Number);
            return { m: [n[0], n[1], 0, 0,  n[2], n[3], 0, 0,  0, 0, 1, 0,  n[4], n[5], 0, 1], is3D: false };
        }
        case 'matrix3d': return { m: args.map(Number).slice(0, 16), is3D: true };
        case 'translate': return { m: _dm_translate(_dm_parse_len(args[0]), args.length > 1 ? _dm_parse_len(args[1]) : 0, 0), is3D: false };
        case 'translateX': return { m: _dm_translate(_dm_parse_len(args[0]), 0, 0), is3D: false };
        case 'translateY': return { m: _dm_translate(0, _dm_parse_len(args[0]), 0), is3D: false };
        case 'translateZ': return { m: _dm_translate(0, 0, _dm_parse_len(args[0])), is3D: true };
        case 'translate3d': return { m: _dm_translate(_dm_parse_len(args[0]), _dm_parse_len(args[1]), _dm_parse_len(args[2])), is3D: true };
        case 'scale': { var sx = Number(args[0]); return { m: _dm_scale(sx, args.length > 1 ? Number(args[1]) : sx, 1), is3D: false }; }
        case 'scaleX': return { m: _dm_scale(Number(args[0]), 1, 1), is3D: false };
        case 'scaleY': return { m: _dm_scale(1, Number(args[0]), 1), is3D: false };
        case 'scaleZ': return { m: _dm_scale(1, 1, Number(args[0])), is3D: true };
        case 'scale3d': return { m: _dm_scale(Number(args[0]), Number(args[1]), Number(args[2])), is3D: true };
        case 'rotate': return { m: _dm_rotate_z(_dm_parse_angle(args[0])), is3D: false };
        case 'rotateX': return { m: _dm_rotate_x(_dm_parse_angle(args[0])), is3D: true };
        case 'rotateY': return { m: _dm_rotate_y(_dm_parse_angle(args[0])), is3D: true };
        case 'rotateZ': return { m: _dm_rotate_z(_dm_parse_angle(args[0])), is3D: true };
        case 'rotate3d': return { m: _dm_rotate_axis_angle(Number(args[0]), Number(args[1]), Number(args[2]), _dm_parse_angle(args[3])), is3D: true };
        case 'skew': return { m: _dm_mat4_mul(_dm_skew_x(_dm_parse_angle(args[0])), _dm_skew_y(args.length > 1 ? _dm_parse_angle(args[1]) : 0)), is3D: false };
        case 'skewX': return { m: _dm_skew_x(_dm_parse_angle(args[0])), is3D: false };
        case 'skewY': return { m: _dm_skew_y(_dm_parse_angle(args[0])), is3D: false };
        case 'perspective': return { m: _dm_perspective(_dm_parse_len(args[0])), is3D: true };
        default: return null;
    }
}
function _dm_parse_transform_string(str) {
    str = String(str).trim();
    if (str === '' || str === 'none') { return { m: _dm_identity(), is2D: true }; }
    var re = /([a-zA-Z0-9]+)\s*\(([^)]*)\)/g, acc = _dm_identity(), any3D = false, found = false, match;
    while ((match = re.exec(str)) !== null) {
        found = true;
        var f = _dm_func_to_matrix(match[1], match[2]);
        if (!f) { throw new TypeError('DOMMatrix: unknown transform function "' + match[1] + '"'); }
        if (f.is3D) { any3D = true; }
        acc = _dm_mat4_mul(f.m, acc);
    }
    if (!found) { throw new TypeError('DOMMatrix: could not parse transform string "' + str + '"'); }
    return { m: acc, is2D: !any3D };
}

// DOMMatrixInit "validate and fixup" (Geometry Interfaces §6.1): a legacy
// member (`a`) and its m_IJ alias (`m11`) may both be given only if they
// agree — this is the one place the module can throw on a shape that isn't
// obviously wrong, and is exactly what `setTransform({a: 1, m11: 2})`
// exercises (BUG-522's Canvas 2D symptom).
function _dm_pick(dict, legacy, modern, def) {
    var hasL = dict[legacy] !== undefined, hasM = dict[modern] !== undefined;
    if (hasL && hasM) {
        if (+dict[legacy] !== +dict[modern]) {
            throw new TypeError("DOMMatrixInit: '" + legacy + "' and '" + modern + "' must match");
        }
        return +dict[legacy];
    }
    if (hasL) { return +dict[legacy]; }
    if (hasM) { return +dict[modern]; }
    return def;
}
function _dm_from_matrixinit_dict(dict) {
    var a = _dm_pick(dict, 'a', 'm11', 1), b = _dm_pick(dict, 'b', 'm12', 0);
    var c = _dm_pick(dict, 'c', 'm21', 0), d = _dm_pick(dict, 'd', 'm22', 1);
    var e = _dm_pick(dict, 'e', 'm41', 0), f = _dm_pick(dict, 'f', 'm42', 0);
    var m13 = dict.m13 !== undefined ? +dict.m13 : 0, m14 = dict.m14 !== undefined ? +dict.m14 : 0;
    var m23 = dict.m23 !== undefined ? +dict.m23 : 0, m24 = dict.m24 !== undefined ? +dict.m24 : 0;
    var m31 = dict.m31 !== undefined ? +dict.m31 : 0, m32 = dict.m32 !== undefined ? +dict.m32 : 0;
    var m33 = dict.m33 !== undefined ? +dict.m33 : 1, m34 = dict.m34 !== undefined ? +dict.m34 : 0;
    var m43 = dict.m43 !== undefined ? +dict.m43 : 0, m44 = dict.m44 !== undefined ? +dict.m44 : 1;
    var has3D = [dict.m13, dict.m14, dict.m23, dict.m24, dict.m31, dict.m32, dict.m34, dict.m43].some(function(v) { return v !== undefined; })
        || (dict.m33 !== undefined && dict.m33 !== 1) || (dict.m44 !== undefined && dict.m44 !== 1);
    if (dict.is2D === true && has3D) {
        throw new TypeError('DOMMatrixInit: is2D is true but 3D components are present');
    }
    var is2D = dict.is2D !== undefined ? !!dict.is2D : !has3D;
    return { m: [a, b, m13, m14,  c, d, m23, m24,  m31, m32, m33, m34,  e, f, m43, m44], is2D: is2D };
}
function _dm_from_typed_array(ctor, arr) {
    var n = arr.length;
    if (n !== 6 && n !== 16) { throw new TypeError('DOMMatrix: typed array must have length 6 or 16'); }
    if (n === 6) { return _dm_from_raw(ctor, [+arr[0], +arr[1], 0, 0,  +arr[2], +arr[3], 0, 0,  0, 0, 1, 0,  +arr[4], +arr[5], 0, 1], true); }
    var row = new Array(16);
    for (var col = 0; col < 4; col++) { for (var r = 0; r < 4; r++) { row[r * 4 + col] = +arr[col * 4 + r]; } }
    return _dm_from_raw(ctor, row, false);
}
// One argument, four accepted shapes (Geometry Interfaces §6 constructor):
// absent/undefined → identity, a transform-list string, a length-6 or -16
// sequence, or a matrix-like object (another DOMMatrixReadOnly/DOMMatrix, or
// a DOMMatrixInit-shaped dict run through validate-and-fixup).
function _dm_parse_init(init) {
    if (init === undefined || init === null) { return { m: _dm_identity(), is2D: true }; }
    if (init instanceof DOMMatrixReadOnly) { return { m: init._m.slice(), is2D: init._is2D }; }
    if (typeof init === 'string') { return _dm_parse_transform_string(init); }
    if (typeof init === 'object' && typeof init.length === 'number') {
        var n = init.length;
        if (n === 6) {
            var a = +init[0], b = +init[1], c = +init[2], d = +init[3], e = +init[4], f = +init[5];
            return { m: [a, b, 0, 0,  c, d, 0, 0,  0, 0, 1, 0,  e, f, 0, 1], is2D: true };
        }
        if (n === 16) {
            var arr = []; for (var i = 0; i < 16; i++) { arr.push(+init[i]); }
            return { m: arr, is2D: false };
        }
        throw new TypeError('DOMMatrix: sequence must have length 6 or 16');
    }
    if (typeof init === 'object') { return _dm_from_matrixinit_dict(init); }
    throw new TypeError('DOMMatrix: invalid init value');
}
function _dm_from_raw(ctor, m, is2D) {
    var out = Object.create(ctor.prototype);
    Object.defineProperty(out, '_m', { value: m.slice(), writable: true, enumerable: false, configurable: true });
    Object.defineProperty(out, '_is2D', { value: !!is2D, writable: true, enumerable: false, configurable: true });
    return out;
}
// Methods inherited from DOMMatrixReadOnly return an instance of whichever
// concrete class `this` actually is (DOMMatrix stays DOMMatrix, per spec) —
// this is the one piece of polymorphism the module needs, since DOMMatrix
// does not otherwise override any of these.
function _dm_result_ctor(self) { return (self instanceof DOMMatrix) ? DOMMatrix : DOMMatrixReadOnly; }

function DOMMatrixReadOnly(init) {
    var parsed = _dm_parse_init(init);
    Object.defineProperty(this, '_m', { value: parsed.m, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_is2D', { value: !!parsed.is2D, writable: true, enumerable: false, configurable: true });
}
_dm_define_readonly_accessors(DOMMatrixReadOnly.prototype);

DOMMatrixReadOnly.prototype.multiply = function(other) {
    var om = _dm_parse_init(other);
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, om.m), this._is2D && om.is2D);
};
DOMMatrixReadOnly.prototype.flipX = function() {
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_scale(-1, 1, 1)), this._is2D);
};
DOMMatrixReadOnly.prototype.flipY = function() {
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_scale(1, -1, 1)), this._is2D);
};
DOMMatrixReadOnly.prototype.inverse = function() {
    var inv = _dm_invert4x4(this._m);
    if (!inv) { return _dm_from_raw(_dm_result_ctor(this), _dm_nan16(), false); }
    return _dm_from_raw(_dm_result_ctor(this), inv, this._is2D);
};
DOMMatrixReadOnly.prototype.translate = function(tx, ty, tz) {
    tx = tx === undefined ? 0 : +tx; ty = ty === undefined ? 0 : +ty; tz = tz === undefined ? 0 : +tz;
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_translate(tx, ty, tz)), this._is2D && tz === 0);
};
DOMMatrixReadOnly.prototype.scale = function(sx, sy, sz, ox, oy, oz) {
    sx = sx === undefined ? 1 : +sx; sy = sy === undefined ? sx : +sy; sz = sz === undefined ? 1 : +sz;
    ox = ox === undefined ? 0 : +ox; oy = oy === undefined ? 0 : +oy; oz = oz === undefined ? 0 : +oz;
    var around = _dm_mat4_mul(_dm_translate(-ox, -oy, -oz), _dm_mat4_mul(_dm_scale(sx, sy, sz), _dm_translate(ox, oy, oz)));
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, around), this._is2D && sz === 1 && oz === 0);
};
DOMMatrixReadOnly.prototype.scale3d = function(scale, ox, oy, oz) { return this.scale(scale, scale, scale, ox, oy, oz); };
DOMMatrixReadOnly.prototype.rotate = function(rotX, rotY, rotZ) {
    if (rotY === undefined && rotZ === undefined) { rotZ = rotX; rotX = 0; rotY = 0; }
    else { rotX = rotX === undefined ? 0 : rotX; rotY = rotY === undefined ? 0 : rotY; rotZ = rotZ === undefined ? 0 : rotZ; }
    var m = _dm_mat4_mul(_dm_rotate_x(rotX), _dm_mat4_mul(_dm_rotate_y(rotY), _dm_rotate_z(rotZ)));
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, m), this._is2D && rotX === 0 && rotY === 0);
};
DOMMatrixReadOnly.prototype.rotateFromVector = function(x, y) {
    x = x === undefined ? 0 : +x; y = y === undefined ? 0 : +y;
    return this.rotate((x === 0 && y === 0) ? 0 : Math.atan2(y, x) * 180 / Math.PI);
};
DOMMatrixReadOnly.prototype.rotateAxisAngle = function(x, y, z, angle) {
    x = x === undefined ? 0 : +x; y = y === undefined ? 0 : +y; z = z === undefined ? 0 : +z; angle = angle === undefined ? 0 : +angle;
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_rotate_axis_angle(x, y, z, angle)), false);
};
DOMMatrixReadOnly.prototype.skewX = function(sx) {
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_skew_x(sx === undefined ? 0 : +sx)), this._is2D);
};
DOMMatrixReadOnly.prototype.skewY = function(sy) {
    return _dm_from_raw(_dm_result_ctor(this), _dm_mat4_mul(this._m, _dm_skew_y(sy === undefined ? 0 : +sy)), this._is2D);
};
DOMMatrixReadOnly.prototype.transformPoint = function(point) {
    var p = point || {}, m = this._m;
    var x = p.x === undefined ? 0 : +p.x, y = p.y === undefined ? 0 : +p.y, z = p.z === undefined ? 0 : +p.z, w = p.w === undefined ? 1 : +p.w;
    return new DOMPoint(
        x * m[0] + y * m[4] + z * m[8]  + w * m[12],
        x * m[1] + y * m[5] + z * m[9]  + w * m[13],
        x * m[2] + y * m[6] + z * m[10] + w * m[14],
        x * m[3] + y * m[7] + z * m[11] + w * m[15]
    );
};
DOMMatrixReadOnly.prototype.toFloat32Array = function() { return new Float32Array(_dm_to_col_major(this._m)); };
DOMMatrixReadOnly.prototype.toFloat64Array = function() { return new Float64Array(_dm_to_col_major(this._m)); };
DOMMatrixReadOnly.prototype.toJSON = function() {
    var out = { a: this.a, b: this.b, c: this.c, d: this.d, e: this.e, f: this.f, is2D: this.is2D, isIdentity: this.isIdentity };
    for (var i = 0; i < _DM_PROPS.length; i++) { out[_DM_PROPS[i]] = this._m[i]; }
    return out;
};
// 2D serializes as `matrix(a,b,c,d,e,f)`; 3D as `matrix3d(...)` with the 16
// values in the same row-major reading order the constructor/parser use —
// deliberately not `_dm_to_col_major`, which is a different, WebGL-shaped
// serialization the typed-array methods use instead.
DOMMatrixReadOnly.prototype.toString = function() {
    if (this._is2D) { return 'matrix(' + [this.a, this.b, this.c, this.d, this.e, this.f].join(', ') + ')'; }
    return 'matrix3d(' + this._m.join(', ') + ')';
};
DOMMatrixReadOnly.fromMatrix = function(other) {
    var d = _dm_from_matrixinit_dict(other || {});
    return _dm_from_raw(DOMMatrixReadOnly, d.m, d.is2D);
};
DOMMatrixReadOnly.fromFloat32Array = function(arr) { return _dm_from_typed_array(DOMMatrixReadOnly, arr); };
DOMMatrixReadOnly.fromFloat64Array = function(arr) { return _dm_from_typed_array(DOMMatrixReadOnly, arr); };

function DOMMatrix(init) {
    var parsed = _dm_parse_init(init);
    Object.defineProperty(this, '_m', { value: parsed.m, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_is2D', { value: !!parsed.is2D, writable: true, enumerable: false, configurable: true });
}
DOMMatrix.prototype = Object.create(DOMMatrixReadOnly.prototype);
DOMMatrix.prototype.constructor = DOMMatrix;
_dm_define_mutable_accessors(DOMMatrix.prototype);

function _dm_apply_self(self, result) { self._m = result._m; self._is2D = result._is2D; return self; }
DOMMatrix.prototype.multiplySelf = function(other) { return _dm_apply_self(this, this.multiply(other)); };
DOMMatrix.prototype.preMultiplySelf = function(other) {
    var om = _dm_parse_init(other);
    return _dm_apply_self(this, _dm_from_raw(DOMMatrixReadOnly, _dm_mat4_mul(om.m, this._m), this._is2D && om.is2D));
};
DOMMatrix.prototype.translateSelf = function(tx, ty, tz) { return _dm_apply_self(this, this.translate(tx, ty, tz)); };
DOMMatrix.prototype.scaleSelf = function(sx, sy, sz, ox, oy, oz) { return _dm_apply_self(this, this.scale(sx, sy, sz, ox, oy, oz)); };
DOMMatrix.prototype.scale3dSelf = function(scale, ox, oy, oz) { return _dm_apply_self(this, this.scale3d(scale, ox, oy, oz)); };
DOMMatrix.prototype.rotateSelf = function(rotX, rotY, rotZ) { return _dm_apply_self(this, this.rotate(rotX, rotY, rotZ)); };
DOMMatrix.prototype.rotateFromVectorSelf = function(x, y) { return _dm_apply_self(this, this.rotateFromVector(x, y)); };
DOMMatrix.prototype.rotateAxisAngleSelf = function(x, y, z, angle) { return _dm_apply_self(this, this.rotateAxisAngle(x, y, z, angle)); };
DOMMatrix.prototype.skewXSelf = function(sx) { return _dm_apply_self(this, this.skewX(sx)); };
DOMMatrix.prototype.skewYSelf = function(sy) { return _dm_apply_self(this, this.skewY(sy)); };
DOMMatrix.prototype.invertSelf = function() { return _dm_apply_self(this, this.inverse()); };
DOMMatrix.prototype.setMatrixValue = function(transformStr) {
    var parsed = _dm_parse_transform_string(transformStr);
    this._m = parsed.m; this._is2D = parsed.is2D;
    return this;
};
DOMMatrix.fromMatrix = function(other) {
    var d = _dm_from_matrixinit_dict(other || {});
    return _dm_from_raw(DOMMatrix, d.m, d.is2D);
};
DOMMatrix.fromFloat32Array = function(arr) { return _dm_from_typed_array(DOMMatrix, arr); };
DOMMatrix.fromFloat64Array = function(arr) { return _dm_from_typed_array(DOMMatrix, arr); };

// ── DOMQuad (Geometry Interfaces §7) ──────────────────────────────────────────

function DOMQuad(p1, p2, p3, p4) {
    function pt(init) { init = init || {}; return new DOMPoint(init.x, init.y, init.z, init.w); }
    Object.defineProperty(this, 'p1', { value: pt(p1), enumerable: true, configurable: true, writable: true });
    Object.defineProperty(this, 'p2', { value: pt(p2), enumerable: true, configurable: true, writable: true });
    Object.defineProperty(this, 'p3', { value: pt(p3), enumerable: true, configurable: true, writable: true });
    Object.defineProperty(this, 'p4', { value: pt(p4), enumerable: true, configurable: true, writable: true });
}
DOMQuad.prototype.getBounds = function() {
    var xs = [this.p1.x, this.p2.x, this.p3.x, this.p4.x];
    var ys = [this.p1.y, this.p2.y, this.p3.y, this.p4.y];
    var minX = Math.min.apply(null, xs), maxX = Math.max.apply(null, xs);
    var minY = Math.min.apply(null, ys), maxY = Math.max.apply(null, ys);
    return new DOMRect(minX, minY, maxX - minX, maxY - minY);
};
DOMQuad.prototype.toJSON = function() {
    return { p1: this.p1.toJSON(), p2: this.p2.toJSON(), p3: this.p3.toJSON(), p4: this.p4.toJSON() };
};
// One DOMRectInit corner per quad point, winding clockwise from the origin —
// the order every `getBoxQuads()`/`getClientRects()` fallback built on this
// (`web_api_shim_mid.js`, BUG-478) relies on.
DOMQuad.fromRect = function(other) {
    other = other || {};
    var x = other.x === undefined ? 0 : +other.x, y = other.y === undefined ? 0 : +other.y;
    var w = other.width === undefined ? 0 : +other.width, h = other.height === undefined ? 0 : +other.height;
    return new DOMQuad({ x: x, y: y }, { x: x + w, y: y }, { x: x + w, y: y + h }, { x: x, y: y + h });
};
DOMQuad.fromQuad = function(other) {
    other = other || {};
    return new DOMQuad(other.p1, other.p2, other.p3, other.p4);
};

// ── globals ───────────────────────────────────────────────────────────────────
window.DOMPointReadOnly  = DOMPointReadOnly;
window.DOMPoint          = DOMPoint;
window.DOMRectReadOnly   = DOMRectReadOnly;
window.DOMRect           = DOMRect;
window.DOMRectList       = DOMRectList;
window.DOMMatrixReadOnly = DOMMatrixReadOnly;
window.DOMMatrix         = DOMMatrix;
window.DOMQuad           = DOMQuad;
// CSS Transforms L1 §13.1 (legacy): "WebKitCSSMatrix must be an alias for
// DOMMatrix" — not a subclass with divergent mutability, literally the same
// constructor, which is also what satisfies the WPT "Equivalence test"
// subtest BUG-522 called out by name.
window.WebKitCSSMatrix   = DOMMatrix;
