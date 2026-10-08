// HTML Sanitizer API (WICG sanitizer-api, merged into WHATWG HTML 2026-08;
// algorithm text followed: WICG index.bs @ c0f1ca8, builtins cross-checked
// against tests/wpt/sanitizer-api — BUG-663).
//
// The configuration model is pure data (canonicalize → validate → store); the
// sanitize walk goes through plain DOM members (`childNodes`, `nodeType`,
// `localName`, `namespaceURI`, `attributes`, `removeChild`, `insertBefore`),
// so the same walk serves live arena nodes (`setHTML`) and the detached
// documents `Document.parseHTML*` builds.
(function () {
'use strict';

var HTML_NS = 'http://www.w3.org/1999/xhtml';
var SVG_NS = 'http://www.w3.org/2000/svg';
var MATHML_NS = 'http://www.w3.org/1998/Math/MathML';
var XLINK_NS = 'http://www.w3.org/1999/xlink';

// Built-in safe default configuration, compacted: per namespace a
// space-separated list of `name[:attr,attr…]` (every per-element list is an
// allow list, possibly empty), plus the global attribute allow list.
var DEFAULT_COMPACT = {
  H: 'a:href,hreflang,type abbr address article aside b bdi bdo blockquote:cite body br caption cite code col:span colgroup:span data:value dd del:cite,datetime dfn div dl dt em figcaption figure footer h1 h2 h3 h4 h5 h6 head header hgroup hr html i ins:cite,datetime kbd li:value main mark menu nav ol:reversed,start,type p pre q rp rt ruby s samp search section small span strong sub sup table tbody td:colspan,headers,rowspan tfoot th:abbr,colspan,headers,rowspan,scope thead time:datetime title tr u ul var wbr',
  S: 'a:href,hreflang,type circle:cx,cy,pathLength,r defs desc ellipse:cx,cy,pathLength,rx,ry foreignObject:height,width,x,y g line:pathLength,x1,x2,y1,y2 marker:markerHeight,markerUnits,markerWidth,orient,preserveAspectRatio,refX,refY,viewBox metadata path:d,pathLength polygon:pathLength,points polyline:pathLength,points rect:height,pathLength,rx,ry,width,x,y svg:height,preserveAspectRatio,viewBox,width,x,y text:dx,dy,lengthAdjust,rotate,textLength,x,y textPath:lengthAdjust,method,path,side,spacing,startOffset,textLength title tspan:dx,dy,lengthAdjust,rotate,textLength,x,y',
  M: 'math merror mfrac mi mmultiscripts mn mo:fence,form,largeop,lspace,maxsize,minsize,movablelimits,rspace,separator,stretchy,symmetric mover:accent mpadded:depth,height,lspace,voffset,width mphantom mprescripts mroot mrow ms mspace:depth,height,width msqrt mstyle msub msubsup msup mtable mtd:columnspan,rowspan mtext mtr munder:accentunder munderover:accent,accentunder semantics',
  attributes: 'alignment-baseline baseline-shift clip-path clip-rule color color-interpolation cursor dir direction display displaystyle dominant-baseline fill fill-opacity fill-rule font-family font-size font-size-adjust font-stretch font-style font-variant font-weight lang letter-spacing marker-end marker-mid marker-start mathbackground mathcolor mathsize opacity paint-order pointer-events scriptlevel shape-rendering stop-color stop-opacity stroke stroke-dasharray stroke-dashoffset stroke-linecap stroke-linejoin stroke-miterlimit stroke-opacity stroke-width text-anchor text-decoration text-overflow text-rendering title transform transform-origin unicode-bidi vector-effect visibility white-space word-spacing writing-mode',
};
var NS_KEYS = { H: HTML_NS, S: SVG_NS, M: MATHML_NS };

// Built-in safe baseline configuration (removeElements; its removeAttributes
// is empty) — WPT's copy, which already lists `base`.
var BASELINE_REMOVE_ELEMENTS = [
  [HTML_NS, 'base'], [HTML_NS, 'embed'], [HTML_NS, 'frame'], [HTML_NS, 'iframe'],
  [HTML_NS, 'object'], [HTML_NS, 'script'], [SVG_NS, 'script'], [SVG_NS, 'use'],
];
// HTML "event handler content attributes" — the materialized list the spec
// publishes next to the safe baseline.
var EVENT_HANDLER_ATTRS = 'onafterprint onauxclick onbeforeinput onbeforematch onbeforeprint onbeforeunload onbeforetoggle onblur oncancel oncanplay oncanplaythrough onchange onclick onclose oncontextlost oncontextmenu oncontextrestored oncopy oncuechange oncut ondblclick ondrag ondragend ondragenter ondragleave ondragover ondragstart ondrop ondurationchange onemptied onended onerror onfocus onformdata onhashchange oninput oninvalid onkeydown onkeypress onkeyup onlanguagechange onload onloadeddata onloadedmetadata onloadstart onmessage onmessageerror onmousedown onmouseenter onmouseleave onmousemove onmouseout onmouseover onmouseup onoffline ononline onpagehide onpagereveal onpageshow onpageswap onpaste onpause onplay onplaying onpopstate onprogress onratechange onreset onresize onrejectionhandled onscroll onscrollend onsecuritypolicyviolation onseeked onseeking onselect onslotchange onstalled onstorage onsubmit onsuspend ontimeupdate ontoggle onunhandledrejection onunload onvolumechange onwaiting onwheel'.split(' ');

var NON_REPLACEABLE = [[HTML_NS, 'html'], [SVG_NS, 'svg'], [MATHML_NS, 'math']];
// [element namespace, element name, attribute namespace, attribute name]
var NAVIGATING_URL_ATTRS = [
  [HTML_NS, 'a', null, 'href'], [HTML_NS, 'area', null, 'href'],
  [HTML_NS, 'base', null, 'href'], [HTML_NS, 'button', null, 'formaction'],
  [HTML_NS, 'form', null, 'action'], [HTML_NS, 'input', null, 'formaction'],
  [SVG_NS, 'a', null, 'href'], [SVG_NS, 'a', XLINK_NS, 'href'],
];
var ANIMATING_ELEMENTS = [[SVG_NS, 'animate'], [SVG_NS, 'animateTransform'], [SVG_NS, 'set']];

// ── WebIDL conversions ───────────────────────────────────────────────────────
// `'' + Symbol()` throws TypeError, exactly as the DOMString conversion must.
function toDOMString(v) { return '' + v; }
function isObject(v) { return v !== null && (typeof v === 'object' || typeof v === 'function'); }
function toSequence(v, what) {
  if (!isObject(v) || typeof v[Symbol.iterator] !== 'function') {
    throw new TypeError("Sanitizer: '" + what + "' is not a sequence");
  }
  return Array.from(v);
}
// SanitizerElement / SanitizerAttribute are (DOMString or dictionary); the
// result is already "canonicalize a sanitizer name"-d.
function canonName(v, defaultNs) {
  if (v === null || v === undefined || isObject(v)) {
    var d = v == null ? {} : v;
    var name = d.name;
    if (name === undefined) throw new TypeError("Sanitizer: required member 'name' is missing");
    var ns = d.namespace;
    ns = ns === undefined ? defaultNs : (ns === null ? null : toDOMString(ns));
    name = toDOMString(name);
    if (ns === '') ns = null;
    return { name: name, namespace: ns };
  }
  return { name: toDOMString(v), namespace: defaultNs };
}
function canonAttr(v) { return canonName(v, null); }
function canonElement(v) { return canonName(v, HTML_NS); }
function canonElementWithAttrs(v) {
  var result = canonElement(v);
  if (isObject(v)) {
    var a = v.attributes, r = v.removeAttributes;
    if (a !== undefined) result.attributes = toSequence(a, 'attributes').map(canonAttr);
    if (r !== undefined) result.removeAttributes = toSequence(r, 'removeAttributes').map(canonAttr);
  }
  if (!result.attributes && !result.removeAttributes) result.removeAttributes = [];
  return result;
}
function canonPI(v) {
  if (v === null || v === undefined || isObject(v)) {
    var d = v == null ? {} : v;
    if (d.target === undefined) throw new TypeError("Sanitizer: required member 'target' is missing");
    return { target: toDOMString(d.target) };
  }
  return { target: toDOMString(v) };
}

// ── list helpers (membership = name + namespace) ─────────────────────────────
function same(a, b) { return a.name === b.name && a.namespace === b.namespace; }
function contains(list, item) {
  if (!list) return false;
  for (var i = 0; i < list.length; i++) if (same(list[i], item)) return true;
  return false;
}
function containsTarget(list, target) {
  if (!list) return false;
  for (var i = 0; i < list.length; i++) if (list[i].target === target) return true;
  return false;
}
function hasDuplicates(list) {
  for (var i = 0; i < list.length; i++) {
    for (var j = i + 1; j < list.length; j++) if (same(list[i], list[j])) return true;
  }
  return false;
}
function hasDuplicateTargets(list) {
  for (var i = 0; i < list.length; i++) {
    for (var j = i + 1; j < list.length; j++) if (list[i].target === list[j].target) return true;
  }
  return false;
}
function intersects(a, b) {
  if (!a || !b) return false;
  for (var i = 0; i < a.length; i++) if (contains(b, a[i])) return true;
  return false;
}
// "remove": drops every matching entry in place, reports whether any went.
function removeFrom(list, item) {
  if (!list) return false;
  var removed = false;
  for (var i = list.length - 1; i >= 0; i--) {
    if (same(list[i], item)) { list.splice(i, 1); removed = true; }
  }
  return removed;
}
function addTo(list, item) { if (!contains(list, item)) list.push(item); }
function dedupe(list) { var out = []; list.forEach(function (e) { addTo(out, e); }); return out; }
function difference(a, b) { return a.filter(function (e) { return !contains(b, e); }); }
function intersection(a, b) { return a.filter(function (e) { return contains(b, e); }); }
function isDataAttr(a) { return a.namespace === null && a.name.indexOf('data-') === 0; }
function isNonReplaceable(e) {
  return NON_REPLACEABLE.some(function (n) { return n[0] === e.namespace && n[1] === e.name; });
}
function nameListEquals(a, b) {
  if (!a || !b) return a === b;
  return a.every(function (e) { return contains(b, e); })
      && b.every(function (e) { return contains(a, e); });
}
// Ordered-map equality of two canonical `elements` entries (order-insensitive).
function elementEquals(a, b) {
  return same(a, b)
      && nameListEquals(a.attributes, b.attributes)
      && nameListEquals(a.removeAttributes, b.removeAttributes);
}
function lessThan(a, b) {
  if (a.namespace === null) {
    if (b.namespace !== null) return true;
  } else {
    if (b.namespace === null) return false;
    if (a.namespace < b.namespace) return true;
    if (a.namespace !== b.namespace) return false;
  }
  return a.name < b.name;
}
function sortNames(list) {
  return list.slice().sort(function (a, b) {
    if (lessThan(a, b)) return -1;
    return lessThan(b, a) ? 1 : 0;
  });
}
function byTarget(a, b) {
  if (a.target < b.target) return -1;
  return a.target > b.target ? 1 : 0;
}
function copyName(e) { return { name: e.name, namespace: e.namespace }; }
function copyElement(e) {
  var out = copyName(e);
  if (e.attributes) out.attributes = e.attributes.map(copyName);
  if (e.removeAttributes) out.removeAttributes = e.removeAttributes.map(copyName);
  return out;
}
function copyPI(p) { return { target: p.target }; }
function copyConfig(c) {
  var out = {};
  Object.keys(c).forEach(function (k) {
    var v = c[k];
    if (!Array.isArray(v)) out[k] = v;
    else if (k === 'elements') out[k] = v.map(copyElement);
    else if (k === 'processingInstructions' || k === 'removeProcessingInstructions') out[k] = v.map(copyPI);
    else out[k] = v.map(copyName);
  });
  return out;
}

// ── canonicalize / validate / set a configuration ────────────────────────────
// SanitizerConfig dictionary conversion: members read in lexicographic order
// (WebIDL §3.2.18), absent members stay absent.
function readConfigDict(v) {
  if (v !== null && v !== undefined && !isObject(v)) {
    throw new TypeError('Sanitizer: configuration is not a dictionary');
  }
  var d = v == null ? {} : v;
  var c = {};
  var x;
  if ((x = d.attributes) !== undefined) c.attributes = toSequence(x, 'attributes').map(canonAttr);
  if ((x = d.comments) !== undefined) c.comments = !!x;
  if ((x = d.dataAttributes) !== undefined) c.dataAttributes = !!x;
  if ((x = d.elements) !== undefined) c.elements = toSequence(x, 'elements').map(canonElementWithAttrs);
  if ((x = d.processingInstructions) !== undefined) {
    c.processingInstructions = toSequence(x, 'processingInstructions').map(canonPI);
  }
  if ((x = d.removeAttributes) !== undefined) c.removeAttributes = toSequence(x, 'removeAttributes').map(canonAttr);
  if ((x = d.removeElements) !== undefined) c.removeElements = toSequence(x, 'removeElements').map(canonElement);
  if ((x = d.removeProcessingInstructions) !== undefined) {
    c.removeProcessingInstructions = toSequence(x, 'removeProcessingInstructions').map(canonPI);
  }
  if ((x = d.replaceWithChildrenElements) !== undefined) {
    c.replaceWithChildrenElements = toSequence(x, 'replaceWithChildrenElements').map(canonElement);
  }
  return c;
}
function canonicalize(c, allowCommentsPIsAndDataAttributes) {
  if (!c.elements && !c.removeElements) c.removeElements = [];
  if (!c.processingInstructions && !c.removeProcessingInstructions) {
    if (allowCommentsPIsAndDataAttributes) c.removeProcessingInstructions = [];
    else c.processingInstructions = [];
  }
  if (!c.attributes && !c.removeAttributes) c.removeAttributes = [];
  if (!('comments' in c)) c.comments = allowCommentsPIsAndDataAttributes;
  if (c.attributes && !('dataAttributes' in c)) c.dataAttributes = allowCommentsPIsAndDataAttributes;
  return c;
}
function isValid(c) {
  if (c.elements && c.removeElements) return false;
  if (c.processingInstructions && c.removeProcessingInstructions) return false;
  if (c.attributes && c.removeAttributes) return false;
  if (hasDuplicates(c.elements || c.removeElements)) return false;
  if (c.replaceWithChildrenElements && hasDuplicates(c.replaceWithChildrenElements)) return false;
  if (hasDuplicateTargets(c.processingInstructions || c.removeProcessingInstructions)) return false;
  if (hasDuplicates(c.attributes || c.removeAttributes)) return false;
  if (c.replaceWithChildrenElements) {
    if (c.replaceWithChildrenElements.some(isNonReplaceable)) return false;
    if (intersects(c.elements || c.removeElements, c.replaceWithChildrenElements)) return false;
  }
  var els = c.elements || [];
  var j, e;
  if (c.attributes) {
    for (j = 0; j < els.length; j++) {
      e = els[j];
      if (e.attributes && hasDuplicates(e.attributes)) return false;
      if (e.removeAttributes && hasDuplicates(e.removeAttributes)) return false;
      if (intersects(c.attributes, e.attributes)) return false;
      if (e.removeAttributes && !e.removeAttributes.every(function (a) { return contains(c.attributes, a); })) {
        return false;
      }
      if (c.dataAttributes && e.attributes && e.attributes.some(isDataAttr)) return false;
    }
    if (c.dataAttributes && c.attributes.some(isDataAttr)) return false;
  } else {
    for (j = 0; j < els.length; j++) {
      e = els[j];
      if (e.attributes && e.removeAttributes) return false;
      if (e.attributes && hasDuplicates(e.attributes)) return false;
      if (e.removeAttributes && hasDuplicates(e.removeAttributes)) return false;
      if (intersects(c.removeAttributes, e.attributes)) return false;
      if (intersects(c.removeAttributes, e.removeAttributes)) return false;
    }
    if ('dataAttributes' in c) return false;
  }
  return true;
}

function nullNsAttr(n) { return { name: n, namespace: null }; }
function builtinDefaultConfig() {
  var elements = [];
  ['H', 'S', 'M'].forEach(function (k) {
    DEFAULT_COMPACT[k].split(' ').forEach(function (entry) {
      var p = entry.split(':');
      elements.push({
        name: p[0],
        namespace: NS_KEYS[k],
        attributes: p[1] ? p[1].split(',').map(nullNsAttr) : [],
      });
    });
  });
  return {
    attributes: DEFAULT_COMPACT.attributes.split(' ').map(nullNsAttr),
    comments: false,
    dataAttributes: false,
    elements: elements,
    processingInstructions: [],
  };
}

// ── modifier algorithms shared by the methods and "remove unsafe" ────────────
function removeAnElement(c, element) {
  element = canonElement(element);
  var modified = removeFrom(c.replaceWithChildrenElements, element);
  if (c.elements) {
    if (contains(c.elements, element)) { removeFrom(c.elements, element); return true; }
    return modified;
  }
  if (contains(c.removeElements, element)) return modified;
  addTo(c.removeElements, element);
  return true;
}
function removeAnAttribute(c, attribute) {
  attribute = canonAttr(attribute);
  var els = c.elements || [];
  if (c.attributes) {
    var modified = removeFrom(c.attributes, attribute);
    els.forEach(function (e) {
      if (removeFrom(e.attributes, attribute)) modified = true;
      removeFrom(e.removeAttributes, attribute);
    });
    return modified;
  }
  if (contains(c.removeAttributes, attribute)) return false;
  els.forEach(function (e) {
    removeFrom(e.attributes, attribute);
    removeFrom(e.removeAttributes, attribute);
  });
  c.removeAttributes.push(attribute);
  return true;
}
function removeUnsafe(c) {
  var result = false;
  BASELINE_REMOVE_ELEMENTS.forEach(function (p) {
    if (removeAnElement(c, { name: p[1], namespace: p[0] })) result = true;
  });
  EVENT_HANDLER_ATTRS.forEach(function (n) {
    if (removeAnAttribute(c, nullNsAttr(n))) result = true;
  });
  return result;
}

// ── the Sanitizer interface ──────────────────────────────────────────────────
var CONFIG = new WeakMap();
function configOf(s) {
  var c = isObject(s) ? CONFIG.get(s) : undefined;
  if (!c) throw new TypeError('Illegal invocation');
  return c;
}
// "set a configuration": false for an invalid dictionary.
function setConfiguration(holder, dict, allow) {
  var c = canonicalize(dict, allow);
  if (!isValid(c)) return false;
  CONFIG.set(holder, c);
  return true;
}
// (SanitizerConfig or SanitizerPresets) → dictionary.
function configFromUnion(v, what) {
  if (v === null || isObject(v)) return readConfigDict(v);
  if (toDOMString(v) !== 'default') {
    throw new TypeError(what + ": '" + v + "' is not a valid SanitizerPresets value");
  }
  return builtinDefaultConfig();
}

class Sanitizer {
  constructor(configuration) {
    var dict = configuration === undefined
      ? builtinDefaultConfig()
      : configFromUnion(configuration, 'Sanitizer');
    if (!setConfiguration(this, dict, true)) throw new TypeError('Sanitizer: invalid configuration');
  }

  get() {
    var c = copyConfig(configOf(this));
    if (c.elements) {
      c.elements.forEach(function (e) {
        if (e.attributes) e.attributes = sortNames(e.attributes);
        if (e.removeAttributes) e.removeAttributes = sortNames(e.removeAttributes);
      });
      c.elements = sortNames(c.elements);
    } else {
      c.removeElements = sortNames(c.removeElements);
    }
    if (c.replaceWithChildrenElements) c.replaceWithChildrenElements = sortNames(c.replaceWithChildrenElements);
    if (c.processingInstructions) c.processingInstructions.sort(byTarget);
    else c.removeProcessingInstructions.sort(byTarget);
    if (c.attributes) c.attributes = sortNames(c.attributes);
    else c.removeAttributes = sortNames(c.removeAttributes);
    // A dictionary converts to a JS object with members in lexicographic order.
    var out = {};
    Object.keys(c).sort().forEach(function (k) { out[k] = c[k]; });
    return out;
  }

  allowElement(element) {
    var c = configOf(this);
    element = canonElementWithAttrs(element);
    var modified;
    if (c.elements) {
      modified = removeFrom(c.replaceWithChildrenElements, element);
      // Per-element lists must not overlap the global ones.
      if (c.attributes) {
        if (element.attributes) {
          element.attributes = difference(dedupe(element.attributes), c.attributes);
          if (c.dataAttributes === true) {
            element.attributes = element.attributes.filter(function (a) { return !isDataAttr(a); });
          }
        }
        if (element.removeAttributes) {
          element.removeAttributes = intersection(dedupe(element.removeAttributes), c.attributes);
        }
      } else {
        if (element.attributes) {
          element.attributes = difference(dedupe(element.attributes), element.removeAttributes || []);
          delete element.removeAttributes;
          element.attributes = difference(element.attributes, c.removeAttributes);
        }
        if (element.removeAttributes) {
          element.removeAttributes = difference(dedupe(element.removeAttributes), c.removeAttributes);
        }
      }
      var current = null;
      for (var i = 0; i < c.elements.length; i++) {
        if (same(c.elements[i], element)) current = c.elements[i];
      }
      if (!current) { c.elements.push(element); return true; }
      if (elementEquals(element, current)) return modified;
      removeFrom(c.elements, element);
      c.elements.push(element);
      return true;
    }
    // Per-element attribute lists need a global allow list (the spec lets a
    // UA warn here; returning false is the observable part).
    if (element.attributes || (element.removeAttributes && element.removeAttributes.length)) {
      return false;
    }
    modified = removeFrom(c.replaceWithChildrenElements, element);
    if (!contains(c.removeElements, element)) return modified;
    removeFrom(c.removeElements, element);
    return true;
  }

  removeElement(element) { return removeAnElement(configOf(this), element); }

  replaceElementWithChildren(element) {
    var c = configOf(this);
    element = canonElement(element);
    if (isNonReplaceable(element)) return false;
    if (contains(c.replaceWithChildrenElements, element)) return false;
    removeFrom(c.removeElements, element);
    removeFrom(c.elements, element);
    if (!c.replaceWithChildrenElements) c.replaceWithChildrenElements = [];
    addTo(c.replaceWithChildrenElements, element);
    return true;
  }

  allowProcessingInstruction(pi) {
    var c = configOf(this);
    pi = canonPI(pi);
    if (c.processingInstructions) {
      if (containsTarget(c.processingInstructions, pi.target)) return false;
      c.processingInstructions.push(pi);
      return true;
    }
    if (!containsTarget(c.removeProcessingInstructions, pi.target)) return false;
    c.removeProcessingInstructions = c.removeProcessingInstructions.filter(function (p) {
      return p.target !== pi.target;
    });
    return true;
  }

  removeProcessingInstruction(pi) {
    var c = configOf(this);
    pi = canonPI(pi);
    if (c.processingInstructions) {
      if (!containsTarget(c.processingInstructions, pi.target)) return false;
      c.processingInstructions = c.processingInstructions.filter(function (p) {
        return p.target !== pi.target;
      });
      return true;
    }
    if (containsTarget(c.removeProcessingInstructions, pi.target)) return false;
    c.removeProcessingInstructions.push(pi);
    return true;
  }

  allowAttribute(attribute) {
    var c = configOf(this);
    attribute = canonAttr(attribute);
    if (c.attributes) {
      if (c.dataAttributes === true && isDataAttr(attribute)) return false;
      if (contains(c.attributes, attribute)) return false;
      (c.elements || []).forEach(function (e) { removeFrom(e.attributes, attribute); });
      c.attributes.push(attribute);
      return true;
    }
    return removeFrom(c.removeAttributes, attribute);
  }

  removeAttribute(attribute) { return removeAnAttribute(configOf(this), attribute); }

  setComments(allow) {
    var c = configOf(this);
    allow = !!allow;
    if ('comments' in c && c.comments === allow) return false;
    c.comments = allow;
    return true;
  }

  setDataAttributes(allow) {
    var c = configOf(this);
    allow = !!allow;
    if (!c.attributes) return false;
    if (c.dataAttributes === allow) return false;
    if (allow) {
      c.attributes = c.attributes.filter(function (a) { return !isDataAttr(a); });
      (c.elements || []).forEach(function (e) {
        if (e.attributes) e.attributes = e.attributes.filter(function (a) { return !isDataAttr(a); });
      });
    }
    c.dataAttributes = allow;
    return true;
  }

  removeUnsafe() { return removeUnsafe(configOf(this)); }
}
Object.defineProperty(Sanitizer.prototype, Symbol.toStringTag, { value: 'Sanitizer', configurable: true });

// "get a sanitizer instance from options" — yields the canonical config.
// SetHTMLOptions defaults to the "default" preset, SetHTMLUnsafeOptions to {}.
function sanitizerConfigFromOptions(options, safe, what) {
  if (options !== null && options !== undefined && !isObject(options)) {
    throw new TypeError(what + ': options is not a dictionary');
  }
  var spec = options == null ? undefined : options.sanitizer;
  if (spec === undefined) spec = safe ? 'default' : {};
  if (isObject(spec) && CONFIG.has(spec)) return CONFIG.get(spec);
  var holder = {};
  if (!setConfiguration(holder, configFromUnion(spec, what), !safe)) {
    throw new TypeError(what + ': invalid sanitizer configuration');
  }
  return CONFIG.get(holder);
}

// ── sanitize ─────────────────────────────────────────────────────────────────
function childList(node) {
  var kids = node.childNodes;
  return kids ? Array.prototype.slice.call(kids) : [];
}
function namespaceOf(el) {
  var ns = el.namespaceURI;
  return ns === undefined ? HTML_NS : ns; // DOMParser's virtual nodes carry none
}
function attributeList(el) {
  var out = [];
  var map = el.attributes;
  if (map && typeof map.length === 'number') {
    for (var i = 0; i < map.length; i++) {
      var a = map[i];
      if (!a) continue;
      out.push({
        name: a.localName !== undefined ? a.localName : a.name,
        namespace: a.namespaceURI == null ? null : a.namespaceURI,
        qname: a.name,
        value: a.value,
      });
    }
    return out;
  }
  if (typeof el.getAttributeNames === 'function') {
    el.getAttributeNames().forEach(function (n) {
      out.push({ name: n, namespace: null, qname: n, value: el.getAttribute(n) });
    });
  }
  return out;
}
function dropAttribute(el, a) {
  if (a.namespace !== null && typeof el.removeAttributeNS === 'function') {
    el.removeAttributeNS(a.namespace, a.name);
  } else {
    el.removeAttribute(a.qname);
  }
}
function containsJavascriptUrl(value) {
  var url;
  try { url = new URL(String(value)); } catch (e) { return false; }
  return url.protocol === 'javascript:';
}
function isAttributeAllowed(c, attr, elementName) {
  var local = null;
  if (c.elements) {
    for (var i = 0; i < c.elements.length; i++) {
      if (same(c.elements[i], elementName)) local = c.elements[i];
    }
  }
  if (local && contains(local.removeAttributes, attr)) return false;
  if (c.attributes) {
    if (contains(c.attributes, attr)) return true;
    if (local && contains(local.attributes, attr)) return true;
    return c.dataAttributes === true && isDataAttr(attr);
  }
  if (local && local.attributes && !contains(local.attributes, attr)) return false;
  return !contains(c.removeAttributes, attr);
}
function isNavigatingUrlAttr(elementName, a) {
  return NAVIGATING_URL_ATTRS.some(function (e) {
    return e[0] === elementName.namespace && e[1] === elementName.name
        && e[2] === a.namespace && e[3] === a.name;
  });
}
function isAnimatingHref(elementName, a) {
  var animating = ANIMATING_ELEMENTS.some(function (e) {
    return e[0] === elementName.namespace && e[1] === elementName.name;
  });
  return animating && a.namespace === null && a.name === 'attributeName'
      && (a.value === 'href' || a.value === 'xlink:href');
}
function sanitizeAttributes(child, elementName, c, jsNav) {
  attributeList(child).forEach(function (a) {
    var attrName = { name: a.name, namespace: a.namespace };
    if (!isAttributeAllowed(c, attrName, elementName)) { dropAttribute(child, a); return; }
    if (!jsNav) return;
    if (isNavigatingUrlAttr(elementName, a) && containsJavascriptUrl(a.value)) {
      dropAttribute(child, a);
    } else if (elementName.namespace === MATHML_NS && a.name === 'href'
               && (a.namespace === null || a.namespace === XLINK_NS)
               && containsJavascriptUrl(a.value)) {
      dropAttribute(child, a);
    } else if (isAnimatingHref(elementName, a)) {
      dropAttribute(child, a);
    }
  });
}
function sanitizeCore(node, c, jsNav) {
  childList(node).forEach(function (child) {
    var type = child.nodeType;
    if (type === 8) {
      if (c.comments !== true) node.removeChild(child);
      return;
    }
    if (type === 7) {
      var target = child.target;
      var drop = c.processingInstructions
        ? !containsTarget(c.processingInstructions, target)
        : containsTarget(c.removeProcessingInstructions, target);
      if (drop) node.removeChild(child);
      return;
    }
    if (type !== 1) return; // Text, CDATA, DocumentType stay
    var elementName = { name: child.localName, namespace: namespaceOf(child) };
    if (contains(c.replaceWithChildrenElements, elementName)) {
      sanitizeCore(child, c, jsNav);
      var inner;
      while ((inner = child.childNodes[0])) node.insertBefore(inner, child);
      node.removeChild(child);
      return;
    }
    var blocked = c.elements
      ? !contains(c.elements, elementName)
      : contains(c.removeElements, elementName);
    if (blocked) { node.removeChild(child); return; }
    if (elementName.name === 'template' && elementName.namespace === HTML_NS && child.content) {
      sanitizeCore(child.content, c, jsNav);
    }
    if (child.shadowRoot) sanitizeCore(child.shadowRoot, c, jsNav);
    sanitizeAttributes(child, elementName, c, jsNav);
    sanitizeCore(child, c, jsNav);
  });
}
function sanitize(node, config, safe) {
  var c = config;
  if (safe) { c = copyConfig(config); removeUnsafe(c); }
  sanitizeCore(node, c, safe);
}

// ── set and filter HTML ──────────────────────────────────────────────────────
// Parses into a detached element shaped like the context (same namespace and
// local name, so table / raw-text / foreign-content parsing modes still
// apply), sanitizes there, then moves the survivors into `target` — nothing
// unsanitized ever joins the target's tree.
function parseDetached(contextEl, html) {
  var ns = namespaceOf(contextEl);
  var local = contextEl.localName;
  // A custom-element name would upgrade (run a constructor) on creation; it
  // parses exactly like any other ordinary HTML element.
  if (ns === HTML_NS && local.indexOf('-') !== -1) local = 'div';
  var nid = ns === HTML_NS
    ? _lumen_create_element(local)
    : _lumen_create_element_ns(ns === null ? '' : ns, local);
  _lumen_set_inner_html(nid, html);
  var holder = _lumen_make_element(nid);
  return (ns === HTML_NS && local === 'template') ? holder.content : holder;
}
function setAndFilter(target, contextEl, html, options, safe, what) {
  if (safe && contextEl.localName === 'script') {
    var cns = namespaceOf(contextEl);
    if (cns === HTML_NS || cns === SVG_NS) return;
  }
  var config = sanitizerConfigFromOptions(options, safe, what);
  var holder = parseDetached(contextEl, html);
  sanitize(holder, config, safe);
  childList(target).forEach(function (k) { target.removeChild(k); });
  childList(holder).forEach(function (k) { target.appendChild(k); });
}
function compliantHTML(html, sink) {
  if (typeof _lumen_tt_get_compliant_html === 'function') {
    return _lumen_tt_get_compliant_html(html, sink, true);
  }
  return html === null ? '' : toDOMString(html);
}
function templateTarget(el) {
  var isTemplate = el.localName === 'template' && namespaceOf(el) === HTML_NS;
  return (isTemplate && el.content) ? el.content : el;
}

if (typeof Element !== 'undefined') {
  Element.prototype.setHTML = function setHTML(html, options) {
    setAndFilter(templateTarget(this), this, toDOMString(html), options, true, 'Element.setHTML');
  };
  Element.prototype.setHTMLUnsafe = function setHTMLUnsafe(html, options) {
    var s = compliantHTML(html, 'Element setHTMLUnsafe');
    setAndFilter(templateTarget(this), this, s, options, false, 'Element.setHTMLUnsafe');
  };
}
if (typeof ShadowRoot !== 'undefined') {
  // The shadow host stands in as the parsing context.
  ShadowRoot.prototype.setHTML = function setHTML(html, options) {
    setAndFilter(this, this.host, toDOMString(html), options, true, 'ShadowRoot.setHTML');
  };
  ShadowRoot.prototype.setHTMLUnsafe = function setHTMLUnsafe(html, options) {
    var s = compliantHTML(html, 'ShadowRoot setHTMLUnsafe');
    setAndFilter(this, this.host, s, options, false, 'ShadowRoot.setHTMLUnsafe');
  };
}
// `Document.parseHTMLUnsafe` from dom_parser.rs (installed earlier) builds the
// detached document; both statics then sanitize it in place.
if (typeof Document !== 'undefined' && typeof Document.parseHTMLUnsafe === 'function') {
  var buildDocument = Document.parseHTMLUnsafe;
  Document.parseHTMLUnsafe = function parseHTMLUnsafe(html, options) {
    var s = compliantHTML(html, 'Document parseHTMLUnsafe');
    var config = sanitizerConfigFromOptions(options, false, 'Document.parseHTMLUnsafe');
    var doc = buildDocument(s);
    sanitize(doc, config, false);
    return doc;
  };
  Document.parseHTML = function parseHTML(html, options) {
    var s = toDOMString(html);
    var config = sanitizerConfigFromOptions(options, true, 'Document.parseHTML');
    var doc = buildDocument(s);
    sanitize(doc, config, true);
    return doc;
  };
}

globalThis.Sanitizer = Sanitizer;
})();
