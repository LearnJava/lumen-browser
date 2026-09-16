//! XPath 1.0 (<https://www.w3.org/TR/1999/REC-xpath-19991116/>) — `document.evaluate`,
//! `XPathEvaluator`, `XPathExpression`, `XPathResult`, `XPathException` (GAP-XPATH, BUG-891).
//!
//! Scope decision (2026-09-16, roadmap review): GAP-XPATH bundled XPath *and* XSLT.
//! `XSLTProcessor` is a separate, much larger transform engine and is explicitly
//! OUT OF SCOPE here — same kind of call as SMIL being declared out of scope
//! elsewhere in `ROADMAP.md`. This file is XPath addressing only.
//!
//! Implementation is pure JS evaluated into the global scope (same shape as
//! [`crate::svg`]/[`crate::dom_parser`]: one native install fn, one big JS string,
//! no per-feature Rust binding) — the evaluator walks the standard DOM interface
//! (`childNodes`/`parentNode`/`attributes`/`nodeType`/`nodeName`/`compareDocumentPosition`)
//! rather than reaching into `_lumen_*` natives directly, so it works uniformly over
//! any DOM-shaped tree without depending on internal node representations.
//!
//! Known gap: `document.evaluate` is patched onto the shared `Document.prototype`
//! (covers the live page document and `document.implementation.createDocument`/
//! `createHTMLDocument` output), but `DOMParser().parseFromString()` builds a fully
//! separate, closure-private `VDocument` in [`crate::dom_parser`] that this file
//! cannot reach from the outside — XPath on a parsed-from-string document is not
//! wired. Left for a follow-up slice, the same incremental-scope style used
//! throughout this roadmap.

/// Install XPath 1.0 (`document.evaluate`, `XPathEvaluator`, `XPathExpression`,
/// `XPathResult`, `XPathException`) into a V8 runtime. Must run after the core
/// DOM shim (`Document.prototype`, `Node.prototype.lookupNamespaceURI`,
/// `Node.prototype.compareDocumentPosition`, `DOMException`) is installed.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_xpath_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(XPATH_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const XPATH_SHIM: &str = r#"
(function() {
  'use strict';

  // ── XPathException (DOM4 §XPathEvaluator) ─────────────────────────────────
  function XPathException(code, message) {
    this.code = code;
    this.message = message || '';
    this.name = code === XPathException.TYPE_ERR ? 'TypeError' : 'InvalidExpressionError';
  }
  XPathException.prototype = Object.create(Error.prototype);
  XPathException.prototype.constructor = XPathException;
  XPathException.prototype.toString = function() { return 'XPathException: ' + this.message; };
  XPathException.INVALID_EXPRESSION_ERR = 51;
  XPathException.TYPE_ERR = 52;
  window.XPathException = XPathException;

  function syntaxError(msg) { return new XPathException(XPathException.INVALID_EXPRESSION_ERR, msg); }
  function typeErr(msg) { return new XPathException(XPathException.TYPE_ERR, msg); }

  // ── Tokenizer ──────────────────────────────────────────────────────────────
  var NAME_START = /[A-Za-z_]/;
  var NAME_PART = /[A-Za-z0-9_.\-]/;

  function tokenize(str) {
    var tokens = [];
    var i = 0;
    var n = str.length;
    while (i < n) {
      var ch = str[i];
      if (ch === ' ' || ch === '\t' || ch === '\r' || ch === '\n') { i++; continue; }
      if (ch === '(') { tokens.push({ type: 'lparen' }); i++; continue; }
      if (ch === ')') { tokens.push({ type: 'rparen' }); i++; continue; }
      if (ch === '[') { tokens.push({ type: 'lbracket' }); i++; continue; }
      if (ch === ']') { tokens.push({ type: 'rbracket' }); i++; continue; }
      if (ch === ',') { tokens.push({ type: 'comma' }); i++; continue; }
      if (ch === '@') { tokens.push({ type: 'at' }); i++; continue; }
      if (ch === '|') { tokens.push({ type: 'pipe' }); i++; continue; }
      if (ch === '+') { tokens.push({ type: 'plus' }); i++; continue; }
      if (ch === '$') { tokens.push({ type: 'dollar' }); i++; continue; }
      if (ch === '-') { tokens.push({ type: 'minus' }); i++; continue; }
      if (ch === '*') { tokens.push({ type: 'star' }); i++; continue; }
      if (ch === '=') { tokens.push({ type: 'eq' }); i++; continue; }
      if (ch === '!') {
        if (str[i + 1] === '=') { tokens.push({ type: 'ne' }); i += 2; continue; }
        throw syntaxError('Unexpected "!" at position ' + i);
      }
      if (ch === '<') {
        if (str[i + 1] === '=') { tokens.push({ type: 'le' }); i += 2; continue; }
        tokens.push({ type: 'lt' }); i++; continue;
      }
      if (ch === '>') {
        if (str[i + 1] === '=') { tokens.push({ type: 'ge' }); i += 2; continue; }
        tokens.push({ type: 'gt' }); i++; continue;
      }
      if (ch === ':') {
        if (str[i + 1] === ':') { tokens.push({ type: 'coloncolon' }); i += 2; continue; }
        tokens.push({ type: 'colon' }); i++; continue;
      }
      if (ch === '/') {
        if (str[i + 1] === '/') { tokens.push({ type: 'slashslash' }); i += 2; continue; }
        tokens.push({ type: 'slash' }); i++; continue;
      }
      if (ch === '.') {
        if (str[i + 1] === '.') { tokens.push({ type: 'dotdot' }); i += 2; continue; }
        if (str[i + 1] >= '0' && str[i + 1] <= '9') {
          var j0 = i + 1;
          while (j0 < n && str[j0] >= '0' && str[j0] <= '9') j0++;
          tokens.push({ type: 'number', value: parseFloat(str.slice(i, j0)) });
          i = j0; continue;
        }
        tokens.push({ type: 'dot' }); i++; continue;
      }
      if (ch >= '0' && ch <= '9') {
        var j1 = i;
        while (j1 < n && str[j1] >= '0' && str[j1] <= '9') j1++;
        if (str[j1] === '.') {
          j1++;
          while (j1 < n && str[j1] >= '0' && str[j1] <= '9') j1++;
        }
        tokens.push({ type: 'number', value: parseFloat(str.slice(i, j1)) });
        i = j1; continue;
      }
      if (ch === '"' || ch === "'") {
        var quote = ch;
        var j2 = i + 1;
        while (j2 < n && str[j2] !== quote) j2++;
        if (j2 >= n) throw syntaxError('Unterminated string literal');
        tokens.push({ type: 'string', value: str.slice(i + 1, j2) });
        i = j2 + 1; continue;
      }
      if (NAME_START.test(ch)) {
        var j3 = i + 1;
        while (j3 < n && NAME_PART.test(str[j3])) j3++;
        tokens.push({ type: 'name', value: str.slice(i, j3) });
        i = j3; continue;
      }
      throw syntaxError('Unexpected character "' + ch + '" at position ' + i);
    }
    return tokens;
  }

  var NODETYPE_NAMES = { 'comment': 1, 'text': 1, 'processing-instruction': 1, 'node': 1 };
  var AXES_LIST = {
    'ancestor': 1, 'ancestor-or-self': 1, 'attribute': 1, 'child': 1, 'descendant': 1,
    'descendant-or-self': 1, 'following': 1, 'following-sibling': 1, 'namespace': 1,
    'parent': 1, 'preceding': 1, 'preceding-sibling': 1, 'self': 1,
  };

  // ── Parser (recursive descent; grammar positions resolve the '*'/keyword
  // ambiguities the XPath 1.0 spec otherwise handles with lexer state) ───────
  function parseXPath(source) {
    var tokens = tokenize(source);
    var pos = 0;
    function peek(o) { return tokens[pos + (o || 0)]; }
    function advance() { return tokens[pos++]; }
    function expect(type) {
      var t = advance();
      if (!t || t.type !== type) throw syntaxError('Expected ' + type + ' but got ' + (t ? t.type : 'end of expression'));
      return t;
    }

    function canStartStep(t) {
      return !!t && (t.type === 'name' || t.type === 'star' || t.type === 'dot' || t.type === 'dotdot' || t.type === 'at');
    }

    function parseNodeTest() {
      var t = peek();
      if (t && t.type === 'star') { advance(); return { kind: 'name', prefix: null, local: '*' }; }
      if (t && t.type === 'name') {
        var name = t.value;
        if (peek(1) && peek(1).type === 'lparen' && NODETYPE_NAMES[name]) {
          advance(); advance();
          if (name === 'processing-instruction') {
            var target = null;
            if (peek() && peek().type === 'string') target = advance().value;
            expect('rparen');
            return { kind: 'pi', target: target };
          }
          expect('rparen');
          return { kind: name };
        }
        advance();
        if (peek() && peek().type === 'colon') {
          advance();
          if (peek() && peek().type === 'star') { advance(); return { kind: 'name', prefix: name, local: '*' }; }
          return { kind: 'name', prefix: name, local: expect('name').value };
        }
        return { kind: 'name', prefix: null, local: name };
      }
      throw syntaxError('Expected a node test');
    }

    function parseStep() {
      if (peek() && peek().type === 'dotdot') { advance(); return { axis: 'parent', test: { kind: 'node' }, predicates: [] }; }
      if (peek() && peek().type === 'dot') { advance(); return { axis: 'self', test: { kind: 'node' }, predicates: [] }; }
      var axis = 'child';
      if (peek() && peek().type === 'at') {
        advance(); axis = 'attribute';
      } else if (peek() && peek().type === 'name' && peek(1) && peek(1).type === 'coloncolon') {
        axis = advance().value;
        advance();
        if (!AXES_LIST[axis]) throw syntaxError('Unknown axis "' + axis + '"');
      }
      var test = parseNodeTest();
      var predicates = [];
      while (peek() && peek().type === 'lbracket') predicates.push(parsePredicate());
      return { axis: axis, test: test, predicates: predicates };
    }

    var DESCENDANT_OR_SELF_STEP = { axis: 'descendant-or-self', test: { kind: 'node' }, predicates: [] };

    function parseRelativeSteps(leadingDoubleSlash) {
      var steps = [];
      if (leadingDoubleSlash) steps.push(DESCENDANT_OR_SELF_STEP);
      steps.push(parseStep());
      while (peek() && (peek().type === 'slash' || peek().type === 'slashslash')) {
        if (advance().type === 'slashslash') steps.push(DESCENDANT_OR_SELF_STEP);
        steps.push(parseStep());
      }
      return steps;
    }

    function parseLocationPath() {
      if (peek() && peek().type === 'slashslash') {
        advance();
        return { type: 'Path', absolute: true, steps: parseRelativeSteps(true) };
      }
      if (peek() && peek().type === 'slash') {
        advance();
        if (!canStartStep(peek())) return { type: 'Path', absolute: true, steps: [] };
        return { type: 'Path', absolute: true, steps: parseRelativeSteps(false) };
      }
      return { type: 'Path', absolute: false, steps: parseRelativeSteps(false) };
    }

    function parsePredicate() {
      expect('lbracket');
      var e = parseExpr();
      expect('rbracket');
      return e;
    }

    function parsePrimaryExpr() {
      var t = peek();
      if (!t) throw syntaxError('Unexpected end of expression');
      if (t.type === 'dollar') {
        advance();
        var vname = expect('name').value;
        if (peek() && peek().type === 'colon') { advance(); vname += ':' + expect('name').value; }
        return { type: 'Variable', name: vname };
      }
      if (t.type === 'lparen') { advance(); var e = parseExpr(); expect('rparen'); return e; }
      if (t.type === 'string') { advance(); return { type: 'Literal', value: t.value }; }
      if (t.type === 'number') { advance(); return { type: 'Number', value: t.value }; }
      if (t.type === 'name') {
        var fname = advance().value;
        expect('lparen');
        var args = [];
        if (!(peek() && peek().type === 'rparen')) {
          args.push(parseExpr());
          while (peek() && peek().type === 'comma') { advance(); args.push(parseExpr()); }
        }
        expect('rparen');
        return { type: 'FunctionCall', name: fname, args: args };
      }
      throw syntaxError('Unexpected token in expression');
    }

    function parseFilterOrPath() {
      var expr = parsePrimaryExpr();
      var predicates = [];
      while (peek() && peek().type === 'lbracket') predicates.push(parsePredicate());
      if (predicates.length) expr = { type: 'Filter', expr: expr, predicates: predicates };
      if (peek() && (peek().type === 'slash' || peek().type === 'slashslash')) {
        var wasDouble = advance().type === 'slashslash';
        expr = { type: 'PathFromFilter', filter: expr, steps: parseRelativeSteps(wasDouble) };
      }
      return expr;
    }

    function parsePathExpr() {
      var t = peek();
      if (t && t.type === 'name' && peek(1) && peek(1).type === 'lparen' && !NODETYPE_NAMES[t.value]) {
        return parseFilterOrPath();
      }
      if (t && (t.type === 'slash' || t.type === 'slashslash' || t.type === 'dot' ||
                t.type === 'dotdot' || t.type === 'at' || t.type === 'star' || t.type === 'name')) {
        return parseLocationPath();
      }
      return parseFilterOrPath();
    }

    function parseUnionExpr() {
      var left = parsePathExpr();
      while (peek() && peek().type === 'pipe') {
        advance();
        left = { type: 'Union', left: left, right: parsePathExpr() };
      }
      return left;
    }

    function parseUnaryExpr() {
      if (peek() && peek().type === 'minus') { advance(); return { type: 'Unary', expr: parseUnaryExpr() }; }
      return parseUnionExpr();
    }

    function parseMultiplicativeExpr() {
      var left = parseUnaryExpr();
      while (peek() && (peek().type === 'star' || (peek().type === 'name' && (peek().value === 'div' || peek().value === 'mod')))) {
        var t = advance();
        var op = t.type === 'star' ? '*' : t.value;
        left = { type: 'Binary', op: op, left: left, right: parseUnaryExpr() };
      }
      return left;
    }

    function parseAdditiveExpr() {
      var left = parseMultiplicativeExpr();
      while (peek() && (peek().type === 'plus' || peek().type === 'minus')) {
        var op = advance().type === 'plus' ? '+' : '-';
        left = { type: 'Binary', op: op, left: left, right: parseMultiplicativeExpr() };
      }
      return left;
    }

    function parseRelationalExpr() {
      var left = parseAdditiveExpr();
      while (peek() && (peek().type === 'lt' || peek().type === 'gt' || peek().type === 'le' || peek().type === 'ge')) {
        var map = { lt: '<', gt: '>', le: '<=', ge: '>=' };
        var op = map[advance().type];
        left = { type: 'Binary', op: op, left: left, right: parseAdditiveExpr() };
      }
      return left;
    }

    function parseEqualityExpr() {
      var left = parseRelationalExpr();
      while (peek() && (peek().type === 'eq' || peek().type === 'ne')) {
        var op = advance().type === 'eq' ? '=' : '!=';
        left = { type: 'Binary', op: op, left: left, right: parseRelationalExpr() };
      }
      return left;
    }

    function parseAndExpr() {
      var left = parseEqualityExpr();
      while (peek() && peek().type === 'name' && peek().value === 'and') {
        advance();
        left = { type: 'Binary', op: 'and', left: left, right: parseEqualityExpr() };
      }
      return left;
    }

    function parseOrExpr() {
      var left = parseAndExpr();
      while (peek() && peek().type === 'name' && peek().value === 'or') {
        advance();
        left = { type: 'Binary', op: 'or', left: left, right: parseAndExpr() };
      }
      return left;
    }

    function parseExpr() { return parseOrExpr(); }

    var ast = parseExpr();
    if (pos !== tokens.length) throw syntaxError('Unexpected trailing input in expression');
    return ast;
  }

  // ── DOM tree helpers (generic — works on any standard-shaped DOM node) ────
  function sameNode(a, b) {
    if (a === b) return true;
    if (!a || !b) return false;
    if (a.nodeType === 2 && b.nodeType === 2) return a.name === b.name && sameNode(a.ownerElement, b.ownerElement);
    if (typeof a.isSameNode === 'function') return a.isSameNode(b);
    return false;
  }

  function docPositionCompare(a, b) {
    if (sameNode(a, b)) return 0;
    var ea = a.nodeType === 2 ? a.ownerElement : a;
    var eb = b.nodeType === 2 ? b.ownerElement : b;
    if (sameNode(ea, eb)) return a.nodeType === 2 && b.nodeType === 2 ? 0 : (a.nodeType === 2 ? -1 : 1);
    if (typeof ea.compareDocumentPosition !== 'function') return 0;
    var pos = ea.compareDocumentPosition(eb);
    if (pos & 4) return -1; // Node.DOCUMENT_POSITION_FOLLOWING: a before b
    if (pos & 2) return 1;  // Node.DOCUMENT_POSITION_PRECEDING: a after b
    return 0;
  }

  function dedupeAndSort(nodes) {
    var out = [];
    for (var i = 0; i < nodes.length; i++) {
      var dup = false;
      for (var j = 0; j < out.length; j++) { if (sameNode(nodes[i], out[j])) { dup = true; break; } }
      if (!dup) out.push(nodes[i]);
    }
    out.sort(docPositionCompare);
    return out;
  }

  function documentRoot(node) {
    var n = node && node.nodeType === 2 ? node.ownerElement : node;
    var guard = 0;
    while (n && n.parentNode && guard++ < 1000000) n = n.parentNode;
    return n;
  }

  function preorder(root) {
    var out = [];
    (function walk(n) {
      out.push(n);
      var kids = n.childNodes || [];
      for (var i = 0; i < kids.length; i++) walk(kids[i]);
    })(root);
    return out;
  }

  function childrenOf(node) {
    if (!node || node.nodeType === 2) return [];
    var kids = node.childNodes;
    return kids ? Array.prototype.slice.call(kids) : [];
  }

  var AXES = {
    self: function(n) { return [n]; },
    child: function(n) { return childrenOf(n); },
    descendant: function(n) {
      var out = [];
      childrenOf(n).forEach(function(c) { out.push(c); out = out.concat(AXES.descendant(c)); });
      return out;
    },
    'descendant-or-self': function(n) { return [n].concat(AXES.descendant(n)); },
    parent: function(n) {
      if (n.nodeType === 2) return n.ownerElement ? [n.ownerElement] : [];
      return n.parentNode ? [n.parentNode] : [];
    },
    ancestor: function(n) {
      var out = [];
      var p = AXES.parent(n)[0];
      while (p) { out.push(p); p = p.parentNode || null; }
      return out;
    },
    'ancestor-or-self': function(n) { return [n].concat(AXES.ancestor(n)); },
    'following-sibling': function(n) {
      if (n.nodeType === 2 || !n.parentNode) return [];
      var sibs = childrenOf(n.parentNode);
      var idx = sibs.findIndex(function(s) { return sameNode(s, n); });
      return idx < 0 ? [] : sibs.slice(idx + 1);
    },
    'preceding-sibling': function(n) {
      if (n.nodeType === 2 || !n.parentNode) return [];
      var sibs = childrenOf(n.parentNode);
      var idx = sibs.findIndex(function(s) { return sameNode(s, n); });
      return idx <= 0 ? [] : sibs.slice(0, idx).reverse();
    },
    following: function(n) {
      var root = documentRoot(n);
      if (!root) return [];
      var all = preorder(root);
      var idx = all.findIndex(function(x) { return sameNode(x, n); });
      if (idx < 0) return n.nodeType === 2 ? AXES.following(n.ownerElement) : [];
      var subtreeSize = preorder(n).length;
      return all.slice(idx + subtreeSize);
    },
    preceding: function(n) {
      var root = documentRoot(n);
      if (!root) return [];
      var all = preorder(root);
      var idx = all.findIndex(function(x) { return sameNode(x, n); });
      if (idx < 0) return [];
      var ancestors = AXES.ancestor(n);
      var out = [];
      for (var i = idx - 1; i >= 0; i--) {
        var isAncestor = ancestors.some(function(a) { return sameNode(a, all[i]); });
        if (!isAncestor) out.push(all[i]);
      }
      return out;
    },
    attribute: function(n) {
      if (!n || n.nodeType !== 1 || !n.attributes) return [];
      var attrs = n.attributes;
      var out = [];
      for (var i = 0; i < attrs.length; i++) out.push(attrs.item ? attrs.item(i) : attrs[i]);
      return out;
    },
    // Namespace nodes are not modelled by this DOM (no XPath 1.0 consumer in
    // the corpus needs them) — explicit scope cut, not an oversight.
    namespace: function() { return []; },
  };

  function resolvePrefix(prefix, resolver) {
    if (prefix === 'xml') return 'http://www.w3.org/XML/1998/namespace';
    if (!resolver) throw new XPathException(XPathException.TYPE_ERR, 'Unresolvable namespace prefix: ' + prefix);
    var uri = typeof resolver === 'function' ? resolver(prefix)
      : (typeof resolver.lookupNamespaceURI === 'function' ? resolver.lookupNamespaceURI(prefix) : null);
    if (!uri) throw new XPathException(XPathException.TYPE_ERR, 'Unresolvable namespace prefix: ' + prefix);
    return uri;
  }

  function nodeTestMatches(node, test, axis, resolver) {
    if (test.kind === 'node') return true;
    if (test.kind === 'text') return node.nodeType === 3;
    if (test.kind === 'comment') return node.nodeType === 8;
    if (test.kind === 'pi') return node.nodeType === 7 && (!test.target || node.nodeName === test.target || node.target === test.target);
    // NameTest — principal node type of the axis (attribute::/namespace:: -> attribute nodes, else element)
    var principal = axis === 'attribute' ? 2 : 1;
    if (node.nodeType !== principal) return false;
    if (test.local === '*') {
      if (!test.prefix) return true;
      return (node.namespaceURI || null) === (resolvePrefix(test.prefix, resolver) || null);
    }
    if (test.prefix) {
      return node.localName === test.local && (node.namespaceURI || null) === (resolvePrefix(test.prefix, resolver) || null);
    }
    // Unprefixed name test is spec'd to match only the null namespace (XPath
    // 1.0 §2.3), but real elements of an HTML document carry the implicit
    // XHTML namespace (`http://www.w3.org/1999/xhtml`) on `namespaceURI`, not
    // null — verified live via `tests/wpt/verify_gap_xpath.py`. Every other
    // XPath 1.0 engine shipped in a browser special-cases this (same
    // compatibility clause that lets `//p` find HTML paragraphs at all), so
    // an unprefixed test accepts both the null and the implicit-XHTML
    // namespace rather than only null.
    var ns = node.namespaceURI || null;
    return (node.localName || node.nodeName) === test.local &&
      (ns === null || ns === 'http://www.w3.org/1999/xhtml');
  }

  function applyStep(contextNodes, step, resolver) {
    var flat = [];
    contextNodes.forEach(function(cn) {
      var candidates = AXES[step.axis](cn).filter(function(n) { return nodeTestMatches(n, step.test, step.axis, resolver); });
      step.predicates.forEach(function(predAst) {
        var size = candidates.length;
        candidates = candidates.filter(function(n, idx) {
          var v = evalExpr(predAst, n, idx + 1, size, resolver);
          return v.type === 'number' ? v.value === (idx + 1) : toBoolean(v);
        });
      });
      flat = flat.concat(candidates);
    });
    return dedupeAndSort(flat);
  }

  function evalPath(ast, contextNode, resolver) {
    var nodes = ast.absolute ? [documentRoot(contextNode)] : [contextNode];
    ast.steps.forEach(function(step) { nodes = applyStep(nodes, step, resolver); });
    return nodes;
  }

  // ── Value model: {type:'nodeset'|'number'|'string'|'boolean', ...} ────────
  function stringValueOf(node) {
    if (node.nodeType === 1 || node.nodeType === 9 || node.nodeType === 11) {
      var s = '';
      (function walk(n) {
        childrenOf(n).forEach(function(k) {
          if (k.nodeType === 3) s += k.data !== undefined ? k.data : (k.textContent || '');
          else if (k.nodeType === 1) walk(k);
        });
      })(node);
      return s;
    }
    if (node.nodeType === 2) return node.value;
    return node.data !== undefined ? node.data : (node.textContent || node.nodeValue || '');
  }

  function numFromXPathString(s) {
    var t = String(s).trim();
    if (t === '') return NaN;
    var n = Number(t);
    return n;
  }

  function numberToXPathString(n) {
    if (isNaN(n)) return 'NaN';
    if (n === Infinity) return 'Infinity';
    if (n === -Infinity) return '-Infinity';
    if (Object.is(n, -0)) return '0';
    return String(n);
  }

  function toBoolean(v) {
    switch (v.type) {
      case 'boolean': return v.value;
      case 'number': return v.value !== 0 && !isNaN(v.value);
      case 'string': return v.value.length > 0;
      case 'nodeset': return v.nodes.length > 0;
    }
  }

  function toNumber(v) {
    switch (v.type) {
      case 'number': return v.value;
      case 'boolean': return v.value ? 1 : 0;
      case 'string': return numFromXPathString(v.value);
      case 'nodeset': return numFromXPathString(toStringValue(v));
    }
  }

  function toStringValue(v) {
    switch (v.type) {
      case 'string': return v.value;
      case 'number': return numberToXPathString(v.value);
      case 'boolean': return v.value ? 'true' : 'false';
      case 'nodeset': return v.nodes.length ? stringValueOf(v.nodes[0]) : '';
    }
  }

  function applyOp(op, a, b) {
    switch (op) {
      case '=': return a === b;
      case '!=': return a !== b;
      case '<': return a < b;
      case '>': return a > b;
      case '<=': return a <= b;
      case '>=': return a >= b;
    }
  }

  // XPath 1.0 §3.4 equality/relational comparison rules.
  function xpathRelOp(l, r, op) {
    if (l.type === 'nodeset' && r.type === 'nodeset') {
      for (var i = 0; i < l.nodes.length; i++) {
        for (var j = 0; j < r.nodes.length; j++) {
          var sa = stringValueOf(l.nodes[i]), sb = stringValueOf(r.nodes[j]);
          var a = (op === '=' || op === '!=') ? sa : numFromXPathString(sa);
          var b = (op === '=' || op === '!=') ? sb : numFromXPathString(sb);
          if (applyOp(op, a, b)) return true;
        }
      }
      return false;
    }
    if (l.type === 'nodeset' || r.type === 'nodeset') {
      var ns = l.type === 'nodeset' ? l : r;
      var other = l.type === 'nodeset' ? r : l;
      var nsIsLeft = l.type === 'nodeset';
      if (other.type === 'boolean') return applyOp(op, nsIsLeft ? toBoolean(ns) : other.value, nsIsLeft ? other.value : toBoolean(ns));
      for (var k = 0; k < ns.nodes.length; k++) {
        var sv = stringValueOf(ns.nodes[k]);
        var nodeVal = other.type === 'number' ? numFromXPathString(sv) : sv;
        var res = nsIsLeft ? applyOp(op, nodeVal, other.value) : applyOp(op, other.value, nodeVal);
        if (res) return true;
      }
      return false;
    }
    if (op === '=' || op === '!=') {
      if (l.type === 'boolean' || r.type === 'boolean') return applyOp(op, toBoolean(l), toBoolean(r));
      if (l.type === 'number' || r.type === 'number') return applyOp(op, toNumber(l), toNumber(r));
      return applyOp(op, toStringValue(l), toStringValue(r));
    }
    return applyOp(op, toNumber(l), toNumber(r));
  }

  function xpathSubstring(s, start, len) {
    if (isNaN(start) || isNaN(len)) return '';
    var strLen = s.length;
    var first = Math.round(start);
    var last = len === Infinity ? strLen + 1 : Math.round(start + len);
    first = Math.max(first, 1);
    last = Math.min(last, strLen + 1);
    return last <= first ? '' : s.slice(first - 1, last - 1);
  }

  function firstNode(v) { return v.type === 'nodeset' && v.nodes.length ? v.nodes[0] : null; }
  function checkArgc(name, args, n) { if (args.length !== n) throw typeErr(name + '() expects ' + n + ' argument(s)'); }

  var XPATH_FUNCTIONS = {
    'last': function(a, cn, cp, cs) { checkArgc('last', a, 0); return { type: 'number', value: cs }; },
    'position': function(a, cn, cp, cs) { checkArgc('position', a, 0); return { type: 'number', value: cp }; },
    'count': function(a) { checkArgc('count', a, 1); if (a[0].type !== 'nodeset') throw typeErr('count() requires a node-set'); return { type: 'number', value: a[0].nodes.length }; },
    'id': function(a, cn) {
      checkArgc('id', a, 1);
      var ids = a[0].type === 'nodeset' ? a[0].nodes.map(stringValueOf).join(' ') : toStringValue(a[0]);
      var root = documentRoot(cn);
      var doc = root && root.nodeType === 9 ? root : (root && root.ownerDocument) || (typeof document !== 'undefined' ? document : null);
      var found = [];
      ids.split(/\s+/).filter(Boolean).forEach(function(id) {
        var el = doc && typeof doc.getElementById === 'function' ? doc.getElementById(id) : null;
        if (el) found.push(el);
      });
      return { type: 'nodeset', nodes: dedupeAndSort(found) };
    },
    'local-name': function(a, cn) { var n = a.length ? firstNode(a[0]) : cn; return { type: 'string', value: n ? (n.localName || n.nodeName || '') : '' }; },
    'namespace-uri': function(a, cn) { var n = a.length ? firstNode(a[0]) : cn; return { type: 'string', value: n ? (n.namespaceURI || '') : '' }; },
    'name': function(a, cn) { var n = a.length ? firstNode(a[0]) : cn; return { type: 'string', value: n ? (n.nodeName !== undefined ? n.nodeName : (n.name || '')) : '' }; },
    'string': function(a, cn) { return { type: 'string', value: a.length ? toStringValue(a[0]) : stringValueOf(cn) }; },
    'concat': function(a) { if (a.length < 2) throw typeErr('concat() requires at least 2 arguments'); return { type: 'string', value: a.map(toStringValue).join('') }; },
    'starts-with': function(a) { checkArgc('starts-with', a, 2); return { type: 'boolean', value: toStringValue(a[0]).indexOf(toStringValue(a[1])) === 0 }; },
    'contains': function(a) { checkArgc('contains', a, 2); return { type: 'boolean', value: toStringValue(a[0]).indexOf(toStringValue(a[1])) !== -1 }; },
    'substring-before': function(a) {
      checkArgc('substring-before', a, 2);
      var s = toStringValue(a[0]), t = toStringValue(a[1]), idx = s.indexOf(t);
      return { type: 'string', value: idx === -1 ? '' : s.slice(0, idx) };
    },
    'substring-after': function(a) {
      checkArgc('substring-after', a, 2);
      var s = toStringValue(a[0]), t = toStringValue(a[1]), idx = s.indexOf(t);
      return { type: 'string', value: idx === -1 ? '' : s.slice(idx + t.length) };
    },
    'substring': function(a) {
      if (a.length < 2 || a.length > 3) throw typeErr('substring() requires 2 or 3 arguments');
      return { type: 'string', value: xpathSubstring(toStringValue(a[0]), toNumber(a[1]), a.length === 3 ? toNumber(a[2]) : Infinity) };
    },
    'string-length': function(a, cn) { return { type: 'number', value: (a.length ? toStringValue(a[0]) : stringValueOf(cn)).length }; },
    'normalize-space': function(a, cn) { return { type: 'string', value: (a.length ? toStringValue(a[0]) : stringValueOf(cn)).replace(/[ \t\n\r]+/g, ' ').trim() }; },
    'translate': function(a) {
      checkArgc('translate', a, 3);
      var s = toStringValue(a[0]), from = toStringValue(a[1]), to = toStringValue(a[2]), out = '';
      for (var i = 0; i < s.length; i++) {
        var idx = from.indexOf(s[i]);
        if (idx === -1) out += s[i]; else if (idx < to.length) out += to[idx];
      }
      return { type: 'string', value: out };
    },
    'boolean': function(a) { checkArgc('boolean', a, 1); return { type: 'boolean', value: toBoolean(a[0]) }; },
    'not': function(a) { checkArgc('not', a, 1); return { type: 'boolean', value: !toBoolean(a[0]) }; },
    'true': function(a) { checkArgc('true', a, 0); return { type: 'boolean', value: true }; },
    'false': function(a) { checkArgc('false', a, 0); return { type: 'boolean', value: false }; },
    'lang': function(a, cn) {
      checkArgc('lang', a, 1);
      var want = toStringValue(a[0]).toLowerCase();
      var n = cn;
      while (n) {
        var v = typeof n.getAttribute === 'function' ? (n.getAttribute('xml:lang') || n.getAttribute('lang')) : null;
        if (v) { v = v.toLowerCase(); return { type: 'boolean', value: v === want || v.indexOf(want + '-') === 0 }; }
        n = n.parentNode;
      }
      return { type: 'boolean', value: false };
    },
    'number': function(a, cn) { return { type: 'number', value: a.length ? toNumber(a[0]) : numFromXPathString(stringValueOf(cn)) }; },
    'sum': function(a) {
      checkArgc('sum', a, 1);
      if (a[0].type !== 'nodeset') throw typeErr('sum() requires a node-set');
      var total = 0;
      a[0].nodes.forEach(function(n) { total += numFromXPathString(stringValueOf(n)); });
      return { type: 'number', value: total };
    },
    'floor': function(a) { checkArgc('floor', a, 1); return { type: 'number', value: Math.floor(toNumber(a[0])) }; },
    'ceiling': function(a) { checkArgc('ceiling', a, 1); return { type: 'number', value: Math.ceil(toNumber(a[0])) }; },
    'round': function(a) { checkArgc('round', a, 1); var n = toNumber(a[0]); return { type: 'number', value: isNaN(n) ? NaN : Math.round(n) }; },
  };

  function evalExpr(ast, ctxNode, ctxPos, ctxSize, resolver) {
    switch (ast.type) {
      case 'Path': return { type: 'nodeset', nodes: evalPath(ast, ctxNode, resolver) };
      case 'PathFromFilter': {
        var base = evalExpr(ast.filter, ctxNode, ctxPos, ctxSize, resolver);
        if (base.type !== 'nodeset') throw typeErr('"/" requires a node-set operand');
        var nodes = base.nodes;
        ast.steps.forEach(function(step) { nodes = applyStep(nodes, step, resolver); });
        return { type: 'nodeset', nodes: nodes };
      }
      case 'Filter': {
        var inner = evalExpr(ast.expr, ctxNode, ctxPos, ctxSize, resolver);
        if (inner.type !== 'nodeset') throw typeErr('predicate applied to a non-node-set expression');
        var filtered = inner.nodes;
        ast.predicates.forEach(function(predAst) {
          var size = filtered.length;
          filtered = filtered.filter(function(n, idx) {
            var v = evalExpr(predAst, n, idx + 1, size, resolver);
            return v.type === 'number' ? v.value === (idx + 1) : toBoolean(v);
          });
        });
        return { type: 'nodeset', nodes: filtered };
      }
      case 'Union': {
        var l = evalExpr(ast.left, ctxNode, ctxPos, ctxSize, resolver);
        var r = evalExpr(ast.right, ctxNode, ctxPos, ctxSize, resolver);
        if (l.type !== 'nodeset' || r.type !== 'nodeset') throw typeErr('"|" requires node-set operands');
        return { type: 'nodeset', nodes: dedupeAndSort(l.nodes.concat(r.nodes)) };
      }
      case 'Unary': return { type: 'number', value: -toNumber(evalExpr(ast.expr, ctxNode, ctxPos, ctxSize, resolver)) };
      case 'Binary': {
        var op = ast.op;
        if (op === 'and') {
          var lv = evalExpr(ast.left, ctxNode, ctxPos, ctxSize, resolver);
          if (!toBoolean(lv)) return { type: 'boolean', value: false };
          return { type: 'boolean', value: toBoolean(evalExpr(ast.right, ctxNode, ctxPos, ctxSize, resolver)) };
        }
        if (op === 'or') {
          var lv2 = evalExpr(ast.left, ctxNode, ctxPos, ctxSize, resolver);
          if (toBoolean(lv2)) return { type: 'boolean', value: true };
          return { type: 'boolean', value: toBoolean(evalExpr(ast.right, ctxNode, ctxPos, ctxSize, resolver)) };
        }
        var lval = evalExpr(ast.left, ctxNode, ctxPos, ctxSize, resolver);
        var rval = evalExpr(ast.right, ctxNode, ctxPos, ctxSize, resolver);
        if (op === '=' || op === '!=' || op === '<' || op === '>' || op === '<=' || op === '>=') {
          return { type: 'boolean', value: xpathRelOp(lval, rval, op) };
        }
        var ln = toNumber(lval), rn = toNumber(rval);
        switch (op) {
          case '+': return { type: 'number', value: ln + rn };
          case '-': return { type: 'number', value: ln - rn };
          case '*': return { type: 'number', value: ln * rn };
          case 'div': return { type: 'number', value: ln / rn };
          case 'mod': return { type: 'number', value: ln % rn };
        }
        throw typeErr('Unknown operator "' + op + '"');
      }
      case 'Literal': return { type: 'string', value: ast.value };
      case 'Number': return { type: 'number', value: ast.value };
      case 'Variable': throw typeErr('Unresolvable variable reference: $' + ast.name);
      case 'FunctionCall': {
        var fn = XPATH_FUNCTIONS[ast.name];
        if (!fn) throw typeErr('Unknown XPath function: ' + ast.name);
        var args = ast.args.map(function(a) { return evalExpr(a, ctxNode, ctxPos, ctxSize, resolver); });
        return fn(args, ctxNode, ctxPos, ctxSize, resolver);
      }
      default: throw typeErr('Unknown expression node "' + ast.type + '"');
    }
  }

  // ── XPathResult (DOM4 §XPathResult) ───────────────────────────────────────
  function XPathResult() { throw new TypeError('Illegal constructor'); }
  var RESULT_TYPES = {
    ANY_TYPE: 0, NUMBER_TYPE: 1, STRING_TYPE: 2, BOOLEAN_TYPE: 3,
    UNORDERED_NODE_ITERATOR_TYPE: 4, ORDERED_NODE_ITERATOR_TYPE: 5,
    UNORDERED_NODE_SNAPSHOT_TYPE: 6, ORDERED_NODE_SNAPSHOT_TYPE: 7,
    ANY_UNORDERED_NODE_TYPE: 8, FIRST_ORDERED_NODE_TYPE: 9,
  };
  Object.keys(RESULT_TYPES).forEach(function(k) {
    Object.defineProperty(XPathResult, k, { value: RESULT_TYPES[k], enumerable: true });
    Object.defineProperty(XPathResult.prototype, k, { value: RESULT_TYPES[k], enumerable: true });
  });
  XPathResult.prototype.iterateNext = function() {
    if (this.resultType !== RESULT_TYPES.UNORDERED_NODE_ITERATOR_TYPE && this.resultType !== RESULT_TYPES.ORDERED_NODE_ITERATOR_TYPE) {
      throw typeErr('iterateNext() is only valid on an iterator result');
    }
    var st = this.__iterState__;
    return st.i < this.__nodes__.length ? this.__nodes__[st.i++] : null;
  };
  XPathResult.prototype.snapshotItem = function(index) {
    if (this.resultType !== RESULT_TYPES.UNORDERED_NODE_SNAPSHOT_TYPE && this.resultType !== RESULT_TYPES.ORDERED_NODE_SNAPSHOT_TYPE) {
      throw typeErr('snapshotItem() is only valid on a snapshot result');
    }
    var i = index >>> 0;
    return i < this.__nodes__.length ? this.__nodes__[i] : null;
  };
  window.XPathResult = XPathResult;

  function makeXPathResult(value, requestedType) {
    var rt = requestedType || RESULT_TYPES.ANY_TYPE;
    if (rt === RESULT_TYPES.ANY_TYPE) {
      rt = value.type === 'nodeset' ? RESULT_TYPES.UNORDERED_NODE_ITERATOR_TYPE
        : value.type === 'number' ? RESULT_TYPES.NUMBER_TYPE
        : value.type === 'string' ? RESULT_TYPES.STRING_TYPE
        : RESULT_TYPES.BOOLEAN_TYPE;
    }
    var result = Object.create(XPathResult.prototype);
    Object.defineProperty(result, 'resultType', { value: rt, enumerable: true });
    if (rt === RESULT_TYPES.NUMBER_TYPE) {
      Object.defineProperty(result, 'numberValue', { value: toNumber(value), enumerable: true });
    } else if (rt === RESULT_TYPES.STRING_TYPE) {
      Object.defineProperty(result, 'stringValue', { value: toStringValue(value), enumerable: true });
    } else if (rt === RESULT_TYPES.BOOLEAN_TYPE) {
      Object.defineProperty(result, 'booleanValue', { value: toBoolean(value), enumerable: true });
    } else if (rt === RESULT_TYPES.UNORDERED_NODE_ITERATOR_TYPE || rt === RESULT_TYPES.ORDERED_NODE_ITERATOR_TYPE ||
               rt === RESULT_TYPES.UNORDERED_NODE_SNAPSHOT_TYPE || rt === RESULT_TYPES.ORDERED_NODE_SNAPSHOT_TYPE) {
      if (value.type !== 'nodeset') throw typeErr('Requested a node-set result type for a non-node-set expression');
      Object.defineProperty(result, '__nodes__', { value: value.nodes });
      Object.defineProperty(result, '__iterState__', { value: { i: 0 } });
      if (rt === RESULT_TYPES.UNORDERED_NODE_SNAPSHOT_TYPE || rt === RESULT_TYPES.ORDERED_NODE_SNAPSHOT_TYPE) {
        Object.defineProperty(result, 'snapshotLength', { value: value.nodes.length, enumerable: true });
      }
    } else if (rt === RESULT_TYPES.ANY_UNORDERED_NODE_TYPE || rt === RESULT_TYPES.FIRST_ORDERED_NODE_TYPE) {
      if (value.type !== 'nodeset') throw typeErr('Requested a node-set result type for a non-node-set expression');
      Object.defineProperty(result, 'singleNodeValue', { value: value.nodes.length ? value.nodes[0] : null, enumerable: true });
    } else {
      throw typeErr('Unknown XPathResult type ' + rt);
    }
    return result;
  }

  // ── XPathExpression / XPathEvaluator (DOM4 §XPathEvaluatorBase) ───────────
  function XPathExpression(ast, resolver) {
    this.__ast__ = ast;
    this.__resolver__ = resolver || null;
  }
  XPathExpression.prototype.evaluate = function(contextNode, type, result) {
    if (!contextNode) throw new TypeError('XPathExpression.evaluate: contextNode is required');
    return makeXPathResult(evalExpr(this.__ast__, contextNode, 1, 1, this.__resolver__), type || 0);
  };
  window.XPathExpression = XPathExpression;

  function xpathCreateExpression(expression, resolver) {
    var ast;
    try { ast = parseXPath(String(expression)); }
    catch (e) {
      if (e instanceof XPathException) throw e;
      throw syntaxError(e && e.message ? e.message : String(e));
    }
    return new XPathExpression(ast, resolver);
  }
  function xpathCreateNSResolver(nodeResolver) {
    // Any Node already implements `lookupNamespaceURI` (GAP-XMLDOC), which is
    // exactly the XPathNSResolver contract, so it doubles as the resolver.
    return nodeResolver;
  }
  function xpathEvaluate(expression, contextNode, resolver, type, result) {
    return xpathCreateExpression(expression, resolver).evaluate(contextNode, type, result);
  }

  function XPathEvaluator() {}
  XPathEvaluator.prototype.createExpression = function(expression, resolver) { return xpathCreateExpression(expression, resolver); };
  XPathEvaluator.prototype.createNSResolver = function(nodeResolver) { return xpathCreateNSResolver(nodeResolver); };
  XPathEvaluator.prototype.evaluate = function(expression, contextNode, resolver, type, result) { return xpathEvaluate(expression, contextNode, resolver, type, result); };
  window.XPathEvaluator = XPathEvaluator;

  // `Document` implements the `XPathEvaluatorBase` mixin directly (DOM4) —
  // covers the live page document and `document.implementation.createDocument`/
  // `createHTMLDocument` output, since both share `Document.prototype`
  // (see `web_api_shim_mid.js`'s `Object.setPrototypeOf(document, Document.prototype)`).
  // `DOMParser().parseFromString()` builds a separate closure-private document
  // (see module doc comment) and is NOT covered here.
  if (typeof Document !== 'undefined' && Document.prototype) {
    Document.prototype.createExpression = function(expression, resolver) { return xpathCreateExpression(expression, resolver); };
    Document.prototype.createNSResolver = function(nodeResolver) { return xpathCreateNSResolver(nodeResolver); };
    Document.prototype.evaluate = function(expression, contextNode, resolver, type, result) { return xpathEvaluate(expression, contextNode, resolver, type, result); };
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal standard-shaped DOM tree (no native Lumen DOM involved) so the
    /// evaluator's generic `childNodes`/`parentNode`/`attributes` walk can be
    /// exercised without pulling in the whole `install_dom` machinery.
    fn with_xpath() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            var window = globalThis;
            function DOMException(msg, name) { this.message = msg; this.name = name || 'Error'; }
            DOMException.prototype = Object.create(Error.prototype);
            window.DOMException = DOMException;

            function Document() {}

            function mkAttr(owner, name, value) {
                return { nodeType: 2, name: name, nodeName: name, localName: name, namespaceURI: null,
                         value: value, ownerElement: owner };
            }
            function mkAttrs(owner, obj) {
                var names = Object.keys(obj);
                var arr = names.map(function(n) { return mkAttr(owner, n, obj[n]); });
                arr.length = names.length;
                arr.item = function(i) { return arr[i] || null; };
                arr.getNamedItem = function(n) { return arr.filter(function(a){return a.name===n;})[0] || null; };
                return arr;
            }
            function El(tag, attrs, kids) {
                var self = this;
                this.nodeType = 1;
                // Real HTML elements carry the implicit XHTML namespace (not null) —
                // mirror that here so the mock actually exercises the compat path.
                this.nodeName = tag; this.localName = tag;
                this.namespaceURI = 'http://www.w3.org/1999/xhtml';
                this.attributes = mkAttrs(this, attrs || {});
                this.childNodes = (kids || []).map(function(k) {
                    if (typeof k === 'string') return { nodeType: 3, data: k, textContent: k, parentNode: self,
                        isSameNode: function(o){ return o === this; } };
                    k.parentNode = self;
                    return k;
                });
                this.getAttribute = function(n) { var a = this.attributes.getNamedItem(n); return a ? a.value : null; };
            }
            El.prototype.isSameNode = function(o) { return o === this; };
            El.prototype.compareDocumentPosition = function(other) {
                function path(n) { var p = []; while (n) { p.unshift(n); n = n.parentNode; } return p; }
                var pa = path(this), pb = path(other);
                var i = 0;
                while (i < pa.length && i < pb.length && pa[i] === pb[i]) i++;
                if (i >= pa.length || i >= pb.length) return pa.length < pb.length ? 20 : 10;
                var ia = pa[i].parentNode ? pa[i].parentNode.childNodes.indexOf(pa[i]) : -1;
                var ib = pb[i].parentNode ? pb[i].parentNode.childNodes.indexOf(pb[i]) : -1;
                return ia < ib ? 4 : 2;
            };

            var leaf1 = new El('bar', { id: 'b1' }, ['hello']);
            var leaf2 = new El('bar', { id: 'b2' }, ['world']);
            var child = new El('foo', {}, [leaf1, leaf2]);
            var root = new El('root', {}, [child]);
            var doc = Object.create(Document.prototype);
            Object.assign(doc, { nodeType: 9, nodeName: '#document', childNodes: [root], parentNode: null,
                        getElementById: function(id) {
                            var found = null;
                            (function walk(n) { if (found) return; if (n.attributes && n.attributes.getNamedItem('id') && n.attributes.getNamedItem('id').value === id) { found = n; return; } (n.childNodes||[]).forEach(walk); })(root);
                            return found;
                        } });
            root.parentNode = doc;
            "#,
        )
        .unwrap();
        super::install_xpath_v8(&rt).unwrap();
        rt
    }

    fn eval_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn globals_exist() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            "typeof XPathResult === 'function' && typeof XPathEvaluator === 'function' && \
             typeof XPathExpression === 'function' && typeof XPathException === 'function' && \
             typeof Document.prototype.evaluate === 'function' && typeof doc.evaluate === 'function'"
        ));
    }

    #[test]
    fn simple_child_path_returns_matching_elements() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var r = new XPathEvaluator().evaluate('/root/foo/bar', doc, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
            r.snapshotLength === 2 && r.snapshotItem(0).getAttribute('id') === 'b1' && r.snapshotItem(1).getAttribute('id') === 'b2'
            "#
        ));
    }

    #[test]
    fn descendant_shorthand_and_predicate() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var r = new XPathEvaluator().evaluate('//bar[@id="b2"]', doc, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
            r.singleNodeValue !== null && r.singleNodeValue.getAttribute('id') === 'b2'
            "#
        ));
    }

    #[test]
    fn positional_predicate() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var r = new XPathEvaluator().evaluate('//bar[1]', doc, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
            r.singleNodeValue.getAttribute('id') === 'b1'
            "#
        ));
    }

    #[test]
    fn count_and_string_functions() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var ev = new XPathEvaluator();
            ev.evaluate('count(//bar)', doc, null, XPathResult.NUMBER_TYPE, null).numberValue === 2 &&
            ev.evaluate('string(//foo)', doc, null, XPathResult.STRING_TYPE, null).stringValue === 'helloworld' &&
            ev.evaluate('concat("a", "-", "b")', doc, null, XPathResult.STRING_TYPE, null).stringValue === 'a-b'
            "#
        ));
    }

    #[test]
    fn attribute_axis_and_boolean_result() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var ev = new XPathEvaluator();
            ev.evaluate('boolean(//bar/@id)', doc, null, XPathResult.BOOLEAN_TYPE, null).booleanValue === true &&
            ev.evaluate('//bar/@id = "b1"', doc, null, XPathResult.BOOLEAN_TYPE, null).booleanValue === true
            "#
        ));
    }

    #[test]
    fn invalid_expression_throws_xpath_exception() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var threw = false;
            try { new XPathEvaluator().createExpression('///[', null); }
            catch (e) { threw = e instanceof XPathException && e.code === XPathException.INVALID_EXPRESSION_ERR; }
            threw
            "#
        ));
    }

    #[test]
    fn document_evaluate_matches_direct_evaluator_call() {
        let rt = with_xpath();
        assert!(eval_bool(
            &rt,
            r#"
            var r = doc.evaluate('//bar', doc, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
            r.snapshotLength === 2
            "#
        ));
    }
}
