
  // ── WebIDL shape pass (BUG-1093) ─────────────────────────────────────────
  // `_IDL` (generated, svg_idl_table.js) lists every attribute/operation/
  // constant the SVG WebIDL gives each interface. For each interface this
  // pass (1) makes the members the hand-written classes above already carry
  // IDL-shaped — enumerable accessors/operations on the PROTOTYPE, getters and
  // operations that brand-check `this`, operations that count their required
  // arguments, `@@toStringTag`, non-writable constants on interface object and
  // prototype, a non-enumerable global — and (2) adds the members they lack,
  // reflecting the content attribute where the IDL says so.
  (function shape_interfaces() {
    var KIND_STRING = { DOMString: 1, USVString: 1, '(DOMString or TrustedScriptURL)': 1 };
    function define(o, k, desc) {
      if (typeof k === 'string' && (desc.get || desc.set)) {
        _fn_name(desc.get, 'get ' + k);
        _fn_name(desc.set, 'set ' + k);
      }
      Object.defineProperty(o, k, desc);
    }
    function ctor_of(name) { return Object.prototype.hasOwnProperty.call(window, name) ? window[name] : undefined; }

    function checker(C) {
      return function(self) {
        if (!(self instanceof C) || self === C.prototype) throw new TypeError('Illegal invocation');
      };
    }
    function wrap_getter(C, get) {
      var check = checker(C);
      return function() { check(this); return get.call(this); };
    }
    function wrap_setter(C, set) {
      var check = checker(C);
      return function(v) { check(this); return set.call(this, v); };
    }
    // [SameObject] attributes: identity kept by `_same_cached`.
    function wrap_same_object(C, name, type, get) {
      var check = checker(C);
      return function() {
        check(this);
        var self = this;
        return _same_cached(self, name, type, function() { return get.call(self); });
      };
    }
    function wrap_op(C, name, req, fn) {
      var check = checker(C);
      var w = function() {
        check(this);
        if (arguments.length < req) {
          throw new TypeError("Failed to execute '" + name + "': " + req + ' argument' +
            (req === 1 ? '' : 's') + ' required, but only ' + arguments.length + ' present.');
        }
        return fn.apply(this, arguments);
      };
      define(w, 'name', { value: name, configurable: true });
      define(w, 'length', { value: req, configurable: true });
      return w;
    }

    function reflect_accessor(C, name, type, flags, iface) {
      var attr = _CONTENT_ATTR[name] || name.toLowerCase();
      var nullable = type.charAt(type.length - 1) === '?';
      var base = nullable ? type.slice(0, -1) : type;
      var check = checker(C);
      var get, set;
      if (base === 'boolean') {
        get = function() { check(this); var n = this.__nid__; return n != null && _lumen_has_attr(n, attr); };
        set = function(v) {
          check(this);
          if (v) _attr_set(this, attr, ''); else _attr_remove(this, attr);
        };
      } else if (KIND_STRING[base]) {
        get = function() {
          check(this);
          var v = _attr_get(this, attr);
          return v === null ? (nullable ? null : '') : v;
        };
        set = function(v) {
          check(this);
          if (v === null && nullable) _attr_remove(this, attr); else _attr_set(this, attr, v);
        };
      } else {
        return null;
      }
      return { get: get, set: flags & 1 ? undefined : set };
    }

    // `<a>` URL utilities (HTML LS hyperlink element utils, minus `href`).
    function url_accessor(C, name, flags) {
      var check = checker(C);
      function parsed(el) {
        try { return new URL(_href_get(el), document.baseURI); } catch (e) { return null; }
      }
      return {
        get: function() {
          check(this);
          var u = parsed(this);
          return u ? String(u[name]) : '';
        },
        set: flags & 1 ? undefined : function(v) {
          check(this);
          var u = parsed(this);
          if (!u) return;
          try { u[name] = String(v); } catch (e) { return; }
          _attr_set(this, 'href', u.href);
        },
      };
    }

    function stub_op(ret) {
      switch (ret) {
        case 'undefined': return function() {};
        case 'boolean': return function() { return false; };
        case 'float': case 'long': case 'unsigned long': return function() { return 0; };
        case 'DOMPoint': return function() { return _pt(0, 0); };
        case 'DOMRect': return function() { return _rect(0, 0, 0, 0); };
        case 'DOMMatrix': return function() { return _mat(); };
        case 'NodeList': return function() { return document.createDocumentFragment().childNodes; };
        case 'SVGNumber': return function() { return _new(SVGNumber); };
        case 'SVGLength': return function() { return _new(SVGLength); };
        case 'SVGAngle': return function() { return _new(SVGAngle); };
        case 'SVGTransform': return function() { return _new(SVGTransform); };
      }
      return function() { return null; };
    }

    // Real bodies for operations the hand-written classes do not carry.
    const OPS = {
      SVGMarkerElement: {
        setOrientToAuto: function() { _attr_set(this, 'orient', 'auto'); },
        setOrientToAngle: function(angle) {
          if (!(angle instanceof SVGAngle)) throw new TypeError('Not an SVGAngle');
          _attr_set(this, 'orient', angle.valueAsString);
        },
      },
    };

    // Attributes with a bespoke accessor.
    const ATTRS = {
      SVGSVGElement: {
        currentScale: function(C) {
          var store = new WeakMap(), check = checker(C);
          return {
            get: function() { check(this); return store.has(this) ? store.get(this) : 1; },
            set: function(v) { check(this); store.set(this, _num(v)); },
          };
        },
        currentTranslate: function(C) {
          var store = new WeakMap(), check = checker(C);
          return {
            get: function() {
              check(this);
              if (!store.has(this)) store.set(this, typeof DOMPointReadOnly === 'function' ? new DOMPointReadOnly(0, 0, 0, 1) : new SVGPoint(0, 0));
              return store.get(this);
            },
          };
        },
      },
      SVGAElement: {
        relList: function(C) {
          var check = checker(C), store = new WeakMap();
          return {
            get: function() {
              check(this);
              if (!store.has(this)) {
                if (this.__nid__ == null || typeof _lumen_make_anchor_rel_list !== 'function') return null;
                store.set(this, _lumen_make_anchor_rel_list(this.__nid__));
              }
              return store.get(this);
            },
            // [PutForwards=value]: assigning forwards to `relList.value`, i.e. `rel`.
            set: function(v) { check(this); _attr_set(this, 'rel', v); },
          };
        },
      },
    };

    Object.keys(_IDL).forEach(function(name) {
      var C = ctor_of(name);
      if (typeof C !== 'function') return;
      var P = C.prototype, spec = _IDL[name], check = checker(C);

      define(window, name, { value: C, writable: true, enumerable: false, configurable: true });
      define(P, Symbol.toStringTag, { value: name, writable: false, enumerable: false, configurable: true });

      Object.keys(spec.c).forEach(function(k) {
        var d = { value: spec.c[k], writable: false, enumerable: true, configurable: false };
        define(C, k, d);
        define(P, k, d);
      });

      spec.a.forEach(function(a) {
        var attr = a[0], type = a[1], flags = a[2], ro = (flags & 1) !== 0;
        var own = Object.getOwnPropertyDescriptor(P, attr);
        var same = _animated_for_type(type);
        if (own && own.get) {
          define(P, attr, {
            get: same ? wrap_same_object(C, attr, type, own.get) : wrap_getter(C, own.get),
            set: ro || !own.set ? undefined : wrap_setter(C, own.set),
            enumerable: true, configurable: true,
          });
          return;
        }
        if (own) return;
        var acc = null;
        var bespoke = ATTRS[name] && ATTRS[name][attr];
        if (bespoke) acc = bespoke(C);
        else if (name === 'SVGAElement' && /^(origin|protocol|username|password|host|hostname|port|pathname|search|hash)$/.test(attr)) {
          acc = url_accessor(C, attr, flags);
        } else if (_animated_for_type(type)) {
          acc = { get: function() { check(this); return _cached_animated(type, attr, this); } };
        } else if (type === 'SVGElement?') {
          acc = { get: function() { check(this); return null; } };
        } else if (type === 'EventHandler') {
          var store = new WeakMap();
          acc = {
            get: function() { check(this); return store.has(this) ? store.get(this) : null; },
            set: function(v) { check(this); store.set(this, typeof v === 'function' ? v : null); },
          };
        } else if (type === 'float') {
          var fstore = new WeakMap();
          acc = {
            get: function() { check(this); return fstore.has(this) ? fstore.get(this) : 0; },
            set: ro ? undefined : function(v) { check(this); fstore.set(this, _num(v)); },
          };
        } else {
          acc = reflect_accessor(C, attr, type, flags, name);
        }
        if (!acc) return;
        define(P, attr, { get: acc.get, set: acc.set, enumerable: true, configurable: true });
      });

      spec.o.forEach(function(o) {
        var op = o[0], req = o[1], ret = o[2];
        var own = Object.getOwnPropertyDescriptor(P, op);
        var fn = own && typeof own.value === 'function' ? own.value
          : OPS[name] && OPS[name][op] ? OPS[name][op]
          : own ? null : stub_op(ret);
        if (!fn) return;
        define(P, op, { value: wrap_op(C, op, req, fn), writable: true, enumerable: true, configurable: true });
      });
    });

    function _animated_for_type(type) {
      return /^SVG(Animated\w+|StringList|PointList)$/.test(type);
    }

    // `Document.rootElement` (SVG 2 §5.1.1 partial interface Document).
    if (typeof Document === 'function'
        && !Object.prototype.hasOwnProperty.call(Document.prototype, 'rootElement')) {
      define(Document.prototype, 'rootElement', {
        get: function() {
          if (!(this instanceof Document) || this === Document.prototype) throw new TypeError('Illegal invocation');
          var e = this.documentElement;
          return e && e instanceof SVGSVGElement ? e : null;
        },
        enumerable: true, configurable: true,
      });
    }
  })();
