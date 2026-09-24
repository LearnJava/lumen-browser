
(function() {
  function WorkerLocation() { throw new TypeError('Illegal constructor'); }
  WorkerLocation.prototype.toString = function() { return this.href; };
  globalThis.WorkerLocation = WorkerLocation;

  var _LOC_MEMBERS = ['href', 'origin', 'protocol', 'host', 'hostname',
                      'port', 'pathname', 'search', 'hash'];

  // Build the scope's `location` from an absolute URL. Parsing goes through
  // the same `_lumen_parse_url` the page's `location` uses, so an opaque URL
  // (a `data:`/`blob:` worker) degrades the same way there as here instead of
  // throwing.
  globalThis._lumen_make_worker_location = function(url) {
    var p = _lumen_parse_url(String(url == null ? '' : url));
    var loc = Object.create(WorkerLocation.prototype);
    _LOC_MEMBERS.forEach(function(name) {
      var value = String(p[name] == null ? '' : p[name]);
      Object.defineProperty(loc, name, {
        get: function() { return value; },
        enumerable: true, configurable: false,
      });
    });
    return loc;
  };

  // BUG-766: `isSecureContext` (WindowOrWorkerGlobalScope mixin,
  // `[Exposed=(Window,Worker)]`). The page's own rule
  // (`_lumen_url_is_potentially_trustworthy`, Secure Contexts §3.1/§3.2)
  // lives in `web_api_shim_mid_b.js`, which is page-only — a worker scope has
  // no `document`/`window` and cannot share that closure. Duplicated here
  // rather than pulled apart into a shared file (the same call this file's
  // own header makes for `WORKER_ERROR_EVENT_SHIM`/`WORKER_MESSAGE_EVENT_SHIM`
  // in `worker.rs`): the function is small, self-contained (only
  // `parts.href`/`parts.hostname`, both of which `_lumen_parse_url` already
  // supplies), and a real divergence between the two copies would need a
  // deliberate edit to both — same trade the sibling classes already made.
  // By the same reasoning as `_lumen_make_worker_location` just above, the
  // value is computed from the worker's own scope URL: a worker inherits its
  // creator's trust boundary in spec terms, but in practice the worker's own
  // script URL was already fetched through that same same-origin/CSP-gated
  // path, so it is the same boundary restated.
  function _lumen_worker_host_is_loopback(host) {
    var h = String(host || '').toLowerCase();
    if (h === 'localhost' || h === 'localhost.') return true;
    if (h.slice(-10) === '.localhost' || h.slice(-11) === '.localhost.') return true;
    if (h.length > 2 && h.charAt(0) === '[' && h.charAt(h.length - 1) === ']') {
      return _lumen_worker_ipv6_is_loopback(h.slice(1, -1));
    }
    var octets = h.split('.');
    if (octets.length !== 4) return false;
    for (var i = 0; i < 4; i++) {
      var o = octets[i];
      if (o.length === 0 || o.length > 3) return false;
      for (var j = 0; j < o.length; j++) {
        var c = o.charCodeAt(j);
        if (c < 0x30 || c > 0x39) return false;
      }
      if (parseInt(o, 10) > 255) return false;
    }
    return parseInt(octets[0], 10) === 127;
  }
  function _lumen_worker_ipv6_is_loopback(addr) {
    var groups, i;
    var dbl = addr.indexOf('::');
    if (dbl >= 0) {
      if (addr.indexOf('::', dbl + 2) >= 0) return false;
      var head = addr.slice(0, dbl);
      var tail = addr.slice(dbl + 2);
      var h = head === '' ? [] : head.split(':');
      var t = tail === '' ? [] : tail.split(':');
      if (h.length + t.length > 7) return false;
      groups = [];
      for (i = 0; i < h.length; i++) groups.push(h[i]);
      for (i = h.length + t.length; i < 8; i++) groups.push('0');
      for (i = 0; i < t.length; i++) groups.push(t[i]);
    } else {
      groups = addr.split(':');
    }
    if (groups.length !== 8) return false;
    for (i = 0; i < 8; i++) {
      var g = groups[i];
      if (g.length === 0 || g.length > 4) return false;
      for (var j = 0; j < g.length; j++) {
        var c = g.charCodeAt(j) | 0x20;
        var isDigit = c >= 0x30 && c <= 0x39;
        var isHex   = c >= 0x61 && c <= 0x66;
        if (!isDigit && !isHex) return false;
      }
      if (parseInt(g, 16) !== (i === 7 ? 1 : 0)) return false;
    }
    return true;
  }
  function _lumen_worker_url_is_potentially_trustworthy(parts) {
    var href   = String(parts.href || '');
    var colon  = href.indexOf(':');
    var scheme = colon >= 0 ? href.slice(0, colon).toLowerCase() : '';
    if (scheme === 'about') {
      var rest = href.slice(colon + 1);
      return rest === 'blank' || rest === 'srcdoc';
    }
    if (scheme === 'data') return true;
    if (scheme === 'blob') {
      return _lumen_worker_url_is_potentially_trustworthy(_lumen_parse_url(href.slice(colon + 1)));
    }
    if (scheme === 'https' || scheme === 'wss' || scheme === 'file') return true;
    return _lumen_worker_host_is_loopback(parts.hostname);
  }
  globalThis._lumen_worker_secure_context_for = function(url) {
    return _lumen_worker_url_is_potentially_trustworthy(
      _lumen_parse_url(String(url == null ? '' : url)));
  };

  function WorkerNavigator() { throw new TypeError('Illegal constructor'); }
  globalThis.WorkerNavigator = WorkerNavigator;

  var _navId = (typeof _lumen_navigator_id === 'object' && _lumen_navigator_id)
    ? _lumen_navigator_id : {};
  Object.keys(_navId).forEach(function(name) {
    Object.defineProperty(WorkerNavigator.prototype, name, {
      get: function() { return _navId[name]; },
      enumerable: true, configurable: true,
    });
  });

  globalThis.navigator = Object.create(WorkerNavigator.prototype);

  // `WorkerGlobalScope` and its per-flavour subclasses (BUG-777). Feature
  // detection in worker code is written as
  // `'DedicatedWorkerGlobalScope' in self && self instanceof
  // DedicatedWorkerGlobalScope` (WPT's own `post-message-on-load-worker.js` is
  // one line of exactly that), so a scope that answers `false` there does
  // nothing at all — which is why the `type` option could not be measured
  // before this existed.
  //
  // The global object's prototype chain is what carries the `instanceof`, not
  // a `Symbol.hasInstance` trick: with it, `EventTarget.prototype`'s methods
  // reach the scope the way HTML LS says they do, and every own global the
  // shims define with `globalThis.x = …` is unaffected (own properties shadow
  // the chain).
  function WorkerGlobalScope() { throw new TypeError('Illegal constructor'); }
  if (typeof EventTarget === 'function') {
    Object.setPrototypeOf(WorkerGlobalScope.prototype, EventTarget.prototype);
  }
  globalThis.WorkerGlobalScope = WorkerGlobalScope;

  // Called by each flavour's own globals shim with its interface name — the
  // flavour is not knowable here, and a scope must not claim to be one of the
  // other two.
  globalThis._lumen_define_worker_scope = function(name) {
    var Ctor = function() { throw new TypeError('Illegal constructor'); };
    Object.defineProperty(Ctor, 'name', { value: name, configurable: true });
    Object.setPrototypeOf(Ctor.prototype, WorkerGlobalScope.prototype);
    globalThis[name] = Ctor;
    Object.setPrototypeOf(globalThis, Ctor.prototype);
    return Ctor;
  };
})();
