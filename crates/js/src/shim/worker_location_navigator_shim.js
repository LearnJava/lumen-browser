
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
