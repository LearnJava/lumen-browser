//! DOMParser + XMLSerializer — W3C DOM Parsing and Serialization spec.
//!
//! **DOMParser** (§11.4): `new DOMParser().parseFromString(html, mimeType)`
//! returns a virtual Document built by a lightweight pure-JS HTML tokenizer.
//! The returned Document is independent of the page DOM — it is backed by plain
//! JS objects, not Rust native nodes.
//!
//! Supported MIME types: `text/html`, `application/xml`, `text/xml`,
//! `application/xhtml+xml`, `image/svg+xml`.
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
//! - Namespace-qualified serialization (`xmlns` attribute injection)
//! - Entity resolution beyond the common ~30 HTML entities
//! - XML error-document on parse failure for XML MIME types
//! - `serializeToString` for `ProcessingInstruction` / `DocumentType` nodes

use rquickjs::Ctx;

/// Install DOMParser and XMLSerializer into the JS context.
///
/// Must be called after `dom::install_dom_api` so that `_lumen_is_text_node`,
/// `_lumen_get_tag_name`, `_lumen_get_children`, `_lumen_get_attr`,
/// `_lumen_get_attr_names`, and `_lumen_get_text_content` are registered.
pub fn install_dom_parser(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(DOM_PARSER_SHIM)?;
    Ok(())
}

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
function VElement(tagName, doc) {
  VNode.call(this, ELEMENT_NODE, doc);
  this.localName = tagName.toLowerCase();
  this.tagName   = this.localName.toUpperCase();
  this.nodeName  = this.tagName;
  this.nodeValue = null;
  this._attrs    = Object.create(null); // name (lc) → value
  this._attrOrd  = [];                  // insertion-ordered attr names
}
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
  var v = this._attrs[String(n).toLowerCase()];
  return v !== undefined ? v : null;
};
VElement.prototype.setAttribute    = function(n, v) {
  var lc = String(n).toLowerCase();
  if (!(lc in this._attrs)) this._attrOrd.push(lc);
  this._attrs[lc] = String(v);
};
VElement.prototype.hasAttribute    = function(n) { return String(n).toLowerCase() in this._attrs; };
VElement.prototype.removeAttribute = function(n) {
  var lc = String(n).toLowerCase();
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
  var c = new VElement(this.localName, this.ownerDocument);
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

VDocument.prototype.createElement        = function(t) { return new VElement(t, this); };
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

// ── HTML tokenizer / tree builder ────────────────────────────────────────────
// State-machine that iterates over the HTML string character by character,
// building a VNode tree into `root`.

function _vParseHTML(html, doc) {
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
      var clTag = html.slice(pos + 2, ge).trim().toLowerCase();
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
    var tagN = html.slice(ts, p2).toLowerCase();
    if (!tagN || !/^[a-z][a-z0-9\-:_.]*$/.test(tagN)) {
      addText('<'); pos++; continue;
    }

    var el = new VElement(tagN, doc);
    // Parse attributes
    while (p2 < len) {
      while (p2 < len && /\s/.test(html[p2])) p2++;
      if (p2 >= len || html[p2] === '>' || html[p2] === '/') break;
      var as = p2;
      while (p2 < len && !/[\s=\/>]/.test(html[p2])) p2++;
      var aN = html.slice(as, p2).toLowerCase();
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

    if (!selfC && !VOID_ELEMS[tagN]) {
      stack.push(el);
      // Raw text mode: script / style — consume until closing tag verbatim
      if (tagN === 'script' || tagN === 'style') {
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
  return root;
}

// Build a full VDocument (html/head/body structure)
function _vBuildDocument(html, mimeType) {
  var doc = new VDocument();
  doc.contentType = mimeType || 'text/html';
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
  var tag = el.localName;
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

  if (tag && tag !== '*' && node.localName !== tag.toLowerCase()) return false;
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
  var r = [], lc = tag === '*' ? null : tag.toLowerCase();
  function w(n) {
    if (n.nodeType === ELEMENT_NODE) {
      if (!lc || n.localName === lc) r.push(n);
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn setup(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal stubs for window / navigator / document
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                var document = {};
                "#,
            )
            .unwrap();
            install_dom_parser(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn dom_parser_class_exists() {
        setup(|ctx| {
            let ok: bool = ctx.eval("typeof DOMParser === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn xml_serializer_class_exists() {
        setup(|ctx| {
            let ok: bool = ctx.eval("typeof XMLSerializer === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn dom_parser_constructor() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval("new DOMParser() instanceof DOMParser")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_returns_document() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var p = new DOMParser();
                    var doc = p.parseFromString('<p>hello</p>', 'text/html');
                    doc !== null && typeof doc === 'object' && doc.nodeType === 9
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_has_body() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString('<p>hello</p>', 'text/html');
                    doc.body !== null && doc.body.nodeType === 1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_query_selector() {
        setup(|ctx| {
            let text: String = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<div><p class="x">hello</p></div>', 'text/html');
                    var p = doc.querySelector('.x');
                    p ? p.textContent : ''
                    "#,
                )
                .unwrap();
            assert_eq!(text, "hello");
        });
    }

    #[test]
    fn parse_from_string_attributes() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<a href="https://example.com" id="lnk">click</a>', 'text/html');
                    var a = doc.getElementById('lnk');
                    a !== null && a.getAttribute('href') === 'https://example.com'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_nested_structure() {
        setup(|ctx| {
            let count: i32 = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<ul><li>a</li><li>b</li><li>c</li></ul>', 'text/html');
                    doc.querySelectorAll('li').length
                    "#,
                )
                .unwrap();
            assert_eq!(count, 3);
        });
    }

    #[test]
    fn xml_serializer_round_trip() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<div id="x">hello</div>', 'text/html');
                    var el = doc.getElementById('x');
                    var s = new XMLSerializer().serializeToString(el);
                    s.indexOf('id="x"') !== -1 && s.indexOf('hello') !== -1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn xml_serializer_constructor() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval("new XMLSerializer() instanceof XMLSerializer")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_text_content() {
        setup(|ctx| {
            let text: String = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<html><body><h1>Title</h1><p>Paragraph</p></body></html>',
                        'text/html'
                    );
                    doc.body.textContent
                    "#,
                )
                .unwrap();
            assert!(text.contains("Title"));
            assert!(text.contains("Paragraph"));
        });
    }

    #[test]
    fn parse_from_string_xml_mime() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<root><item>1</item></root>', 'application/xml');
                    doc !== null && doc.nodeType === 9
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn parse_from_string_invalid_mime_throws() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    try {
                      new DOMParser().parseFromString('x', 'text/csv');
                      false
                    } catch(e) {
                      e instanceof TypeError
                    }
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn document_create_element() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString('', 'text/html');
                    var el = doc.createElement('span');
                    el.nodeType === 1 && el.tagName === 'SPAN'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn element_inner_html_set_get() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString('<div></div>', 'text/html');
                    var div = doc.querySelector('div');
                    div.innerHTML = '<span>hi</span>';
                    div.querySelector('span') !== null &&
                    div.querySelector('span').textContent === 'hi'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn serializer_void_elements() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<img src="x.png" alt="img"><br>', 'text/html');
                    var s = new XMLSerializer().serializeToString(doc.body || doc);
                    s.indexOf('<img') !== -1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn document_get_elements_by_tag_name() {
        setup(|ctx| {
            let count: i32 = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<p>a</p><p>b</p><p>c</p>', 'text/html');
                    doc.getElementsByTagName('p').length
                    "#,
                )
                .unwrap();
            assert_eq!(count, 3);
        });
    }

    #[test]
    fn document_get_elements_by_class_name() {
        setup(|ctx| {
            let count: i32 = ctx
                .eval(
                    r#"
                    var doc = new DOMParser().parseFromString(
                        '<p class="a b">1</p><p class="a">2</p><p class="b">3</p>', 'text/html');
                    doc.getElementsByClassName('a').length
                    "#,
                )
                .unwrap();
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn exported_on_window() {
        setup(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    typeof window.DOMParser === 'function' &&
                    typeof window.XMLSerializer === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
