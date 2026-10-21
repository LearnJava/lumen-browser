// ── CSSPseudoElement (CSS Pseudo-Elements L4 §3, CSSOM-11, BUG-1456) ─────────
// `Element.prototype.pseudo(type)` answers one stable `CSSPseudoElement` per (element, type)
// whether or not the pseudo-element is currently generated; the object is a handle, its boxes
// are read from layout on every call through `_lumen_get_pseudo_geometry` (a same-tick flush,
// like `getBoundingClientRect`). Geometry mirrors GeometryUtils (CSSOM View §Extensions):
// `getBoundingClientRect`/`getClientRects`/`getBoxQuads` and the `convert*FromNode` family.
//
// Known gaps: a pseudo-element that is inline-level (a plain `::before` with the initial
// `display: inline`) is flattened into its owner's line and has no box, so its geometry is empty;
// quads are axis-aligned (an own `transform` becomes the AABB, ancestor transforms are ignored);
// `pseudoTarget` on mouse/pointer events is not wired.

function CSSPseudoElement() { throw new TypeError('Illegal constructor'); }
CSSPseudoElement.prototype = Object.create(EventTarget.prototype);
Object.defineProperty(CSSPseudoElement.prototype, 'constructor', {
    value: CSSPseudoElement, writable: true, enumerable: false, configurable: true });
Object.defineProperty(CSSPseudoElement.prototype, Symbol.toStringTag, {
    value: 'CSSPseudoElement', writable: false, enumerable: false, configurable: true });

// Pseudo-element types an element (or a view-transition parent) can name, with the parent type
// each functional view-transition pseudo hangs from. `::first-line`/`::first-letter` are not
// addressable (they are not elements, CSS Pseudo 4 §3.1), `::selection` and friends neither.
var _PE_ELEMENT_TYPES = /^::(before|after|marker|backdrop|scroll-marker|scroll-marker-group|scroll-button\((up|down|left|right|block-start|block-end|inline-start|inline-end)\)|view-transition)$/;
var _PE_VT_CHILD = {
    '::view-transition': ['::view-transition-group'],
    '::view-transition-group': ['::view-transition-image-pair'],
    '::view-transition-image-pair': ['::view-transition-old', '::view-transition-new']
};

// `::view-transition-group(name)` → ['::view-transition-group', 'name']; null when `type` is not
// a functional view-transition pseudo.
function _pe_split_functional(type) {
    var m = /^(::view-transition-(?:group|image-pair|old|new))\(([^()\s]+)\)$/.exec(type);
    return m ? [m[1], m[2]] : null;
}

var _pe_cache = new WeakMap();  // owner wrapper / pseudo → { selectorText: CSSPseudoElement }

function _pe_make(owner, parent, base, selectorText) {
    var pe = Object.create(CSSPseudoElement.prototype);
    EventTarget.call(pe);
    Object.defineProperty(pe, '_owner', { value: owner });
    Object.defineProperty(pe, '_parent', { value: parent });
    Object.defineProperty(pe, '_type', { value: base });
    Object.defineProperty(pe, '_selector', { value: selectorText });
    return pe;
}

function _pe_lookup(host, owner, type) {
    type = String(type);
    var base, parentType;
    if (host === owner) {
        if (!_PE_ELEMENT_TYPES.test(type)) return null;
        // `::view-transition` belongs to the root element only.
        if (type === '::view-transition' && owner !== document.documentElement) return null;
        base = type;
    } else {
        var fn = _pe_split_functional(type);
        if (!fn) return null;
        parentType = host._type;
        var allowed = _PE_VT_CHILD[parentType];
        if (!allowed || allowed.indexOf(fn[0]) < 0) return null;
        base = fn[0];
    }
    var slot = _pe_cache.get(host);
    if (!slot) { slot = Object.create(null); _pe_cache.set(host, slot); }
    if (!slot[type]) slot[type] = _pe_make(owner, host, base, type);
    return slot[type];
}

Object.defineProperty(Element.prototype, 'pseudo', {
    value: function pseudo(type) {
        if (arguments.length < 1) throw new TypeError("Failed to execute 'pseudo' on 'Element': 1 argument required, but only 0 present.");
        return _pe_lookup(this, this, type);
    },
    writable: true, enumerable: true, configurable: true
});

[
    ['type', function() { return this._type; }],
    ['element', function() { return this._owner; }],
    ['parent', function() { return this._parent; }],
    ['selectorText', function() { return this._selector; }]
].forEach(function(a) {
    Object.defineProperty(CSSPseudoElement.prototype, a[0], {
        get: a[1], enumerable: true, configurable: true });
});
Object.defineProperty(CSSPseudoElement.prototype, 'pseudo', {
    value: function pseudo(type) {
        if (arguments.length < 1) throw new TypeError("Failed to execute 'pseudo' on 'CSSPseudoElement': 1 argument required, but only 0 present.");
        return _pe_lookup(this, this._owner, type);
    },
    writable: true, enumerable: true, configurable: true
});

// The layout name of a pseudo-element (`::before` → `before`), or null for those that never own a
// box we publish (view-transition pseudos, `::marker`, `::backdrop`).
function _pe_layout_name(pe) {
    var t = pe._type;
    if (t === '::before' || t === '::after' || t === '::scroll-marker' ||
        t === '::scroll-marker-group' || t.indexOf('::scroll-button(') === 0) {
        return t.slice(2);
    }
    return null;
}

// `[x, y, w, h, border×4, padding×4, margin×4]` (viewport CSS px), or null when the
// pseudo-element owns no box.
function _pe_geometry(pe) {
    var name = _pe_layout_name(pe);
    if (name === null) return null;
    var g = _lumen_get_pseudo_geometry(pe._owner.__nid__, name);
    return g ? g : null;
}

function _pe_box_rect(g, box) {
    var x = g[0], y = g[1], w = g[2], h = g[3];
    if (box === 'border') return [x, y, w, h];
    if (box === 'margin') {
        return [x - g[15], y - g[12], w + g[15] + g[13], h + g[12] + g[14]];
    }
    // padding box: inside the border
    x += g[7]; y += g[4]; w -= g[7] + g[5]; h -= g[4] + g[6];
    if (box === 'padding') return [x, y, Math.max(w, 0), Math.max(h, 0)];
    // content box: inside the padding
    x += g[11]; y += g[8]; w -= g[11] + g[9]; h -= g[8] + g[10];
    return [x, y, Math.max(w, 0), Math.max(h, 0)];
}

// Viewport position of a node's origin (the top-left of its border box); null when the node has
// no box. A Document's origin is the page origin, so it moves with the scroll position.
function _pe_origin(node) {
    if (node instanceof CSSPseudoElement) {
        var g = _pe_geometry(node);
        return g ? [g[0], g[1]] : null;
    }
    var type = node.nodeType;
    if (type === 9) return [-(window.scrollX || 0), -(window.scrollY || 0)];
    var nid;
    if (type === 1) nid = node.__nid__;
    else if (type === 3 && node.parentNode) nid = node.parentNode.__nid__;
    else return null;
    var r = _lumen_get_bounding_rect(nid);
    return r ? [r[0], r[1]] : null;
}

function _pe_check_node(node, method) {
    if (!(node instanceof CSSPseudoElement) && !(node instanceof Node)) {
        throw new TypeError("Failed to execute '" + method + "' on 'CSSPseudoElement': parameter 2 is not of type 'Node'.");
    }
}

// Offset that maps coordinates measured from `from`'s origin into `pe`'s; null when either side
// has no box.
function _pe_delta(pe, from) {
    var a = _pe_origin(from), b = _pe_origin(pe);
    return (a && b) ? [a[0] - b[0], a[1] - b[1]] : null;
}

function _pe_shift_point(p, d) {
    p = p || {};
    return { x: (+p.x || 0) + d[0], y: (+p.y || 0) + d[1], z: p.z, w: p.w };
}

var _PE_BOXES = ['margin', 'border', 'padding', 'content'];

var _pe_members = {
    getBoundingClientRect: function() {
        var g = _pe_geometry(this);
        return g ? new DOMRect(g[0], g[1], g[2], g[3]) : new DOMRect(0, 0, 0, 0);
    },
    getClientRects: function() {
        var g = _pe_geometry(this);
        return new DOMRectList(g ? [new DOMRect(g[0], g[1], g[2], g[3])] : []);
    },
    getBoxQuads: function(options) {
        options = options || {};
        var box = options.box === undefined ? 'border' : String(options.box);
        if (_PE_BOXES.indexOf(box) < 0) {
            throw new TypeError("Failed to execute 'getBoxQuads' on 'CSSPseudoElement': The provided value '" +
                box + "' is not a valid enum value of type CSSBoxType.");
        }
        var g = _pe_geometry(this);
        if (!g) return [];
        var r = _pe_box_rect(g, box);
        var rel = options.relativeTo;
        if (rel !== undefined && rel !== null) {
            _pe_check_node(rel, 'getBoxQuads');
            var o = _pe_origin(rel);
            if (!o) return [];
            r = [r[0] - o[0], r[1] - o[1], r[2], r[3]];
        }
        return [DOMQuad.fromRect(new DOMRect(r[0], r[1], r[2], r[3]))];
    },
    convertPointFromNode: function(point, from, options) {
        _pe_check_node(from, 'convertPointFromNode');
        var d = _pe_delta(this, from);
        if (!d) return null;
        var p = _pe_shift_point(point, d);
        return new DOMPoint(p.x, p.y, p.z, p.w);
    },
    convertRectFromNode: function(rect, from, options) {
        _pe_check_node(from, 'convertRectFromNode');
        var d = _pe_delta(this, from);
        if (!d) return null;
        rect = rect || {};
        return DOMQuad.fromRect({ x: (+rect.x || 0) + d[0], y: (+rect.y || 0) + d[1],
                                  width: rect.width, height: rect.height });
    },
    convertQuadFromNode: function(quad, from, options) {
        _pe_check_node(from, 'convertQuadFromNode');
        var d = _pe_delta(this, from);
        if (!d) return null;
        quad = quad || {};
        return new DOMQuad(_pe_shift_point(quad.p1, d), _pe_shift_point(quad.p2, d),
                           _pe_shift_point(quad.p3, d), _pe_shift_point(quad.p4, d));
    }
};
Object.keys(_pe_members).forEach(function(k) {
    Object.defineProperty(CSSPseudoElement.prototype, k, {
        value: _pe_members[k], writable: true, enumerable: true, configurable: true });
});

window.CSSPseudoElement = CSSPseudoElement;
