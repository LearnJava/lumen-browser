//! TC39 Decorators (Stage 3) Phase 0 — `@decorator` syntax for QuickJS.
//!
//! QuickJS (ES2023) has no native decorator support: any `@dec` in source is a
//! `SyntaxError`. Phase 0 ships a **pure-JS source-to-source transformer**
//! installed as the global `__lumen_transform_decorators(src)`. The Rust eval
//! entry points (`JsRuntime::eval`, `QuickJsRuntime::eval_module`) pre-process
//! page scripts through it before handing the source to QuickJS.
//!
//! Supported (Phase 0):
//! * class decorators on **named class declarations** (incl. `export class`,
//!   `export default class C`), with factory args: `@dec`, `@ns.dec`, `@dec(1)`,
//!   `@(expr)`; applied bottom-up per spec, return value replaces the class;
//! * method decorators (instance + `static`): `(value, context) -> newValue?`;
//! * field decorators: `(undefined, context) -> initTransformer?` — the
//!   transformer rewrites the initializer inline, so decorator expressions are
//!   evaluated **per instantiation** (spec: once at class definition — Phase 0
//!   deviation, documented here);
//! * well-known symbols `Symbol.ClassDecorator` / `Symbol.MethodDecorator` —
//!   present on the matching decorator `context` objects as `true` tags.
//!
//! Not supported (left untransformed → original `SyntaxError` surfaces, or
//! decorators are stripped with a `console.warn`): class expressions,
//! anonymous classes, accessors (`get`/`set`), `accessor` auto-accessors,
//! `#private` / computed / string member names.
//!
//! The transformer is fail-open: any internal error returns the source
//! unchanged so the engine's own diagnostics are preserved.
//!
//! `// CSS: n/a` — no layout/paint wiring needed.

use rquickjs::{Ctx, Function};

/// Install the decorator transformer shim and well-known symbols into `ctx`.
///
/// Defines the globals `__lumen_transform_decorators`,
/// `__lumen_apply_class_decorators`, `__lumen_apply_method_decorators`,
/// `__lumen_apply_field_decorators`, plus `Symbol.ClassDecorator` and
/// `Symbol.MethodDecorator`. Pure JS; safe to call on a bare context.
pub fn install_decorator_shim(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(DECORATOR_SHIM)?;
    Ok(())
}

/// Pre-process `source` through the JS decorator transformer.
///
/// Returns `Some(transformed)` only when decorator syntax was found and
/// rewritten. Returns `None` when the source contains no `@` (fast path),
/// when the shim is not installed (bare contexts), or when nothing changed —
/// the caller then evaluates the original source as-is.
pub fn maybe_transform_decorators(ctx: &Ctx<'_>, source: &str) -> Option<String> {
    if !source.contains('@') {
        return None;
    }
    let f: Function = ctx.globals().get("__lumen_transform_decorators").ok()?;
    let out: String = f.call((source,)).ok()?;
    if out == source { None } else { Some(out) }
}

/// The combined shim: well-known symbols, runtime apply-helpers and the
/// tokenizer-based source transformer.
const DECORATOR_SHIM: &str = r##"(function(global) {
  'use strict';

  // ── Well-known symbols ─────────────────────────────────────────────────────
  if (typeof Symbol.ClassDecorator !== 'symbol') {
    Object.defineProperty(Symbol, 'ClassDecorator', { value: Symbol('Symbol.ClassDecorator') });
  }
  if (typeof Symbol.MethodDecorator !== 'symbol') {
    Object.defineProperty(Symbol, 'MethodDecorator', { value: Symbol('Symbol.MethodDecorator') });
  }

  // ── Runtime helpers (called by transformed code) ───────────────────────────

  // Class decorators apply bottom-up (closest to the class first); a non-null
  // return value replaces the class.
  global.__lumen_apply_class_decorators = function(cls, decs, name) {
    for (var i = decs.length - 1; i >= 0; i--) {
      var ctx = { kind: 'class', name: name };
      ctx[Symbol.ClassDecorator] = true;
      var r = decs[i](cls, ctx);
      if (r !== undefined && r !== null) cls = r;
    }
    return cls;
  };

  // Method decorators receive (fn, context) and may return a replacement.
  // `owner` is the class for static members, the prototype otherwise.
  global.__lumen_apply_method_decorators = function(owner, name, decs, isStatic, clsName) {
    var value = owner[name];
    for (var i = decs.length - 1; i >= 0; i--) {
      var ctx = { kind: 'method', name: name, static: isStatic, private: false, class: clsName };
      ctx[Symbol.MethodDecorator] = true;
      var r = decs[i](value, ctx);
      if (r !== undefined && r !== null) value = r;
    }
    owner[name] = value;
  };

  // Field decorators receive (undefined, context) and may return an
  // initial-value transformer. Returns the composed transformer; the
  // transformed source calls it with `this` = instance (or class for static).
  global.__lumen_apply_field_decorators = function(decs, name, isStatic) {
    var fns = [];
    for (var i = decs.length - 1; i >= 0; i--) {
      var ctx = { kind: 'field', name: name, static: isStatic, private: false };
      var r = decs[i](undefined, ctx);
      if (typeof r === 'function') fns.push(r);
    }
    return function(v) {
      for (var j = 0; j < fns.length; j++) v = fns[j].call(this, v);
      return v;
    };
  };

  // ── Source transformer ─────────────────────────────────────────────────────

  function isIdStart(c) { return /[A-Za-z_$]/.test(c); }
  function isIdChar(c) { return /[A-Za-z0-9_$]/.test(c); }

  var REGEX_KEYWORDS = ['return', 'typeof', 'instanceof', 'in', 'of', 'new',
    'delete', 'void', 'throw', 'case', 'do', 'else', 'yield', 'await'];

  // Minimal lexer: strings / comments / template literals / regex literals are
  // opaque single tokens so a '@' inside them is never mistaken for a
  // decorator. Token: {t: 'id'|'p'|'str'|'tmpl'|'num'|'re', v, s, e}.
  function tokenize(src) {
    var toks = [];
    var i = 0, n = src.length;
    var tmplStack = []; // per-open-${ } brace counters

    function regexAllowed() {
      if (!toks.length) return true;
      var t = toks[toks.length - 1];
      if (t.t === 'id') return REGEX_KEYWORDS.indexOf(t.v) >= 0;
      if (t.t === 'p') return ')]}'.indexOf(t.v) < 0;
      return false;
    }

    // Scan a template chunk starting at i (src[i] is '`' or the '}' resuming
    // an interpolation). Pushes one opaque 'tmpl' token.
    function scanTemplateChunk() {
      var s = i;
      i++;
      for (;;) {
        if (i >= n) break;
        var ch = src[i];
        if (ch === '\\') { i += 2; continue; }
        if (ch === '`') { i++; break; }
        if (ch === '$' && src[i + 1] === '{') { i += 2; tmplStack.push(0); break; }
        i++;
      }
      toks.push({ t: 'tmpl', v: '', s: s, e: i });
    }

    while (i < n) {
      var c = src[i];
      if (c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === '\f' || c === '\v') { i++; continue; }
      if (c === '/' && src[i + 1] === '/') {
        var nl = src.indexOf('\n', i);
        i = nl < 0 ? n : nl;
        continue;
      }
      if (c === '/' && src[i + 1] === '*') {
        var ce = src.indexOf('*/', i + 2);
        i = ce < 0 ? n : ce + 2;
        continue;
      }
      if (c === '"' || c === "'") {
        var ss = i;
        i++;
        while (i < n && src[i] !== c) { if (src[i] === '\\') i++; i++; }
        i++;
        toks.push({ t: 'str', v: '', s: ss, e: i });
        continue;
      }
      if (c === '`') { scanTemplateChunk(); continue; }
      if (c === '}' && tmplStack.length && tmplStack[tmplStack.length - 1] === 0) {
        tmplStack.pop();
        scanTemplateChunk();
        continue;
      }
      if (c === '/' && regexAllowed()) {
        var rs = i;
        i++;
        var inCls = false, ok = false;
        while (i < n) {
          var rc = src[i];
          if (rc === '\\') { i += 2; continue; }
          if (rc === '\n') break;
          if (rc === '[') inCls = true;
          else if (rc === ']') inCls = false;
          else if (rc === '/' && !inCls) { i++; ok = true; break; }
          i++;
        }
        if (ok) {
          while (i < n && isIdChar(src[i])) i++;
          toks.push({ t: 're', v: '', s: rs, e: i });
        } else {
          i = rs + 1;
          toks.push({ t: 'p', v: '/', s: rs, e: rs + 1 });
        }
        continue;
      }
      if (isIdStart(c) || c === '#') {
        var is = i;
        i++;
        while (i < n && isIdChar(src[i])) i++;
        toks.push({ t: 'id', v: src.slice(is, i), s: is, e: i });
        continue;
      }
      if (c >= '0' && c <= '9') {
        var ns = i;
        i++;
        while (i < n && /[0-9A-Za-z._]/.test(src[i])) i++;
        toks.push({ t: 'num', v: '', s: ns, e: i });
        continue;
      }
      if (c === '{' && tmplStack.length) tmplStack[tmplStack.length - 1]++;
      if (c === '}' && tmplStack.length) tmplStack[tmplStack.length - 1]--;
      toks.push({ t: 'p', v: c, s: i, e: i + 1 });
      i++;
    }
    return toks;
  }

  function transform(src) {
    var toks = tokenize(src);
    var edits = []; // {s, e, text}; s === e means insertion
    var scopes = []; // {cls: null | {name, classDecs, methods}}

    function topClass() {
      var sc = scopes[scopes.length - 1];
      return sc ? sc.cls : null;
    }

    function skipBalanced(k0, open, close) {
      var d = 0;
      for (var k = k0; k < toks.length; k++) {
        if (toks[k].t !== 'p') continue;
        if (toks[k].v === open) d++;
        else if (toks[k].v === close) { d--; if (d === 0) return k; }
      }
      return -1;
    }

    // Parse `@expr @expr ...` starting at token k0 ('@'). Returns
    // {decs: [exprText], next, start, end} or null.
    function parseDecoratorGroup(k0) {
      var decs = [], k = k0;
      var startPos = toks[k0].s, endPos = startPos;
      while (k < toks.length && toks[k].t === 'p' && toks[k].v === '@') {
        var k1 = k + 1;
        if (k1 >= toks.length) return null;
        var exprStart = toks[k1].s;
        if (toks[k1].t === 'p' && toks[k1].v === '(') {
          var r = skipBalanced(k1, '(', ')');
          if (r < 0) return null;
          endPos = toks[r].e;
          k = r + 1;
        } else if (toks[k1].t === 'id') {
          var k2 = k1 + 1;
          while (k2 + 1 < toks.length && toks[k2].t === 'p' && toks[k2].v === '.'
                 && toks[k2 + 1].t === 'id') k2 += 2;
          endPos = toks[k2 - 1].e;
          if (k2 < toks.length && toks[k2].t === 'p' && toks[k2].v === '(') {
            var r2 = skipBalanced(k2, '(', ')');
            if (r2 < 0) return null;
            endPos = toks[r2].e;
            k2 = r2 + 1;
          }
          k = k2;
        } else {
          return null;
        }
        decs.push(src.slice(exprStart, endPos));
      }
      return decs.length ? { decs: decs, next: k, start: startPos, end: endPos } : null;
    }

    // A class declaration (vs expression) heuristic: what precedes it.
    function isStatementPos(prevIdx) {
      if (prevIdx < 0) return true;
      var p = toks[prevIdx];
      if (p.t === 'p') return ';}{'.indexOf(p.v) >= 0;
      if (p.t === 'id') return ['export', 'default', 'else', 'do'].indexOf(p.v) >= 0;
      return false;
    }

    function warnUnsupported(what) {
      if (typeof console !== 'undefined' && console.warn) {
        console.warn('Lumen decorators Phase 0: unsupported decorator target (' + what + '); decorators ignored');
      }
    }

    // Handle `class` header at token kClass; `g` is the pending decorator
    // group (or null), `prevIdx` the token index before the construct.
    // Always consumes through the body '{' and pushes a scope.
    function parseClassHeader(kClass, g, prevIdx) {
      var kn = kClass + 1, name = null;
      if (kn < toks.length && toks[kn].t === 'id' && toks[kn].v !== 'extends') {
        name = toks[kn].v;
        kn++;
      }
      // Find the class-body '{' (skip heritage; braces only occur nested in
      // parens/brackets there).
      var d = 0, kb = kn;
      for (; kb < toks.length; kb++) {
        var t = toks[kb];
        if (t.t !== 'p') continue;
        if (t.v === '(' || t.v === '[') d++;
        else if (t.v === ')' || t.v === ']') d--;
        else if (t.v === '{' && d === 0) break;
      }
      if (kb >= toks.length) return -1;
      var decorate = name !== null && isStatementPos(prevIdx);
      if (g && !decorate) {
        warnUnsupported(name === null ? 'anonymous class' : 'class expression');
        return -1; // leave the group untouched
      }
      if (g) edits.push({ s: g.start, e: g.end, text: '' });
      scopes.push({
        cls: decorate ? { name: name, classDecs: g ? g.decs : [], methods: [] } : null
      });
      return kb + 1;
    }

    // Handle a decorator group on a class member. Returns the next token index.
    function handleMember(g, cls) {
      var k = g.next;
      var isStatic = false;
      while (k < toks.length && toks[k].t === 'id'
             && (toks[k].v === 'static' || toks[k].v === 'async')) {
        var nx = toks[k + 1];
        // Only a modifier when another member name (or '*'/'[') follows;
        // otherwise it's the member name itself.
        if (nx && (nx.t === 'id' || (nx.t === 'p' && (nx.v === '*' || nx.v === '[')))) {
          if (toks[k].v === 'static') isStatic = true;
          k++;
          continue;
        }
        break;
      }
      if (k < toks.length && toks[k].t === 'p' && toks[k].v === '*') k++;

      var unsupported = false;
      if (k < toks.length && toks[k].t === 'id'
          && (toks[k].v === 'get' || toks[k].v === 'set' || toks[k].v === 'accessor')) {
        var nx2 = toks[k + 1];
        if (nx2 && (nx2.t === 'id' || (nx2.t === 'p' && nx2.v === '['))) unsupported = true;
      }
      var name = null;
      if (!unsupported && k < toks.length && toks[k].t === 'id' && toks[k].v.charAt(0) !== '#') {
        name = toks[k].v;
        k++;
      } else {
        unsupported = true;
      }
      if (unsupported) {
        edits.push({ s: g.start, e: g.end, text: '' });
        warnUnsupported('accessor / private / computed member');
        return g.next;
      }

      edits.push({ s: g.start, e: g.end, text: '' });
      var decArr = '[' + g.decs.join(', ') + ']';
      var after = k < toks.length ? toks[k] : null;

      if (after && after.t === 'p' && after.v === '(') {
        cls.methods.push({ name: name, isStatic: isStatic, decs: g.decs });
        return k;
      }

      var helper = '__lumen_apply_field_decorators(' + decArr + ', "' + name + '", ' + isStatic + ')';
      if (after && after.t === 'p' && after.v === '=') {
        // x = INIT;  →  x = HELPER.call(this, (INIT));
        edits.push({ s: after.e, e: after.e, text: ' ' + helper + '.call(this, (' });
        var d = 0, kt = k + 1;
        for (; kt < toks.length; kt++) {
          var t3 = toks[kt];
          if (t3.t !== 'p') continue;
          if ('([{'.indexOf(t3.v) >= 0) d++;
          else if (')]}'.indexOf(t3.v) >= 0) { if (d === 0) break; d--; }
          else if (t3.v === ';' && d === 0) break;
        }
        var endPos = kt < toks.length ? toks[kt].s : src.length;
        edits.push({ s: endPos, e: endPos, text: '))' });
        return k + 1;
      }

      // Field without initializer: x;  →  x = HELPER.call(this, undefined);
      var namePos = toks[k - 1].e;
      edits.push({ s: namePos, e: namePos, text: ' = ' + helper + '.call(this, undefined)' });
      return k;
    }

    // Emit deferred statements right after the class declaration closes.
    // Spec order: method decorators run before class decorators.
    function flushClass(cls, pos) {
      var out = '';
      for (var i = 0; i < cls.methods.length; i++) {
        var m = cls.methods[i];
        var owner = m.isStatic ? cls.name : cls.name + '.prototype';
        out += ';__lumen_apply_method_decorators(' + owner + ', "' + m.name + '", ['
          + m.decs.join(', ') + '], ' + m.isStatic + ', "' + cls.name + '")';
      }
      if (cls.classDecs.length) {
        out += ';' + cls.name + ' = __lumen_apply_class_decorators(' + cls.name
          + ', [' + cls.classDecs.join(', ') + '], "' + cls.name + '")';
      }
      if (out) edits.push({ s: pos, e: pos, text: out + ';' });
    }

    var k = 0;
    while (k < toks.length) {
      var tk = toks[k];
      if (tk.t === 'p' && tk.v === '@') {
        var g = parseDecoratorGroup(k);
        if (!g) { k++; continue; }
        var cls = topClass();
        if (cls) { k = handleMember(g, cls); continue; }
        // Statement level: allow `@dec export [default] class` and
        // `export @dec class` (prevIdx handles the latter).
        var k3 = g.next;
        while (k3 < toks.length && toks[k3].t === 'id'
               && (toks[k3].v === 'export' || toks[k3].v === 'default')) k3++;
        if (k3 < toks.length && toks[k3].t === 'id' && toks[k3].v === 'class') {
          var h = parseClassHeader(k3, g, k - 1);
          if (h >= 0) { k = h; continue; }
        }
        k = g.next;
        continue;
      }
      if (tk.t === 'id' && tk.v === 'class') {
        var h2 = parseClassHeader(k, null, k - 1);
        if (h2 >= 0) { k = h2; continue; }
        k++;
        continue;
      }
      if (tk.t === 'p' && tk.v === '{') { scopes.push({ cls: null }); k++; continue; }
      if (tk.t === 'p' && tk.v === '}') {
        var sc = scopes.pop();
        if (sc && sc.cls) flushClass(sc.cls, tk.e);
        k++;
        continue;
      }
      k++;
    }

    if (!edits.length) return src;
    edits.sort(function(a, b) { return b.s - a.s || b.e - a.e; });
    var out = src;
    for (var ei = 0; ei < edits.length; ei++) {
      var ed = edits[ei];
      out = out.slice(0, ed.s) + ed.text + out.slice(ed.e);
    }
    return out;
  }

  // Fail-open entry point: any transformer error returns the source unchanged
  // so QuickJS's own diagnostics surface.
  global.__lumen_transform_decorators = function(src) {
    src = String(src);
    if (src.indexOf('@') < 0) return src;
    try {
      return transform(src);
    } catch (e) {
      return src;
    }
  };
})(globalThis);
"##;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Transform `src` like the eval hook does, then evaluate; the script's
    /// last expression must be a boolean.
    fn eval_decorated(ctx: &rquickjs::Ctx, src: &str) -> bool {
        let code = maybe_transform_decorators(ctx, src).unwrap_or_else(|| src.to_owned());
        ctx.eval::<bool, _>(code.as_str()).unwrap()
    }

    #[test]
    fn well_known_symbols_defined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "typeof Symbol.ClassDecorator === 'symbol' \
                     && typeof Symbol.MethodDecorator === 'symbol'",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn class_decorator_side_effect_and_context() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function seal(cls, c) {
                  if (c.kind !== 'class' || c.name !== 'A') throw new Error('bad ctx');
                  if (c[Symbol.ClassDecorator] !== true) throw new Error('no tag');
                  cls.sealed = true;
                }
                @seal class A {}
                A.sealed === true
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn class_decorator_factory_replaces_class() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function tag(v) {
                  return function(cls) {
                    return class extends cls { tagged() { return v; } };
                  };
                }
                @tag(7) class B { base() { return 1; } }
                var b = new B();
                b.tagged() === 7 && b.base() === 1
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn multiple_class_decorators_apply_bottom_up() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                var order = [];
                function A() { order.push('A'); }
                function B() { order.push('B'); }
                @A @B class C {}
                order.join('') === 'BA'
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn method_decorator_wraps_method() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function twice(fn, c) {
                  if (c.kind !== 'method' || c.static !== false) throw new Error('bad ctx');
                  if (c[Symbol.MethodDecorator] !== true) throw new Error('no tag');
                  return function() { return fn.apply(this, arguments) * 2; };
                }
                class D { @twice val() { return 21; } }
                new D().val() === 42
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn static_method_decorator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function up(fn, c) {
                  if (!c.static) throw new Error('expected static');
                  return function() { return fn.call(this).toUpperCase(); };
                }
                class E { static greet() { return 'hi'; } @up static shout() { return 'ab'; } }
                E.shout() === 'AB' && E.greet() === 'hi'
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn field_decorator_transforms_initial_value() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function plus10(v, c) {
                  if (c.kind !== 'field') throw new Error('bad ctx');
                  return function(init) { return init + 10; };
                }
                class F { @plus10 x = 5; plain = 1; }
                var f = new F();
                f.x === 15 && f.plain === 1
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn field_decorator_without_initializer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let ok = eval_decorated(
                &ctx,
                r#"
                function def42(v, c) {
                  return function(init) { return init === undefined ? 42 : init; };
                }
                class G { @def42 y; }
                new G().y === 42
                "#,
            );
            assert!(ok);
        });
    }

    #[test]
    fn at_sign_in_strings_and_comments_is_untouched() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            let src = r#"
                // contact: admin@example.com
                var s = "user@host"; var t = `a@${s}@b`;
                class H { m() { return s; } }
                new H().m() === "user@host"
            "#;
            assert!(
                maybe_transform_decorators(&ctx, src).is_none(),
                "source without decorators must pass through unchanged"
            );
            let ok: bool = ctx.eval(src).unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn no_at_sign_fast_path() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_decorator_shim(&ctx).unwrap();
            assert!(maybe_transform_decorators(&ctx, "class I {}").is_none());
        });
    }
}
