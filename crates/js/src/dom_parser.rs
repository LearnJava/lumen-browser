//! DOMParser + XMLSerializer — W3C DOM Parsing and Serialization spec.
//!
//! **DOMParser** (§11.4): `new DOMParser().parseFromString(html, mimeType)`
//! returns a virtual Document built by a lightweight pure-JS HTML tokenizer.
//! The returned Document is independent of the page DOM — it is backed by plain
//! JS objects, not Rust native nodes.
//!
//! Supported MIME types: `text/html`, `application/xml`, `text/xml`,
//! `application/xhtml+xml`, `image/svg+xml`.  The four XML types take a distinct
//! parse path (BUG-781): names keep their case, the first top-level element
//! becomes `documentElement` with no `<html>`/`<head>`/`<body>` synthesis,
//! CR/CRLF are normalized to LF before parsing (XML 1.0 §2.11), and a fatal
//! well-formedness error yields a `<parsererror>` document rather than a throw.
//!
//! **XMLSerializer** (§2.4): `new XMLSerializer().serializeToString(node)`
//! serializes a node to a string.  Handles two node types:
//! - Virtual nodes (from `DOMParser`) — full round-trip serialization.
//! - Native nodes (live page DOM, have `__nid__`) — uses `_lumen_get_attr_names`,
//!   `_lumen_get_attr`, `_lumen_get_children`, `_lumen_get_tag_name`,
//!   `_lumen_is_text_node`, `_lumen_get_text_content`.
//!
//! Phase 0: complete structural parsing + serialization.
//! Phase 1: namespace-aware XML output, `responseXML` integration in XHR.
//!
//! Not yet implemented:
//! - Namespace-qualified serialization (`xmlns` attribute injection); `prefix`
//!   and `localName` are split off the qualified name, but `namespaceURI` is not
//!   resolved from the in-scope `xmlns` declarations
//! - Entity resolution beyond the common ~30 HTML entities; an undeclared entity
//!   is left verbatim instead of being a fatal XML error
//! - `serializeToString` for `ProcessingInstruction` / `DocumentType` nodes

/// Install DOMParser and XMLSerializer into a V8 runtime (Ph3 V8 migration
/// S5-S7; the rquickjs twin was removed in S12b-B23).
///
/// Must be called after `v8_runtime.rs::install_dom` so that `_lumen_is_text_node`,
/// `_lumen_get_tag_name`, `_lumen_get_children`, `_lumen_get_attr`,
/// `_lumen_get_attr_names`, and `_lumen_get_text_content` are registered.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_dom_parser_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(DOM_PARSER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const DOM_PARSER_SHIM: &str = r#"
(function() {
'use strict';

// ── Node type constants ───────────────────────────────────────────────────────
var ELEMENT_NODE           = 1;
var TEXT_NODE              = 3;
var COMMENT_NODE           = 8;
var DOCUMENT_NODE          = 9;
var DOCUMENT_FRAGMENT_NODE = 11;

// HTML void elements — must not have closing tag in HTML serialization
var VOID_ELEMS = {
  area:1,base:1,br:1,col:1,embed:1,hr:1,img:1,input:1,
  link:1,meta:1,param:1,source:1,track:1,wbr:1
};

// ── VNode (virtual DOM node) ──────────────────────────────────────────────────
// Represents a DOM node backed by plain JS objects (not Rust native nodes).
// Used by DOMParser to return an independent document.

function VNode(nodeType, doc) {
  this.nodeType    = nodeType;
  this.ownerDocument = doc;
  this.parentNode  = null;
  this.childNodes  = [];
}

// ── VElement ─────────────────────────────────────────────────────────────────
function VElement(tagName, doc, isXML) {
  VNode.call(this, ELEMENT_NODE, doc);
  if (isXML) {
    // XML names are case-sensitive (XML 1.0 §2.3) and may carry a prefix, so the
    // qualified name is kept verbatim: `tagName`/`nodeName` are NOT upper-cased.
    var qn = String(tagName);
    var ci = qn.indexOf(':');
    this._xml      = true;
    this._qname    = qn;
    this.prefix    = ci > 0 ? qn.slice(0, ci) : null;
    this.localName = ci > 0 ? qn.slice(ci + 1) : qn;
    this.tagName   = qn;
    this.nodeName  = qn;
  } else {
    this.localName = tagName.toLowerCase();
    this.tagName   = this.localName.toUpperCase();
    this.nodeName  = this.tagName;
  }
  this.nodeValue = null;
  this._attrs    = Object.create(null); // name (lc in HTML, verbatim in XML) → value
  this._attrOrd  = [];                  // insertion-ordered attr names
}

// Attribute names are ASCII-lower-cased in HTML documents and case-sensitive in
// XML ones — every attribute accessor funnels its key through this.
function _vAttrKey(el, n) { return el._xml ? String(n) : String(n).toLowerCase(); }
VElement.prototype = Object.create(VNode.prototype);
VElement.prototype.constructor = VElement;

Object.defineProperty(VElement.prototype, 'textContent', {
  get: function() { return _vCollectText(this); },
  set: function(v) {
    this.childNodes = [];
    if (v !== '' && v != null) {
      var t = new VText(String(v), this.ownerDocument);
      t.parentNode = this;
      this.childNodes.push(t);
    }
  }
});
Object.defineProperty(VElement.prototype, 'innerHTML', {
  get: function() { return _vSerializeChildren(this, false); },
  set: function(v) {
    this.childNodes = [];
    var frag = _vParseFragment(String(v), this.ownerDocument);
    for (var i = 0; i < frag.childNodes.length; i++) {
      frag.childNodes[i].parentNode = this;
      this.childNodes.push(frag.childNodes[i]);
    }
  }
});
Object.defineProperty(VElement.prototype, 'outerHTML', {
  get: function() { return _vSerializeElement(this, false); }
});
Object.defineProperty(VElement.prototype, 'children', {
  get: function() { return this.childNodes.filter(function(n) { return n.nodeType === ELEMENT_NODE; }); }
});
Object.defineProperty(VElement.prototype, 'firstChild', {
  get: function() { return this.childNodes.length ? this.childNodes[0] : null; }
});
Object.defineProperty(VElement.prototype, 'lastChild', {
  get: function() { return this.childNodes.length ? this.childNodes[this.childNodes.length - 1] : null; }
});
Object.defineProperty(VElement.prototype, 'firstElementChild', {
  get: function() {
    for (var i = 0; i < this.childNodes.length; i++) {
      if (this.childNodes[i].nodeType === ELEMENT_NODE) return this.childNodes[i];
    }
    return null;
  }
});
Object.defineProperty(VElement.prototype, 'lastElementChild', {
  get: function() {
    for (var i = this.childNodes.length - 1; i >= 0; i--) {
      if (this.childNodes[i].nodeType === ELEMENT_NODE) return this.childNodes[i];
    }
    return null;
  }
});
Object.defineProperty(VElement.prototype, 'nextSibling', {
  get: function() {
    if (!this.parentNode) return null;
    var idx = this.parentNode.childNodes.indexOf(this);
    return idx < this.parentNode.childNodes.length - 1 ? this.parentNode.childNodes[idx + 1] : null;
  }
});
Object.defineProperty(VElement.prototype, 'previousSibling', {
  get: function() {
    if (!this.parentNode) return null;
    var idx = this.parentNode.childNodes.indexOf(this);
    return idx > 0 ? this.parentNode.childNodes[idx - 1] : null;
  }
});

VElement.prototype.getAttribute    = function(n) {
  var v = this._attrs[_vAttrKey(this, n)];
  return v !== undefined ? v : null;
};
VElement.prototype.setAttribute    = function(n, v) {
  var lc = _vAttrKey(this, n);
  if (!(lc in this._attrs)) this._attrOrd.push(lc);
  this._attrs[lc] = String(v);
};
VElement.prototype.hasAttribute    = function(n) { return _vAttrKey(this, n) in this._attrs; };
VElement.prototype.removeAttribute = function(n) {
  var lc = _vAttrKey(this, n);
  delete this._attrs[lc];
  var idx = this._attrOrd.indexOf(lc);
  if (idx !== -1) this._attrOrd.splice(idx, 1);
};
VElement.prototype.getAttributeNames = function() { return this._attrOrd.slice(); };
VElement.prototype.toggleAttribute = function(n, force) {
  if (force === undefined) force = !this.hasAttribute(n);
  if (force) this.setAttribute(n, ''); else this.removeAttribute(n);
  return force;
};

VElement.prototype.appendChild  = _vAppendChild;
VElement.prototype.removeChild  = _vRemoveChild;
VElement.prototype.insertBefore = _vInsertBefore;
VElement.prototype.replaceChild = function(newChild, oldChild) {
  var idx = this.childNodes.indexOf(oldChild);
  if (idx === -1) throw new Error('Node not found');
  this.childNodes.splice(idx, 1, newChild);
  if (oldChild.parentNode === this) oldChild.parentNode = null;
  newChild.parentNode = this;
  return oldChild;
};
VElement.prototype.cloneNode    = function(deep) {
  var c = new VElement(this._xml ? this._qname : this.localName, this.ownerDocument, this._xml);
  for (var i = 0; i < this._attrOrd.length; i++) {
    var k = this._attrOrd[i];
    c._attrs[k] = this._attrs[k];
    c._attrOrd.push(k);
  }
  if (deep) {
    for (var j = 0; j < this.childNodes.length; j++) c.appendChild(this.childNodes[j].cloneNode(true));
  }
  return c;
};
VElement.prototype.querySelector         = function(sel) { return _vQuerySelector(this, sel, false); };
VElement.prototype.querySelectorAll      = function(sel) { return _vQuerySelector(this, sel, true); };
VElement.prototype.getElementsByTagName  = function(t) { return _vGetByTag(this, t); };
VElement.prototype.getElementsByClassName = function(c) { return _vGetByClass(this, c); };
VElement.prototype.getElementById        = function(id) { return _vQuerySelector(this, '#' + id, false); };
VElement.prototype.matches               = function(sel) { return _vMatchesComplex(this, sel, null); };
VElement.prototype.closest               = function(sel) {
  var n = this;
  while (n && n.nodeType === ELEMENT_NODE) {
    if (_vMatchesComplex(n, sel, null)) return n;
    n = n.parentNode;
  }
  return null;
};
// Convenience: dispatchEvent / addEventListener no-ops (Phase 0)
VElement.prototype.dispatchEvent      = function() { return true; };
VElement.prototype.addEventListener   = function() {};
VElement.prototype.removeEventListener = function() {};

// ── VText ─────────────────────────────────────────────────────────────────────
function VText(data, doc) {
  VNode.call(this, TEXT_NODE, doc);
  this.nodeName  = '#text';
  this.nodeValue = data;
  this.data      = data;
}
VText.prototype = Object.create(VNode.prototype);
VText.prototype.constructor = VText;
Object.defineProperty(VText.prototype, 'textContent', {
  get: function() { return this.nodeValue || ''; },
  set: function(v) { this.nodeValue = this.data = String(v); }
});
Object.defineProperty(VText.prototype, 'nextSibling', {
  get: VElement.prototype.__lookupGetter__('nextSibling') || function() {
    if (!this.parentNode) return null;
    var idx = this.parentNode.childNodes.indexOf(this);
    return idx < this.parentNode.childNodes.length - 1 ? this.parentNode.childNodes[idx + 1] : null;
  }
});
VText.prototype.cloneNode = function() { return new VText(this.nodeValue, this.ownerDocument); };

// ── VComment ─────────────────────────────────────────────────────────────────
function VComment(data, doc) {
  VNode.call(this, COMMENT_NODE, doc);
  this.nodeName  = '#comment';
  this.nodeValue = data;
  this.data      = data;
}
VComment.prototype = Object.create(VNode.prototype);
VComment.prototype.constructor = VComment;
VComment.prototype.cloneNode = function() { return new VComment(this.nodeValue, this.ownerDocument); };

// ── VDocument ────────────────────────────────────────────────────────────────
function VDocument() {
  VNode.call(this, DOCUMENT_NODE, this);
  this.nodeName        = '#document';
  this.nodeValue       = null;
  this.documentElement = null;
  this.head            = null;
  this.body            = null;
  this.doctype         = null;
  this.URL             = 'about:blank';
  this.contentType     = 'text/html';
  this._isXML          = false;
}
VDocument.prototype = Object.create(VNode.prototype);
VDocument.prototype.constructor = VDocument;
Object.defineProperty(VDocument.prototype, 'textContent', { get: function() { return null; } });
Object.defineProperty(VDocument.prototype, 'children', {
  get: function() { return this.childNodes.filter(function(n) { return n.nodeType === ELEMENT_NODE; }); }
});
Object.defineProperty(VDocument.prototype, 'firstChild', {
  get: function() { return this.childNodes.length ? this.childNodes[0] : null; }
});
Object.defineProperty(VDocument.prototype, 'lastChild', {
  get: function() { return this.childNodes.length ? this.childNodes[this.childNodes.length - 1] : null; }
});
Object.defineProperty(VDocument.prototype, 'innerHTML', {
  get: function() { return _vSerializeChildren(this, false); }
});

// DOM §4.5.1: `createElement` lower-cases the name only for HTML documents.
VDocument.prototype.createElement        = function(t) { return new VElement(t, this, this._isXML); };
VDocument.prototype.createTextNode       = function(d) { return new VText(String(d), this); };
VDocument.prototype.createComment        = function(d) { return new VComment(String(d), this); };
VDocument.prototype.createDocumentFragment = function() {
  var f = new VNode(DOCUMENT_FRAGMENT_NODE, this);
  f.nodeName = '#document-fragment'; f.nodeValue = null;
  return f;
};
VDocument.prototype.appendChild          = _vAppendChild;
VDocument.prototype.removeChild          = _vRemoveChild;
VDocument.prototype.insertBefore         = _vInsertBefore;
VDocument.prototype.querySelector        = function(s) { return _vQuerySelector(this, s, false); };
VDocument.prototype.querySelectorAll     = function(s) { return _vQuerySelector(this, s, true); };
VDocument.prototype.getElementsByTagName  = function(t) { return _vGetByTag(this, t); };
VDocument.prototype.getElementsByClassName = function(c) { return _vGetByClass(this, c); };
VDocument.prototype.getElementById       = function(id) { return _vQuerySelector(this, '#' + id, false); };
VDocument.prototype.dispatchEvent        = function() { return true; };
VDocument.prototype.addEventListener     = function() {};
VDocument.prototype.removeEventListener  = function() {};

// ── Shared tree-mutation helpers ─────────────────────────────────────────────
function _vAppendChild(child) {
  if (!child) return child;
  // DocumentFragment: transfer children
  if (child.nodeType === DOCUMENT_FRAGMENT_NODE) {
    for (var i = 0; i < child.childNodes.length; i++) {
      child.childNodes[i].parentNode = this;
      this.childNodes.push(child.childNodes[i]);
    }
    child.childNodes = [];
    return child;
  }
  if (child.parentNode) _vRemoveChild.call(child.parentNode, child);
  child.parentNode = this;
  this.childNodes.push(child);
  return child;
}
function _vRemoveChild(child) {
  var idx = this.childNodes.indexOf(child);
  if (idx !== -1) { this.childNodes.splice(idx, 1); child.parentNode = null; }
  return child;
}
function _vInsertBefore(newNode, ref) {
  if (!ref) return _vAppendChild.call(this, newNode);
  var idx = this.childNodes.indexOf(ref);
  if (idx === -1) return _vAppendChild.call(this, newNode);
  if (newNode.parentNode) _vRemoveChild.call(newNode.parentNode, newNode);
  newNode.parentNode = this;
  this.childNodes.splice(idx, 0, newNode);
  return newNode;
}

// ── Text content collector ───────────────────────────────────────────────────
function _vCollectText(node) {
  if (!node) return '';
  if (node.nodeType === TEXT_NODE) return node.nodeValue || '';
  if (node.nodeType === COMMENT_NODE) return '';
  var r = '';
  for (var i = 0; i < node.childNodes.length; i++) r += _vCollectText(node.childNodes[i]);
  return r;
}

// ── HTML entity table (common subset) ────────────────────────────────────────
var _ENT = {
  amp:'&',lt:'<',gt:'>',quot:'"',apos:"'",nbsp:' ',
  copy:'©',reg:'®',trade:'™',mdash:'—',
  ndash:'–',laquo:'«',raquo:'»',ldquo:'“',
  rdquo:'”',lsquo:'‘',rsquo:'’',hellip:'…',
  euro:'€',pound:'£',yen:'¥',cent:'¢',
  times:'×',divide:'÷',plusmn:'±',frac12:'½',
  frac14:'¼',frac34:'¾',deg:'°',micro:'µ',
  acute:'´',uml:'¨',cedil:'¸',macr:'¯',
  lfloor:'⌊',rfloor:'⌋',lceil:'⌈',rceil:'⌉',
  infin:'∞',sum:'∑',prod:'∏',radic:'√',
  and:'∧',or:'∨',not:'¬',ne:'≠',le:'≤',
  ge:'≥',sub:'⊂',sup:'⊃',forall:'∀',exist:'∃',
  empty:'∅',there4:'∴',cong:'≅',asymp:'≈',
  prime:'′',Prime:'″',loz:'◊',spades:'♠',
  clubs:'♣',hearts:'♥',diams:'♦',larr:'←',
  rarr:'→',darr:'↓',uarr:'↑',harr:'↔',
  crarr:'↵',lArr:'⇐',rArr:'⇒',uArr:'⇑',dArr:'⇓',
  hArr:'⇔',alpha:'α',beta:'β',gamma:'γ',delta:'δ',
  epsilon:'ε',zeta:'ζ',eta:'η',theta:'θ',iota:'ι',
  kappa:'κ',lambda:'λ',mu:'μ',nu:'ν',xi:'ξ',
  omicron:'ο',pi:'π',rho:'ρ',sigma:'σ',tau:'τ',
  upsilon:'υ',phi:'φ',chi:'χ',psi:'ψ',omega:'ω'
};
function _decEnt(str) {
  if (!str || str.indexOf('&') === -1) return str;
  return str.replace(/&(?:#(\d+)|#x([0-9a-fA-F]+)|([a-zA-Z]+));?/g, function(m, dec, hex, name) {
    if (dec) return String.fromCodePoint(parseInt(dec, 10));
    if (hex) return String.fromCodePoint(parseInt(hex, 16));
    return _ENT[name] || m;
  });
}

// ── HTML / XML tokenizer / tree builder ──────────────────────────────────────
// State-machine that iterates over the source string character by character,
// building a VNode tree into `root`.
//
// `isXML` switches the four places where XML differs from HTML: names keep their
// case, a closing tag must match the innermost open element exactly, there are
// no void elements (only `<x/>` self-closes) and `<script>`/`<style>` have no
// raw-text mode.  In XML mode a well-formedness violation throws a marked Error
// that `_vBuildXMLDocument` turns into a `parsererror` document.

function _vXmlWFError(msg) {
  var e = new Error(msg);
  e.__vXmlWF = true;
  return e;
}

function _vParseHTML(html, doc, isXML) {
  var root = new VNode(DOCUMENT_FRAGMENT_NODE, doc);
  root.childNodes = [];
  root.nodeName   = '#document-fragment';
  root.nodeValue  = null;
  var stack = [root];
  var pos = 0;
  var len = html ? html.length : 0;

  function cur() { return stack[stack.length - 1]; }

  function addText(text) {
    if (!text) return;
    var dec = _decEnt(text);
    if (!dec) return;
    var p = cur();
    var last = p.childNodes.length ? p.childNodes[p.childNodes.length - 1] : null;
    if (last && last.nodeType === TEXT_NODE) {
      last.nodeValue += dec;
      last.data = last.nodeValue;
    } else {
      var t = new VText(dec, doc);
      t.parentNode = p;
      p.childNodes.push(t);
    }
  }

  while (pos < len) {
    var lt = html.indexOf('<', pos);
    if (lt === -1) { addText(html.slice(pos)); break; }
    if (lt > pos) addText(html.slice(pos, lt));
    pos = lt;

    // Comment <!-- ... -->
    if (html.charCodeAt(pos+1) === 33 && html.charCodeAt(pos+2) === 45 && html.charCodeAt(pos+3) === 45) {
      var ce = html.indexOf('-->', pos + 4);
      if (ce === -1) ce = len - 3;
      var cmt = new VComment(html.slice(pos + 4, ce), doc);
      cmt.parentNode = cur();
      cur().childNodes.push(cmt);
      pos = ce + 3;
      continue;
    }
    // CDATA <![CDATA[...]]>
    if (html.slice(pos, pos + 9) === '<![CDATA[') {
      var cd = html.indexOf(']]>', pos + 9);
      if (cd === -1) cd = len - 3;
      addText(html.slice(pos + 9, cd));
      pos = cd + 3;
      continue;
    }
    // Declaration <!...>  (DOCTYPE, etc.)
    if (html.charCodeAt(pos+1) === 33) {
      var de = html.indexOf('>', pos + 2);
      pos = de === -1 ? len : de + 1;
      continue;
    }
    // Processing instruction <?...?>
    if (html.charCodeAt(pos+1) === 63) {
      var pe = html.indexOf('?>', pos + 2);
      pos = pe === -1 ? len : pe + 2;
      continue;
    }
    // Closing tag </tag>
    if (html.charCodeAt(pos+1) === 47) {
      var ge = html.indexOf('>', pos + 2);
      if (ge === -1) { addText('</'); pos += 2; continue; }
      var clRaw = html.slice(pos + 2, ge).trim();
      if (isXML) {
        var top = stack[stack.length - 1];
        if (!top || top.nodeType !== ELEMENT_NODE || top._qname !== clRaw) {
          throw _vXmlWFError('Opening and ending tag mismatch: expected </' +
            (top && top._qname ? top._qname : '') + '>, got </' + clRaw + '>');
        }
        stack.pop();
        pos = ge + 1;
        continue;
      }
      var clTag = clRaw.toLowerCase();
      for (var si = stack.length - 1; si > 0; si--) {
        var sn = stack[si];
        if (sn.nodeType === ELEMENT_NODE && sn.localName === clTag) {
          stack.length = si;
          break;
        }
      }
      pos = ge + 1;
      continue;
    }
    // Opening tag <tag ...>
    var ts = pos + 1;
    var p2 = ts;
    while (p2 < len && !/[\s\/>]/.test(html[p2])) p2++;
    var tagRawN = html.slice(ts, p2);
    var tagN = isXML ? tagRawN : tagRawN.toLowerCase();
    var nameOk = isXML ? /^[A-Za-z_][A-Za-z0-9\-:_.]*$/.test(tagN)
                       : /^[a-z][a-z0-9\-:_.]*$/.test(tagN);
    if (!tagN || !nameOk) {
      if (isXML) throw _vXmlWFError('StartTag: invalid element name');
      addText('<'); pos++; continue;
    }

    var el = new VElement(tagN, doc, isXML);
    // Parse attributes
    while (p2 < len) {
      while (p2 < len && /\s/.test(html[p2])) p2++;
      if (p2 >= len || html[p2] === '>' || html[p2] === '/') break;
      var as = p2;
      while (p2 < len && !/[\s=\/>]/.test(html[p2])) p2++;
      var aRawN = html.slice(as, p2);
      var aN = isXML ? aRawN : aRawN.toLowerCase();
      if (!aN) { p2++; continue; }
      while (p2 < len && /\s/.test(html[p2])) p2++;
      var aV = '';
      if (p2 < len && html[p2] === '=') {
        p2++;
        while (p2 < len && /\s/.test(html[p2])) p2++;
        if (p2 < len && (html[p2] === '"' || html[p2] === "'")) {
          var q = html[p2]; p2++;
          var vS = p2;
          while (p2 < len && html[p2] !== q) p2++;
          aV = _decEnt(html.slice(vS, p2));
          if (p2 < len) p2++;
        } else {
          var uvS = p2;
          while (p2 < len && !/[\s>\/]/.test(html[p2])) p2++;
          aV = _decEnt(html.slice(uvS, p2));
        }
      }
      el.setAttribute(aN, aV);
    }

    var selfC = p2 < len && html[p2] === '/';
    if (selfC) p2++;
    if (p2 < len && html[p2] === '>') p2++;
    pos = p2;

    var par = cur();
    el.parentNode = par;
    par.childNodes.push(el);

    if (!selfC && (isXML || !VOID_ELEMS[tagN])) {
      stack.push(el);
      // Raw text mode: script / style — consume until closing tag verbatim.
      // XML has no raw-text elements: their content is ordinary markup there.
      if (!isXML && (tagN === 'script' || tagN === 'style')) {
        var closeTag2 = '</' + tagN;
        var rawEnd = html.toLowerCase().indexOf(closeTag2, pos);
        var rawContent = '';
        if (rawEnd !== -1) {
          rawContent = html.slice(pos, rawEnd);
          var gtRaw = html.indexOf('>', rawEnd + closeTag2.length);
          pos = gtRaw !== -1 ? gtRaw + 1 : len;
        } else {
          rawContent = html.slice(pos);
          pos = len;
        }
        if (rawContent) {
          var rt = new VText(rawContent, doc);
          rt.parentNode = el;
          el.childNodes.push(rt);
        }
        if (stack[stack.length - 1] === el) stack.pop();
      }
    }
  }
  if (isXML && stack.length > 1) {
    throw _vXmlWFError('Premature end of data in tag ' + (stack[stack.length - 1]._qname || ''));
  }
  return root;
}

// MIME types DOMParser must parse with the XML tokenizer rather than the HTML one.
var XML_MIMES = {
  'application/xml':1, 'text/xml':1, 'application/xhtml+xml':1, 'image/svg+xml':1
};

// XML 1.0 §2.8 — VersionNum is `'1.' [0-9]+`; anything else is a fatal error.
// Returns an error message, or null when the prolog (if any) is acceptable.
function _vCheckXMLProlog(src) {
  var m = /^\s*<\?xml\s+version\s*=\s*(?:"([^"]*)"|'([^']*)')/.exec(src);
  if (!m) return null;
  var ver = m[1] !== undefined ? m[1] : m[2];
  if (!/^1\.[0-9]+$/.test(ver)) return 'XML declaration: unsupported version "' + ver + '"';
  return null;
}

// DOM Parsing §8.2 — a fatal XML error yields a document whose root is
// `<parsererror>` in the Mozilla namespace, not an exception.
function _vXMLErrorDocument(msg, mimeType) {
  var doc = new VDocument();
  doc.contentType = mimeType;
  doc._isXML = true;
  var el = new VElement('parsererror', doc, true);
  el.setAttribute('xmlns', 'http://www.mozilla.org/newlayout/xml/parsererror.xml');
  var t = new VText(String(msg), doc);
  t.parentNode = el;
  el.childNodes.push(t);
  doc.appendChild(el);
  doc.documentElement = el;
  return doc;
}

// Build a VDocument for an XML MIME type: the real root element is the document
// element — no `<html>`/`<head>`/`<body>` synthesis, and `doc.head`/`doc.body`
// stay null as they must for a non-HTML document.
function _vBuildXMLDocument(xml, mimeType) {
  // XML 1.0 §2.11 — normalize CRLF and lone CR to LF before parsing.
  var src = String(xml).replace(/\r\n?/g, '\n');

  var prologErr = _vCheckXMLProlog(src);
  if (prologErr) return _vXMLErrorDocument(prologErr, mimeType);

  var doc = new VDocument();
  doc.contentType = mimeType;
  doc._isXML = true;

  var root;
  try {
    root = _vParseHTML(src, doc, true);
  } catch (e) {
    if (e && e.__vXmlWF) return _vXMLErrorDocument(e.message, mimeType);
    throw e;
  }

  var kids = root.childNodes.slice();
  var docEl = null;
  for (var i = 0; i < kids.length; i++) {
    var n = kids[i];
    if (n.nodeType === ELEMENT_NODE) {
      if (docEl) return _vXMLErrorDocument('Extra content at the end of the document', mimeType);
      docEl = n;
    } else if (n.nodeType === TEXT_NODE && /\S/.test(n.nodeValue || '')) {
      // Character data outside the root element is not well-formed.
      return _vXMLErrorDocument('Extra content at the end of the document', mimeType);
    }
  }
  if (!docEl) return _vXMLErrorDocument('Document is empty', mimeType);

  // Keep the root and any comments around it; whitespace between them is Misc.
  for (var k = 0; k < kids.length; k++) {
    if (kids[k].nodeType === ELEMENT_NODE || kids[k].nodeType === COMMENT_NODE) {
      doc.appendChild(kids[k]);
    }
  }
  doc.documentElement = docEl;
  return doc;
}

// Build a full VDocument (html/head/body structure for HTML, real root for XML)
function _vBuildDocument(html, mimeType) {
  var mt = mimeType || 'text/html';
  if (XML_MIMES[mt]) return _vBuildXMLDocument(html, mt);
  var doc = new VDocument();
  doc.contentType = mt;
  var root = _vParseHTML(html, doc);

  // Find or synthesize html/head/body
  var htmlEl = null;
  for (var i = 0; i < root.childNodes.length; i++) {
    var n = root.childNodes[i];
    if (n.nodeType === ELEMENT_NODE && n.localName === 'html') { htmlEl = n; break; }
  }
  var headEl = null, bodyEl = null;
  if (htmlEl) {
    for (var j = 0; j < htmlEl.childNodes.length; j++) {
      var c = htmlEl.childNodes[j];
      if (c.nodeType !== ELEMENT_NODE) continue;
      if (c.localName === 'head') headEl = c;
      else if (c.localName === 'body') bodyEl = c;
    }
    doc.appendChild(htmlEl);
  } else {
    // Wrap bare content in html/head/body
    htmlEl = doc.createElement('html');
    headEl = doc.createElement('head');
    bodyEl = doc.createElement('body');
    htmlEl.appendChild(headEl);
    htmlEl.appendChild(bodyEl);
    var rootKids = root.childNodes.slice(); // snapshot — appendChild mutates root.childNodes
    for (var k = 0; k < rootKids.length; k++) {
      bodyEl.appendChild(rootKids[k]);
    }
    doc.appendChild(htmlEl);
  }
  if (!headEl) { headEl = doc.createElement('head'); htmlEl.childNodes.unshift(headEl); headEl.parentNode = htmlEl; }
  if (!bodyEl) { bodyEl = doc.createElement('body'); htmlEl.appendChild(bodyEl); }

  doc.documentElement = htmlEl;
  doc.head = headEl;
  doc.body = bodyEl;
  htmlEl.parentNode = doc;
  return doc;
}

// Parse an HTML fragment (no html/head/body wrapping)
function _vParseFragment(html, doc) {
  var frag = new VNode(DOCUMENT_FRAGMENT_NODE, doc);
  frag.nodeName = '#document-fragment';
  frag.nodeValue = null;
  var result = _vParseHTML(html, doc);
  frag.childNodes = result.childNodes;
  for (var i = 0; i < frag.childNodes.length; i++) frag.childNodes[i].parentNode = frag;
  return frag;
}

// ── Serialization helpers ─────────────────────────────────────────────────────
function _escH(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }
function _escA(s) { return String(s).replace(/&/g,'&amp;').replace(/"/g,'&quot;'); }

function _vSerializeChildren(node, isXML) {
  var r = '';
  for (var i = 0; i < node.childNodes.length; i++) r += _vSerializeNode(node.childNodes[i], isXML);
  return r;
}
function _vSerializeNode(node, isXML) {
  if (!node) return '';
  switch (node.nodeType) {
    case TEXT_NODE:     return _escH(node.nodeValue || '');
    case COMMENT_NODE:  return '<!--' + (node.nodeValue || '') + '-->';
    case ELEMENT_NODE:  return _vSerializeElement(node, isXML);
    case DOCUMENT_NODE:
    case DOCUMENT_FRAGMENT_NODE: return _vSerializeChildren(node, isXML);
    default:            return '';
  }
}
function _vSerializeElement(el, isXML) {
  // An XML-parsed element round-trips under its qualified name, case intact.
  var tag = el._xml ? el._qname : el.localName;
  var r = '<' + tag;
  for (var i = 0; i < el._attrOrd.length; i++) {
    var n = el._attrOrd[i];
    r += ' ' + n + '="' + _escA(el._attrs[n]) + '"';
  }
  var isVoid = !isXML && VOID_ELEMS[tag];
  if (isVoid) { r += '>'; return r; }
  if (isXML && !el.childNodes.length) { r += '/>'; return r; }
  r += '>';
  r += _vSerializeChildren(el, isXML);
  r += '</' + tag + '>';
  return r;
}

// Serialize a native DOM node backed by Rust (has __nid__)
function _nativeSerializeNode(node) {
  if (!node || node.__nid__ === undefined) return '';
  var nid = node.__nid__;
  if (typeof _lumen_is_text_node === 'function' && _lumen_is_text_node(nid)) {
    return _escH(typeof _lumen_get_text_content === 'function' ? _lumen_get_text_content(nid) : '');
  }
  var tagRaw = typeof _lumen_get_tag_name === 'function' ? (_lumen_get_tag_name(nid) || '') : '';
  // '#text', '#comment', '#document', '#document-fragment' — descend into children
  if (tagRaw.charAt(0) === '#') {
    var kids2 = typeof _lumen_get_children === 'function' ? _lumen_get_children(nid) : [];
    return kids2.map(function(cid) { return _nativeSerializeNode({__nid__: cid}); }).join('');
  }
  var tag2 = tagRaw.toLowerCase();
  var out = '<' + tag2;
  if (typeof _lumen_get_attr_names === 'function') {
    var attrNames = _lumen_get_attr_names(nid);
    for (var i = 0; i < attrNames.length; i++) {
      var v = typeof _lumen_get_attr === 'function' ? _lumen_get_attr(nid, attrNames[i]) : undefined;
      if (v !== undefined) out += ' ' + attrNames[i] + '="' + _escA(v) + '"';
    }
  }
  if (VOID_ELEMS[tag2]) { out += '>'; return out; }
  out += '>';
  var kids3 = typeof _lumen_get_children === 'function' ? _lumen_get_children(nid) : [];
  for (var j = 0; j < kids3.length; j++) out += _nativeSerializeNode({__nid__: kids3[j]});
  out += '</' + tag2 + '>';
  return out;
}

// ── CSS selector engine ───────────────────────────────────────────────────────
// Supports: tag, .class, #id, [attr], [attr=val], [attr^=val], [attr$=val],
// [attr*=val], [attr~=val], [attr|=val], combinators ' ' and '>',
// multi-selector ',', :not() (single simple selector inside).

function _vQuerySelector(root, sel, all) {
  var results = [];
  var segs = sel.split(',');
  for (var s = 0; s < segs.length; s++) segs[s] = segs[s].trim();

  function walk(node, skipRoot) {
    if (!skipRoot && node.nodeType === ELEMENT_NODE) {
      for (var s = 0; s < segs.length; s++) {
        if (_vMatchesComplex(node, segs[s], root)) {
          results.push(node);
          if (!all) return true;
          break;
        }
      }
    }
    for (var i = 0; i < node.childNodes.length; i++) {
      if (walk(node.childNodes[i], false)) return true;
    }
    return false;
  }
  walk(root, true);
  return all ? results : (results[0] || null);
}

function _vMatchesComplex(node, sel, scope) {
  sel = sel.trim();
  if (!sel) return false;
  // Split into parts by ' ' (descendant) and '>' (child)
  var parts = _vSplitCombinators(sel);
  if (!parts || !parts.length) return false;
  var last = parts[parts.length - 1];
  if (!_vMatchSimple(node, last.sel)) return false;
  if (parts.length === 1) return true;
  var combinator = last.comb || ' ';
  var restParts = parts.slice(0, parts.length - 1);
  var restSel = restParts.map(function(p) { return (p.comb ? p.comb + ' ' : '') + p.sel; }).join(' ').trim();
  if (combinator === '>') {
    var par = node.parentNode;
    if (!par || par === scope || par.nodeType !== ELEMENT_NODE) return false;
    return _vMatchesComplex(par, restSel, scope);
  }
  // Descendant combinator
  var anc = node.parentNode;
  while (anc && anc !== scope) {
    if (anc.nodeType === ELEMENT_NODE && _vMatchesComplex(anc, restSel, scope)) return true;
    anc = anc.parentNode;
  }
  return false;
}

function _vSplitCombinators(sel) {
  var parts = [];
  var i = 0, cur2 = '', comb = null;
  while (i <= sel.length) {
    var ch = i < sel.length ? sel[i] : null;
    // Handle attribute brackets — treat content as opaque
    if (ch === '[') {
      var end = sel.indexOf(']', i + 1);
      if (end === -1) end = sel.length - 1;
      cur2 += sel.slice(i, end + 1);
      i = end + 1;
      continue;
    }
    if (ch === null || ch === ' ' || ch === '>') {
      if (cur2) { parts.push({ sel: cur2, comb: comb }); cur2 = ''; comb = null; }
      if (ch === '>') { comb = '>'; i++; while (i < sel.length && sel[i] === ' ') i++; continue; }
      else if (ch === ' ') { if (!comb) comb = ' '; }
    } else {
      cur2 += ch;
    }
    i++;
  }
  return parts;
}

function _vMatchSimple(node, sel) {
  if (node.nodeType !== ELEMENT_NODE || !sel) return false;
  sel = sel.trim();
  if (sel === '*') return true;

  var i = 0, tag = '', id = null, classes = [], attrs = [], notSels = [];

  // Tag name
  if (i < sel.length && /[a-zA-Z*]/.test(sel[i])) {
    var ts = i;
    while (i < sel.length && /[a-zA-Z0-9\-_]/.test(sel[i])) i++;
    tag = sel.slice(ts, i);
  }

  while (i < sel.length) {
    var c = sel[i];
    if (c === '#') {
      i++;
      var ids = i;
      while (i < sel.length && /[a-zA-Z0-9\-_]/.test(sel[i])) i++;
      id = sel.slice(ids, i);
    } else if (c === '.') {
      i++;
      var cs2 = i;
      while (i < sel.length && /[a-zA-Z0-9\-_]/.test(sel[i])) i++;
      classes.push(sel.slice(cs2, i));
    } else if (c === '[') {
      i++;
      var bs = i;
      while (i < sel.length && sel[i] !== ']') i++;
      attrs.push(sel.slice(bs, i));
      if (sel[i] === ']') i++;
    } else if (sel.slice(i, i + 5) === ':not(') {
      i += 5;
      var ns = i, dep = 1;
      while (i < sel.length && dep > 0) {
        if (sel[i] === '(') dep++;
        else if (sel[i] === ')') dep--;
        if (dep > 0) i++;
      }
      notSels.push(sel.slice(ns, i));
      if (sel[i] === ')') i++;
    } else if (c === ':') {
      // Skip pseudo-classes/elements
      i++;
      while (i < sel.length && /[a-zA-Z\-]/.test(sel[i])) i++;
      if (i < sel.length && sel[i] === '(') {
        var dep2 = 1; i++;
        while (i < sel.length && dep2 > 0) {
          if (sel[i] === '(') dep2++;
          else if (sel[i] === ')') dep2--;
          i++;
        }
      }
    } else { i++; }
  }

  if (tag && tag !== '*') {
    // XML tag names are case-sensitive and matched as qualified names.
    if (node._xml ? node._qname !== tag : node.localName !== tag.toLowerCase()) return false;
  }
  if (id !== null && node.getAttribute('id') !== id) return false;
  if (classes.length) {
    var nc = (node.getAttribute('class') || '').split(/\s+/);
    for (var ci = 0; ci < classes.length; ci++) if (nc.indexOf(classes[ci]) === -1) return false;
  }
  for (var ai = 0; ai < attrs.length; ai++) {
    var spec = attrs[ai], eq = -1, op = '=';
    // Find operator position
    for (var k = 0; k < spec.length; k++) {
      if (spec[k] === '=' && k > 0) {
        var prev = spec[k - 1];
        if (prev === '~' || prev === '|' || prev === '^' || prev === '$' || prev === '*') {
          op = prev; eq = k - 1;
        } else { op = '='; eq = k; }
        break;
      }
    }
    if (eq === -1) {
      if (!node.hasAttribute(spec.trim())) return false;
    } else {
      var an = spec.slice(0, eq).trim();
      var av = spec.slice(eq + (op === '=' ? 1 : 2)).replace(/^["']|["']$/g, '');
      var nv = node.getAttribute(an) || '';
      if (op === '=' && nv !== av) return false;
      if (op === '~' && (' ' + nv + ' ').indexOf(' ' + av + ' ') === -1) return false;
      if (op === '|' && nv !== av && nv.indexOf(av + '-') !== 0) return false;
      if (op === '^' && nv.indexOf(av) !== 0) return false;
      if (op === '$' && nv.lastIndexOf(av) !== nv.length - av.length) return false;
      if (op === '*' && nv.indexOf(av) === -1) return false;
    }
  }
  for (var ni = 0; ni < notSels.length; ni++) {
    if (_vMatchSimple(node, notSels[ni])) return false;
  }
  return true;
}

function _vGetByTag(root, tag) {
  var all = tag === '*', lc = all ? null : tag.toLowerCase(), r = [];
  function w(n) {
    if (n.nodeType === ELEMENT_NODE) {
      if (all || (n._xml ? n._qname === tag : n.localName === lc)) r.push(n);
    }
    for (var i = 0; i < n.childNodes.length; i++) w(n.childNodes[i]);
  }
  for (var i = 0; i < root.childNodes.length; i++) w(root.childNodes[i]);
  return r;
}
function _vGetByClass(root, cls) {
  var clss = cls.split(/\s+/).filter(Boolean), r = [];
  function w(n) {
    if (n.nodeType === ELEMENT_NODE) {
      var nc = (n.getAttribute('class') || '').split(/\s+/);
      if (clss.every(function(c) { return nc.indexOf(c) !== -1; })) r.push(n);
    }
    for (var i = 0; i < n.childNodes.length; i++) w(n.childNodes[i]);
  }
  for (var i = 0; i < root.childNodes.length; i++) w(root.childNodes[i]);
  return r;
}

// ── DOMParser ─────────────────────────────────────────────────────────────────
// W3C DOM Parsing and Serialization §11.4

function DOMParser() {}

DOMParser.prototype.parseFromString = function(str, type) {
  if (typeof str !== 'string') str = String(str != null ? str : '');
  var mimeType = typeof type === 'string' ? type : 'text/html';
  var valid = {
    'text/html':1,'application/xml':1,'text/xml':1,
    'application/xhtml+xml':1,'image/svg+xml':1
  };
  if (!valid[mimeType]) {
    throw new TypeError('DOMParser.parseFromString: unsupported MIME type "' + mimeType + '"');
  }
  return _vBuildDocument(str, mimeType);
};

// ── XMLSerializer ─────────────────────────────────────────────────────────────
// W3C DOM Parsing and Serialization §2.4

function XMLSerializer() {}

XMLSerializer.prototype.serializeToString = function(node) {
  if (node == null) throw new TypeError('XMLSerializer.serializeToString: node is null');
  // Native node (live page DOM — has __nid__)
  if (node.__nid__ !== undefined) return _nativeSerializeNode(node);
  // Virtual node (from DOMParser)
  return _vSerializeNode(node, true);
};

// ── Export ────────────────────────────────────────────────────────────────────
globalThis.DOMParser    = DOMParser;
globalThis.XMLSerializer = XMLSerializer;
if (typeof window !== 'undefined') {
  window.DOMParser    = DOMParser;
  window.XMLSerializer = XMLSerializer;
}

})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic, clippy::unwrap_used)]
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    use crate::v8_runtime::V8JsRuntime;

    use super::*;

    fn setup() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        // Minimal stubs for window / navigator / document
        rt.eval(
            r#"
            var window = globalThis;
            var navigator = {};
            var document = {};
            "#,
        )
        .unwrap();
        install_dom_parser_v8(&rt).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    fn string_eval(rt: &V8JsRuntime, expr: &str) -> String {
        match rt.eval(expr).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    fn number_eval(rt: &V8JsRuntime, expr: &str) -> f64 {
        match rt.eval(expr).unwrap() {
            JsValue::Number(n) => n,
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn dom_parser_class_exists() {
        let rt = setup();
        assert!(bool_eval(&rt, "typeof DOMParser === 'function'"));
    }

    #[test]
    fn xml_serializer_class_exists() {
        let rt = setup();
        assert!(bool_eval(&rt, "typeof XMLSerializer === 'function'"));
    }

    #[test]
    fn dom_parser_constructor() {
        let rt = setup();
        assert!(bool_eval(&rt, "new DOMParser() instanceof DOMParser"));
    }

    #[test]
    fn parse_from_string_returns_document() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var p = new DOMParser();
            var doc = p.parseFromString('<p>hello</p>', 'text/html');
            doc !== null && typeof doc === 'object' && doc.nodeType === 9
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_from_string_has_body() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString('<p>hello</p>', 'text/html');
            doc.body !== null && doc.body.nodeType === 1
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_from_string_query_selector() {
        let rt = setup();
        let text = string_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<div><p class="x">hello</p></div>', 'text/html');
            var p = doc.querySelector('.x');
            p ? p.textContent : ''
            "#,
        );
        assert_eq!(text, "hello");
    }

    #[test]
    fn parse_from_string_attributes() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<a href="https://example.com" id="lnk">click</a>', 'text/html');
            var a = doc.getElementById('lnk');
            a !== null && a.getAttribute('href') === 'https://example.com'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_from_string_nested_structure() {
        let rt = setup();
        let count = number_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<ul><li>a</li><li>b</li><li>c</li></ul>', 'text/html');
            doc.querySelectorAll('li').length
            "#,
        );
        assert_eq!(count, 3.0);
    }

    #[test]
    fn xml_serializer_round_trip() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<div id="x">hello</div>', 'text/html');
            var el = doc.getElementById('x');
            var s = new XMLSerializer().serializeToString(el);
            s.indexOf('id="x"') !== -1 && s.indexOf('hello') !== -1
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn xml_serializer_constructor() {
        let rt = setup();
        assert!(bool_eval(
            &rt,
            "new XMLSerializer() instanceof XMLSerializer"
        ));
    }

    #[test]
    fn parse_from_string_text_content() {
        let rt = setup();
        let text = string_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<html><body><h1>Title</h1><p>Paragraph</p></body></html>',
                'text/html'
            );
            doc.body.textContent
            "#,
        );
        assert!(text.contains("Title"));
        assert!(text.contains("Paragraph"));
    }

    #[test]
    fn parse_from_string_xml_mime() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<root><item>1</item></root>', 'application/xml');
            doc !== null && doc.nodeType === 9
            "#,
        );
        assert!(ok);
    }

    /// BUG-781: an XML MIME type must produce the real root element, not the
    /// synthetic `<html><head></head><body>…` wrapper the HTML path builds.
    #[test]
    fn xml_document_element_is_the_real_root() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var d = new DOMParser().parseFromString(
                '<?xml version="1.0"?>\n<a><b>x</b></a>', 'text/xml');
            d.documentElement.tagName === 'a'
              && d.documentElement.nodeName === 'a'
              && d.head === null && d.body === null
              && d.documentElement.firstChild.tagName === 'b'
            "#,
        );
        assert!(ok);
    }

    /// XML 1.0 §2.3 — element and attribute names are case-sensitive.
    #[test]
    fn xml_names_keep_their_case() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var d = new DOMParser().parseFromString(
                '<Root fooBar="1"><Child/></Root>', 'application/xml');
            var r = d.documentElement;
            r.tagName === 'Root'
              && r.localName === 'Root'
              && r.getAttribute('fooBar') === '1'
              && r.getAttribute('foobar') === null
              && r.getElementsByTagName('Child').length === 1
              && r.getElementsByTagName('child').length === 0
            "#,
        );
        assert!(ok);
    }

    /// A prefixed name keeps the qualified form in `tagName` and splits into
    /// `prefix` / `localName`; serialization round-trips the qualified name.
    #[test]
    fn xml_qualified_name_splits_and_round_trips() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var d = new DOMParser().parseFromString(
                '<h:root xmlns:h="urn:x"><h:kid/></h:root>', 'text/xml');
            var r = d.documentElement;
            r.tagName === 'h:root' && r.prefix === 'h' && r.localName === 'root'
              && new XMLSerializer().serializeToString(r).indexOf('<h:root') === 0
            "#,
        );
        assert!(ok);
    }

    /// XML 1.0 §2.11 — CRLF and a lone CR are normalized to LF before parsing.
    #[test]
    fn xml_eol_normalization() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            function first(x) {
              return new DOMParser().parseFromString(x, 'text/xml')
                .documentElement.firstChild.nodeValue;
            }
            first('<a>\r\n\t<b>x</b></a>') === '\n\t'
              && first('<a>\r\t<b>x</b></a>') === '\n\t'
              && first('<a>\r\r\n\n</a>') === '\n\n\n'
            "#,
        );
        assert!(ok);
    }

    /// XML 1.0 §2.8 — `VersionNum` is `'1.' [0-9]+`; anything else is fatal and
    /// yields a `parsererror` document instead of the requested tree.
    #[test]
    fn xml_prolog_version_is_validated() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            function root(v) {
              return new DOMParser()
                .parseFromString('<?xml version="' + v + '"?><x></x>', 'text/xml')
                .documentElement.tagName;
            }
            ['1.0','1.1','1.2','1.7','1.1075','1.000'].every(function(v) { return root(v) === 'x'; })
              && ['10.0','100','2.0','17.0'].every(function(v) { return root(v) === 'parsererror'; })
            "#,
        );
        assert!(ok);
    }

    /// A well-formedness violation is reported as a `parsererror` document
    /// (DOM Parsing §8.2), never as a thrown exception.
    #[test]
    fn xml_ill_formed_input_yields_parsererror() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            function root(x) {
              return new DOMParser().parseFromString(x, 'text/xml').documentElement.tagName;
            }
            root('<a></b>') === 'parsererror'          // mismatched end tag
              && root('<a><b></a>') === 'parsererror'  // unclosed child
              && root('<a/>') === 'a'                  // self-closing is fine
              && root('') === 'parsererror'            // no root element
              && root('<a/><b/>') === 'parsererror'    // two roots
              && root('<a><br/></a>') === 'a'          // no void elements in XML
            "#,
        );
        assert!(ok);
    }

    /// The HTML path is untouched: `text/html` still wraps bare content and
    /// still lower-cases names.
    #[test]
    fn html_path_still_wraps_and_lowercases() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var d = new DOMParser().parseFromString('<A FooBar="1">x</A>', 'text/html');
            d.documentElement.tagName === 'HTML'
              && d.body.firstChild.tagName === 'A'
              && d.body.firstChild.getAttribute('foobar') === '1'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_from_string_invalid_mime_throws() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            try {
              new DOMParser().parseFromString('x', 'text/csv');
              false
            } catch(e) {
              e instanceof TypeError
            }
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn document_create_element() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString('', 'text/html');
            var el = doc.createElement('span');
            el.nodeType === 1 && el.tagName === 'SPAN'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn element_inner_html_set_get() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString('<div></div>', 'text/html');
            var div = doc.querySelector('div');
            div.innerHTML = '<span>hi</span>';
            div.querySelector('span') !== null &&
            div.querySelector('span').textContent === 'hi'
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn serializer_void_elements() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<img src="x.png" alt="img"><br>', 'text/html');
            var s = new XMLSerializer().serializeToString(doc.body || doc);
            s.indexOf('<img') !== -1
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn document_get_elements_by_tag_name() {
        let rt = setup();
        let count = number_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<p>a</p><p>b</p><p>c</p>', 'text/html');
            doc.getElementsByTagName('p').length
            "#,
        );
        assert_eq!(count, 3.0);
    }

    #[test]
    fn document_get_elements_by_class_name() {
        let rt = setup();
        let count = number_eval(
            &rt,
            r#"
            var doc = new DOMParser().parseFromString(
                '<p class="a b">1</p><p class="a">2</p><p class="b">3</p>', 'text/html');
            doc.getElementsByClassName('a').length
            "#,
        );
        assert_eq!(count, 2.0);
    }

    #[test]
    fn exported_on_window() {
        let rt = setup();
        let ok = bool_eval(
            &rt,
            r#"
            typeof window.DOMParser === 'function' &&
            typeof window.XMLSerializer === 'function'
            "#,
        );
        assert!(ok);
    }
}
