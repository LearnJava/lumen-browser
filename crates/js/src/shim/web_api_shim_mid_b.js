
var _lumen_loc_parts = _lumen_parse_url(typeof _LUMEN_PAGE_URL !== 'undefined' ? _LUMEN_PAGE_URL : '');
var _lumen_loc_href  = _lumen_loc_parts.href;
var _lumen_loc_hash  = _lumen_loc_parts.hash;
// BUG-586: `document.domain`'s backing store (HTML LS "relaxing the
// same-origin restriction"). Separate from `_lumen_loc_parts.hostname`
// because the two diverge the moment a page relaxes its domain — `location`
// must keep reporting the real host, `document.domain` the relaxed one.
// Persists across a same-document navigation (`_lumen_location_update` never
// touches it); a real cross-document navigation gets a fresh JS context, so
// this re-initializes for free rather than needing an explicit reset.
var _lumen_document_domain = _lumen_loc_parts.hostname;

// BUG-765: single source of truth for every `[SecureContext]`-gated surface
// installed below and by the per-module shims that run after `WEB_API_SHIM`
// (generic sensors, Screen Wake Lock, Geolocation's real-position path —
// `window.isSecureContext`'s own getter, further down this file's sibling
// `web_api_shim_tail_mc.js`, reads this same variable rather than
// recomputing it). Safe to call before `_lumen_url_is_potentially_trustworthy`'s
// textual definition below: it is a top-level `function` declaration, which
// hoists across the whole concatenated shim (BUG-378's indirect-eval comment
// on `WEB_API_SHIM`'s installation). A standalone module unit test that
// skips `WEB_API_SHIM` entirely never sets this global, so every gate below
// treats *only* an explicit `false` as insecure — `undefined` (no shim, no
// computed flag) reads as "expose", matching those tests' pre-BUG-765
// behaviour instead of silently hiding the surface they exist to check.
var _lumen_secure_context = _lumen_url_is_potentially_trustworthy(_lumen_loc_parts);

// ── Secure context (W3C Secure Contexts §3.1/§3.2) ──────────────────────────
// BUG-399: `window.isSecureContext` used to be the literal `true`, so every
// `[SecureContext]`-gated API would answer «safe» even on a plain http:// page.
// It is computed here instead, from the very URL the document was installed
// with.
// A host is loopback when it is `localhost` (or a subdomain / trailing-dot form
// of it), a 127.0.0.0/8 address, or the IPv6 loopback — Secure Contexts §3.1.
// The host is matched as written: `_lumen_parse_url` is not a full URL parser
// and does not normalise a shorthand IPv4 literal (`127.1`), so such a form is
// answered «not trustworthy». That is the safe direction to be wrong in — a
// false negative denies a gated API, a false positive would hand it out on an
// insecure origin.
function _lumen_ipv6_is_loopback(addr) {
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
        // Only ::1 itself is loopback; ::ffff:127.0.0.1 is not (per spec).
        if (parseInt(g, 16) !== (i === 7 ? 1 : 0)) return false;
    }
    return true;
}
function _lumen_host_is_loopback(host) {
    var h = String(host || '').toLowerCase();
    if (h === 'localhost' || h === 'localhost.') return true;
    if (h.slice(-10) === '.localhost' || h.slice(-11) === '.localhost.') return true;
    if (h.length > 2 && h.charAt(0) === '[' && h.charAt(h.length - 1) === ']') {
        return _lumen_ipv6_is_loopback(h.slice(1, -1));
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
function _lumen_url_is_potentially_trustworthy(parts) {
    // The scheme is taken from the href rather than from `parts.protocol`:
    // `_lumen_parse_url` splits on the first `://`, so it reads
    // `blob:https://h/id` as protocol `blob:https:` and a `data:` URL whose
    // payload happens to contain `://` as protocol `data:text/html,…:`.
    // The URL Standard's scheme is simply everything before the first colon.
    var href   = String(parts.href || '');
    var colon  = href.indexOf(':');
    var scheme = colon >= 0 ? href.slice(0, colon).toLowerCase() : '';
    // §3.2 short-circuits these before the origin check: `about:blank` and
    // `about:srcdoc` inherit their creator's context, and a `data:` URL is
    // called potentially trustworthy despite its opaque origin.
    if (scheme === 'about') {
        var rest = href.slice(colon + 1);
        return rest === 'blank' || rest === 'srcdoc';
    }
    if (scheme === 'data') return true;
    // A blob: URL carries its origin as the URL that follows the scheme.
    if (scheme === 'blob') {
        return _lumen_url_is_potentially_trustworthy(_lumen_parse_url(href.slice(colon + 1)));
    }
    if (scheme === 'https' || scheme === 'wss' || scheme === 'file') return true;
    return _lumen_host_is_loopback(parts.hostname);
}
// Engine-side URL commit. Writes the backing state ONLY — never through the
// Location accessors, whose setters navigate (HTML LS §7.10.5). Routing an
// internal update through them would turn every committed navigation into a
// fresh navigation request. Previously the components were plain data fields on
// the `location` literal, so the engine and the page wrote to the same slots —
// which is exactly why a page write updated the field and navigated nowhere
// (BUG-376 §2).
function _lumen_location_update(url) {
    _lumen_loc_parts = _lumen_parse_url(url);
    _lumen_loc_href  = _lumen_loc_parts.href;
    _lumen_loc_hash  = _lumen_loc_parts.hash;
}
// ── Location (HTML LS §7.10.5) ──────────────────────────────────────────────
// `Location` is `[LegacyUnforgeable]`: every member is an OWN, non-configurable
// property of the object, not an inherited one, so a page cannot
// `delete location.assign` out from under the scripts that come after it
// (BUG-376 §3). Only `constructor` and `Symbol.toStringTag` live on the
// prototype — the same shape a real browser exposes.
function Location() { throw new TypeError('Illegal constructor'); }
if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(Location.prototype, Symbol.toStringTag,
        { value: 'Location', configurable: true });
}
// A component setter re-serializes the current URL with that one component
// replaced and navigates to the result. The write is delegated to a throwaway
// `URL` object because `URL.prototype` already owns every parsing, encoding and
// re-serialization rule (BUG-375) — hand-patching `_lumen_loc_parts` here would
// be a second, divergent URL writer. A component write that the URL Standard
// ignores (opaque path, invalid scheme, non-numeric port, …) leaves `href`
// untouched and therefore navigates nowhere, which is the required behaviour.
function _lumen_location_set_component(name, value) {
    var u;
    try { u = new URL(_lumen_loc_href); } catch (e) { return; }
    try { u[name] = value; } catch (e) { return; }
    if (u.href === _lumen_loc_href) return;
    _lumen_navigate_or_fragment(u.href, false);
}
var _lumen_location = (function() {
    var loc = Object.create(Location.prototype);
    function accessor(name, getter, setter) {
        var d = { get: getter, enumerable: true, configurable: false };
        if (setter) d.set = setter;
        Object.defineProperty(loc, name, d);
    }
    function component(name) {
        accessor(name,
            function()  { return _lumen_loc_parts[name]; },
            function(v) { _lumen_location_set_component(name, v); });
    }
    accessor('href',
        function()  { return _lumen_loc_href; },
        function(v) { _lumen_navigate_or_fragment(String(v || ''), false); });
    component('protocol');
    component('host');
    component('hostname');
    component('port');
    component('pathname');
    component('search');
    // `hash` keeps its dedicated path: a fragment write is a same-document
    // navigation that also pushes a history entry and fires `hashchange`.
    accessor('hash',
        function()  { return _lumen_loc_hash; },
        function(v) { _lumen_set_location_hash(v); });
    accessor('origin', function() { return _lumen_loc_parts.origin; }); // readonly per spec
    function method(name, fn) {
        Object.defineProperty(loc, name,
            { value: fn, writable: false, enumerable: true, configurable: false });
    }
    method('assign',   function(url) { _lumen_navigate_or_fragment(String(url || ''), false); });
    method('replace',  function(url) { _lumen_navigate_or_fragment(String(url || ''), true); });
    method('reload',   function()    { _lumen_reload(); });
    method('toString', function()    { return _lumen_loc_href; });
    return loc;
})();
// `window.location` is `[LegacyUnforgeable]` + `[PutForwards=href]` (HTML LS
// §7.3.5): an accessor that cannot be redefined, not the writable `var` binding
// this used to be. `window.location = url` now navigates instead of replacing
// the Location object with a string and leaving the rest of the page with a
// broken, unrecoverable `location` (`configurable:false` made it unrestorable —
// BUG-376 §1).
Object.defineProperty(globalThis, 'location', {
    get: function()  { return _lumen_location; },
    set: function(v) { _lumen_location.href = v; },
    enumerable: true, configurable: false
});
// HTML LS Location.hash setter: same-document fragment navigation.
// Mutates only the fragment of the current URL; updates location + history
// without a page reload and fires `popstate` then `hashchange` (HTML LS
// §7.4.6, same rule as `_lumen_navigate_or_fragment` below — BUG-971: this
// dedicated path used to skip `popstate` too). Internal updates use the
// `_lumen_loc_hash` backing var directly to avoid re-triggering this path.
function _lumen_set_location_hash(v) {
    var frag = String(v || '');
    if (frag.charAt(0) === '#') frag = frag.substring(1);
    var baseWithoutFragment = _lumen_loc_href.split('#')[0];
    var newHref = frag.length ? (baseWithoutFragment + '#' + frag) : baseWithoutFragment;
    if (newHref === _lumen_loc_href) return;
    var oldHref = _lumen_loc_href;
    _lumen_location_update(newHref);
    _lumen_history_push('null', newHref);
    _lumen_history_push_url(newHref, 'null');
    _lumen_dispatch_popstate(null);
    _lumen_fire_hashchange(oldHref, newHref);
}
// HTML LS navigation entry point for location.href= / assign() / replace().
// If the resolved target differs from the current URL only in its fragment,
// performs a same-document fragment navigation (no reload): updates location,
// pushes/replaces a same-document history entry, and fires `popstate` then
// `hashchange` (HTML LS §7.4.6 — a forward same-document navigation to a new
// current entry fires `popstate` exactly like a shell-driven traversal does;
// only `pushState`/`replaceState` themselves are excluded from that, per
// spec. BUG-971: this branch used to fire `hashchange` alone, so a page
// waiting on `popstate` from a fragment-only `location.hash =`/`.href =` or
// an `<a href="#x">` click never saw it — only `history.back/forward/go`
// (`_lumen_deliver_popstate`) did).
// Otherwise falls through to a full navigation via `_lumen_navigate`.
function _lumen_navigate_or_fragment(rawUrl, replace) {
    var url = String(rawUrl || '');
    var resolved = null;
    try {
        resolved = new URL(url, _lumen_loc_href).href;
    } catch (e) {
        resolved = null;
    }
    if (resolved !== null) {
        var curBase = _lumen_loc_href.split('#')[0];
        var newBase = resolved.split('#')[0];
        if (curBase === newBase && resolved !== _lumen_loc_href) {
            var oldHref = _lumen_loc_href;
            _lumen_location_update(resolved);
            if (replace) {
                _lumen_history_replace('null', resolved);
                _lumen_history_replace_url(resolved, 'null');
            } else {
                _lumen_history_push('null', resolved);
                _lumen_history_push_url(resolved, 'null');
            }
            _lumen_dispatch_popstate(null);
            _lumen_fire_hashchange(oldHref, resolved);
            return;
        }
    }
    _lumen_navigate(resolved !== null ? resolved : url, replace);
}
// HTML LS §7.10.6: `hashchange` is fired from a task queued on the DOM
// manipulation task source, NOT from the `location.hash` setter itself. The
// difference is observable and is the whole of BUG-832: a page that assigns
// the hash and registers its listener on the very next line — the shape all
// four residual `scroll-to-fragid` tests use, and a perfectly ordinary one,
// since the assignment is what the listener is meant to react to — used to
// miss the event outright, because the dispatch had already run inside the
// assignment.
//
// The event object is built HERE, at queueing time, so it carries the URL pair
// as of the navigation that caused it: two hash writes in one turn deliver two
// events with the right `oldURL`/`newURL` each, in order, rather than both
// reporting whatever `location` settled on.
//
// Written straight into `_lumen_timers` with `nesting: 0` rather than through
// setTimeout, for the same reason as `_ro_schedule_initial` and Animation's
// `_fire`: the §8.6 4 ms clamp is about timer nesting and must not apply to an
// engine-queued task.
function _lumen_fire_hashchange(oldURL, newURL) {
    var ev;
    try {
        ev = new HashChangeEvent('hashchange', { oldURL: oldURL, newURL: newURL, bubbles: false });
    } catch (e) {
        ev = { type: 'hashchange', oldURL: oldURL, newURL: newURL };
    }
    var deadline = _lumen_now_ms();
    _lumen_timers.push({
        id: _lumen_timer_seq++,
        fn: function () { _lumen_dispatch_hashchange(ev); },
        deadline: deadline, interval: null, nesting: 0,
    });
    _lumen_request_wakeup(deadline);
}

// Run the listeners of one queued `hashchange`. A listener that throws is
// reported (BUG-591) and the remaining listeners still run, per §8.5 «invoke».
function _lumen_dispatch_hashchange(ev) {
    if (typeof window.onhashchange === 'function') {
        try { window.onhashchange.call(window, ev); } catch (e) { _lumen_report_exception(e); }
    }
    var arr = _other_win_listeners['hashchange'];
    if (arr) {
        arr = arr.slice();
        for (var i = 0; i < arr.length; i++) {
            try { arr[i].call(window, ev); } catch (e) { _lumen_report_exception(e); }
        }
    }
}

// ── Service Worker API ────────────────────────────────────────────────────────

function _lumen_req_url(r) {
    return (typeof r === 'string') ? r : (r && r.url ? r.url : String(r));
}
function _lumen_req_method(r) {
    return (typeof r === 'string') ? 'GET' : ((r && r.method) ? r.method.toUpperCase() : 'GET');
}
function _lumen_build_response(body, infoJson) {
    var opts = { status: 200, statusText: 'OK', headers: {} };
    if (infoJson) {
        try {
            var m = JSON.parse(infoJson);
            opts.status = m.status || 200;
            opts.statusText = m.statusText || 'OK';
            opts.headers = m.headers || {};
        } catch(e) {}
    }
    return new Response(body, opts);
}

function _lumen_build_cache_object(origin, cacheName) {
    return {
        put: function(request, response) {
            var url = _lumen_req_url(request);
            var method = _lumen_req_method(request);
            var status = response.status || 200;
            var statusText = response.statusText || 'OK';
            var hdrs = {};
            if (response.headers && typeof response.headers.forEach === 'function') {
                response.headers.forEach(function(v, k) { hdrs[k] = v; });
            }
            var metaJson = JSON.stringify({ method: method, status: status, statusText: statusText, headers: hdrs });
            return response.arrayBuffer().then(function(buf) {
                _lumen_cache_put(origin, cacheName, url, metaJson, new Uint8Array(buf));
                return undefined;
            });
        },
        match: function(request, options) {
            var url = _lumen_req_url(request);
            var body = _lumen_cache_match(origin, cacheName, url);
            if (body === undefined || body === null) return Promise.resolve(undefined);
            return Promise.resolve(_lumen_build_response(body, _lumen_cache_match_info(origin, cacheName, url)));
        },
        matchAll: function(request, options) {
            if (request === undefined) {
                var urls = _lumen_cache_keys(origin, cacheName);
                return Promise.resolve(urls.map(function(u) {
                    return _lumen_build_response(
                        _lumen_cache_match(origin, cacheName, u),
                        _lumen_cache_match_info(origin, cacheName, u)
                    );
                }));
            }
            var url = _lumen_req_url(request);
            var body = _lumen_cache_match(origin, cacheName, url);
            if (body === undefined || body === null) return Promise.resolve([]);
            return Promise.resolve([_lumen_build_response(body, _lumen_cache_match_info(origin, cacheName, url))]);
        },
        delete: function(request, options) {
            var url = _lumen_req_url(request);
            return Promise.resolve(_lumen_cache_delete(origin, cacheName, url));
        },
        keys: function(request, options) {
            var entries = JSON.parse(_lumen_cache_keys_full(origin, cacheName));
            if (request !== undefined) {
                var filterUrl = _lumen_req_url(request);
                entries = entries.filter(function(e) { return e.url === filterUrl; });
            }
            return Promise.resolve(entries.map(function(e) {
                return new Request(e.url, { method: e.method });
            }));
        },
        add: function(request) {
            var url = _lumen_req_url(request);
            var self = this;
            return fetch(url).then(function(r) { return self.put(new Request(url), r); });
        },
        addAll: function(requests) {
            var self = this;
            return Promise.all(requests.map(function(r) { return self.add(r); }));
        },
    };
}

var _sw_origin = (typeof location !== 'undefined') ? (location.protocol + '//' + location.host) : '';

var caches = {
    open: function(name) {
        return Promise.resolve(_lumen_build_cache_object(_sw_origin, String(name)));
    },
    match: function(request, options) {
        var url = _lumen_req_url(request);
        var body = _lumen_cache_match_any(_sw_origin, url);
        if (body === undefined || body === null) return Promise.resolve(undefined);
        return Promise.resolve(_lumen_build_response(body, _lumen_cache_match_any_info(_sw_origin, url)));
    },
    has: function(name) {
        return Promise.resolve(_lumen_cache_has(_sw_origin, String(name)));
    },
    delete: function(name) {
        return Promise.resolve(_lumen_cache_delete_cache(_sw_origin, String(name)));
    },
    keys: function() {
        return Promise.resolve(_lumen_cache_names(_sw_origin));
    },
};

// ── Service Worker lifecycle helpers ─────────────────────────────────────────

var _sw_registrations = {};

function _sw_make_event_target() {
    var _listeners = {};
    return {
        addEventListener: function(type, fn) {
            if (!_listeners[type]) _listeners[type] = [];
            _listeners[type].push(fn);
        },
        removeEventListener: function(type, fn) {
            if (!_listeners[type]) return;
            _listeners[type] = _listeners[type].filter(function(f) { return f !== fn; });
        },
        dispatchEvent: function(evt) {
            var handlers = _listeners[evt.type] || [];
            var cb = this['on' + evt.type];
            if (typeof cb === 'function') cb.call(this, evt);
            for (var i = 0; i < handlers.length; i++) { handlers[i].call(this, evt); }
            return !evt.defaultPrevented;
        },
    };
}

function _sw_make_worker(scriptUrl, initState) {
    var et = _sw_make_event_target();
    var w = Object.assign({
        scriptURL: String(scriptUrl),
        state: initState || 'installing',
        onstatechange: null,
        onerror: null,
        postMessage: function() {},
    }, et);
    w._setState = function(s) {
        w.state = s;
        var e = new Event('statechange');
        et.dispatchEvent.call(w, e);
    };
    return w;
}

function _sw_make_registration(scope, scriptUrl) {
    var et = _sw_make_event_target();
    var reg = Object.assign({
        scope: scope,
        scriptURL: String(scriptUrl),
        updateViaCache: 'imports',
        installing: null,
        waiting: null,
        active: null,
        onupdatefound: null,
        update: function() { return Promise.resolve(); },
        unregister: function() {
            _lumen_sw_unregister(_sw_origin, scope);
            delete _sw_registrations[scope];
            _sw_persist();
            return Promise.resolve(true);
        },
    }, et);
    return reg;
}

function _sw_persist() {
    try {
        var snap = [];
        for (var sc in _sw_registrations) {
            var r = _sw_registrations[sc];
            snap.push({
                scope: r.scope,
                scriptURL: r.scriptURL,
                state: r.active ? 'activated' : (r.waiting ? 'installed' : 'installing'),
            });
        }
        _lumen_sw_persist(_sw_origin, JSON.stringify(snap));
    } catch(e) {}
}

function _sw_run_lifecycle(reg) {
    var sw = reg.installing;
    // Notify updatefound
    var uf = new Event('updatefound');
    reg.dispatchEvent(uf);
    // installing → install event → installed → activating → activate → activated
    setTimeout(function() {
        // Fire install event (SW spec §8.2.4)
        var installEvt = new Event('install');
        installEvt.waitUntil = function() {};
        if (sw.state === 'installing') {
            sw._setState('installed');
            reg.waiting = sw;
            reg.installing = null;
            _lumen_sw_register(_sw_origin, reg.scope, reg.scriptURL);
            setTimeout(function() {
                reg.waiting = null;
                sw._setState('activating');
                reg.active = sw;
                _sw_container.controller = sw;
                var activateEvt = new Event('activate');
                activateEvt.waitUntil = function() {};
                sw._setState('activated');
                _sw_persist();
                // PH3-20: fetch SW script and hand it to the Rust execution thread.
                if (typeof fetch !== 'undefined' && typeof _lumen_sw_activate_script === 'function') {
                    (function(scope, scriptURL) {
                        fetch(scriptURL)
                            .then(function(res) { return res.text(); })
                            .then(function(text) {
                                _lumen_sw_activate_script(_sw_origin, scope, text);
                            })
                            .catch(function() {}); // ignore fetch errors — lifecycle still simulated
                    })(reg.scope, reg.scriptURL);
                }
                // Fire controllerchange
                var ce = new Event('controllerchange');
                _sw_container.dispatchEvent(ce);
                // Resolve ready
                if (_sw_ready_resolve) {
                    _sw_ready_resolve(reg);
                    _sw_ready_resolve = null;
                }
            }, 0);
        }
    }, 0);
}

// Restore registrations saved from a previous page load.
(function() {
    try {
        var snap = _lumen_sw_load(_sw_origin);
        if (snap) {
            var arr = JSON.parse(snap);
            for (var i = 0; i < arr.length; i++) {
                var item = arr[i];
                var reg = _sw_make_registration(item.scope, item.scriptURL);
                if (item.state === 'activated' || item.state === 'installed') {
                    var sw = _sw_make_worker(item.scriptURL, item.state);
                    reg.active = sw;
                    _sw_registrations[item.scope] = reg;
                    _lumen_sw_register(_sw_origin, item.scope, item.scriptURL);
                }
            }
        }
    } catch(e) {}
}());

var _sw_ready_resolve = null;
var _sw_ready_promise = new Promise(function(resolve) {
    _sw_ready_resolve = resolve;
    // If already have an active registration, resolve immediately.
    for (var sc in _sw_registrations) {
        if (_sw_registrations[sc].active) {
            resolve(_sw_registrations[sc]);
            _sw_ready_resolve = null;
            break;
        }
    }
});

var _sw_container_et = _sw_make_event_target();
var _sw_container = Object.assign({
    get controller() {
        for (var sc in _sw_registrations) {
            if (_sw_registrations[sc].active) return _sw_registrations[sc].active;
        }
        return null;
    },
    get ready() { return _sw_ready_promise; },
    oncontrollerchange: null,
    onmessage: null,
    onmessageerror: null,
    register: function(scriptUrl, options) {
        var scope = (options && options.scope) ? String(options.scope) : '/';
        var existing = _sw_registrations[scope];
        if (existing && existing.active && existing.scriptURL === String(scriptUrl)) {
            return Promise.resolve(existing);
        }
        var reg = _sw_make_registration(scope, scriptUrl);
        var sw = _sw_make_worker(scriptUrl, 'installing');
        reg.installing = sw;
        _sw_registrations[scope] = reg;
        // Register immediately in Rust-side map (for _lumen_sw_has_registration sync checks).
        _lumen_sw_register(_sw_origin, scope, String(scriptUrl));
        _sw_run_lifecycle(reg);
        return Promise.resolve(reg);
    },
    getRegistration: function(url) {
        var u = url || _sw_origin + '/';
        for (var sc in _sw_registrations) {
            if (String(u).indexOf(sc) === 0) return Promise.resolve(_sw_registrations[sc]);
        }
        return Promise.resolve(undefined);
    },
    getRegistrations: function() {
        return Promise.resolve(Object.values(_sw_registrations));
    },
}, _sw_container_et);

var navigator = {
    userAgent: 'Lumen/0.5.0',
    language: 'en-US',
    onLine: false,
    // Beacon API (W3C Beacon §3.1): fire-and-forget POST to url.
    // data may be string | URLSearchParams | FormData | Blob | ArrayBuffer | null.
    sendBeacon: function(url, data) {
        var body = '';
        var ct = '';
        if (data == null) {
            body = '';
        } else if (typeof data === 'string') {
            body = data;
            ct = 'text/plain;charset=UTF-8';
        } else if (typeof URLSearchParams !== 'undefined' && data instanceof URLSearchParams) {
            body = data.toString();
            ct = 'application/x-www-form-urlencoded;charset=UTF-8';
        } else if (typeof FormData !== 'undefined' && data instanceof FormData) {
            body = typeof data._toUrlEncoded === 'function' ? data._toUrlEncoded() : '';
            ct = 'application/x-www-form-urlencoded;charset=UTF-8';
        } else if (typeof Blob !== 'undefined' && data instanceof Blob) {
            body = typeof data._data === 'string' ? data._data : '';
            ct = data.type || 'application/octet-stream';
        }
        try {
            var ok = _lumen_send_beacon(url, body, ct);
            if (!ok && typeof _lumen_beacon_last_csp_block === 'function') {
                _lumen_fire_connect_src_violation(_lumen_beacon_last_csp_block());
            }
            return ok;
        } catch(e) { return false; }
    },
};

// BUG-765: `navigator.serviceWorker` is `[SecureContext]` (Service Workers
// §2.9) — absent entirely on an insecure origin, not merely inert, per
// `'X' in window/navigator === false` (see `_lumen_secure_context`'s doc
// comment, this file's top, for the `undefined`-reads-as-secure convention).
if (_lumen_secure_context !== false) {
    navigator.serviceWorker = _sw_container;
}

// ── Clipboard API (W3C Clipboard API §4) ─────────────────────────────────────
// navigator.clipboard.readText()  → Promise<string>
// navigator.clipboard.writeText(text) → Promise<void>
// navigator.clipboard.read()  → Promise<ClipboardItems> stub (empty array)
// navigator.clipboard.write() → Promise<void> stub
//
// readText/writeText delegate to native bindings (_lumen_clipboard_read /
// _lumen_clipboard_write) when the shell wires them.  Until then readText
// returns '' and writeText silently succeeds.
//
// BUG-765: the `Clipboard` interface is `[SecureContext]` (Clipboard API
// §4.1 partial `Navigator`) — absent entirely on an insecure origin, not
// merely inert (see `_lumen_secure_context`'s doc comment, this file's top).
if (_lumen_secure_context !== false) {
navigator.clipboard = {
    readText: function() {
        return new Promise(function(resolve, reject) {
            try {
                var text = (typeof _lumen_clipboard_read === 'function')
                    ? _lumen_clipboard_read() : '';
                resolve(typeof text === 'string' ? text : '');
            } catch(e) { reject(e); }
        });
    },
    writeText: function(text) {
        return new Promise(function(resolve, reject) {
            try {
                if (typeof _lumen_clipboard_write === 'function') {
                    _lumen_clipboard_write(String(text == null ? '' : text));
                }
                resolve(undefined);
            } catch(e) { reject(e); }
        });
    },
    read:  function() { return Promise.resolve([]); },
    write: function() { return Promise.resolve(undefined); },
};
}

// ── Permissions API (W3C Permissions §5) ─────────────────────────────────────
// Lives in `crates/js/src/permissions.rs`, not here: BUG-386 replaced the 25
// lines that used to sit at this spot — one deny list of 11 names and `granted`
// for everything else, including names this engine has never heard of — with a
// recognised-name registry that rejects the rest with a TypeError, and a
// PermissionStatus that is a real EventTarget.

// ── Timer queue (HTML LS §8.6 «timers») ──────────────────────────────────────
// Timers are stored as a JS-side array; Rust drains them each event loop tick
// via _lumen_tick_timers() called from about_to_wait. When a new timer is
// scheduled, _lumen_request_wakeup(deadline_ms) notifies the shell so that
// ControlFlow::WaitUntil wakes the loop at the right time.
var _lumen_timer_seq = 1;
var _lumen_timers = [];
// HTML LS §8.6 «timer nesting level»: callbacks scheduled from inside a timer
// callback inherit nesting+1; past level 5 the timeout is clamped to >=4 ms
// (BUG-271: without this, setTimeout(fn,0) chains and setInterval(fn,0) wake
// the shell event loop as fast as it can spin — a full busy core per page).
var _lumen_timer_nesting = 0;

function _lumen_clamp_timeout(ms, nesting) {
    return (nesting > 5 && ms < 4) ? 4 : ms;
}

// WebIDL `long` — the type §8.6 declares for the `timeout` argument — is
// ToNumber followed by ToInt32, and only then does step 5 «if timeout is less
// than 0, set timeout to 0» apply. `(typeof delay === 'number' && delay > 0)`
// implemented neither half (BUG-847), and got four separate answers wrong:
// `Math.pow(2, 32)` armed a timer 49 days out where ToInt32 makes it 0,
// `Math.pow(2, 31)` the same via the negative side of the modulo, `Infinity`
// produced a deadline nothing can ever reach, and a delay that is not already
// a number — `'100'`, an object with a `valueOf` — was silently taken as 0
// instead of being converted. This is `_toDelay` of `WORKER_TIMERS_SHIM`
// verbatim: §8.6 is `WindowOrWorkerGlobalScope`, so the two must agree.
function _lumen_timer_delay(v) {
    var n = Number(v) | 0;
    return n < 0 ? 0 : n;
}

// The handle of `clearTimeout`/`clearInterval` is a WebIDL `long` too, so
// `clearTimeout(String(id))` must cancel the timer `id` — the strict `===`
// against a raw argument never matched one (BUG-847, same defect one function
// over; `cancelAnimationFrame` next door already converted its own).
function _lumen_timer_handle(v) {
    return Number(v) | 0;
}

// HTML LS §8.6 «timer initialization steps»: a handler that is not a Function
// is taken as a string and run as a **classic script** — compiled when the
// timer FIRES, not when it is scheduled, and afresh on every firing of a
// `setInterval` (BUG-831: both entry points used to answer `0` and queue
// nothing, which a page cannot tell apart from a timer that is not due yet).
//
// `(0, eval)` — indirect eval — is what makes it a classic script: it runs in
// global scope, so `setTimeout('var x = 1')` creates a global the way every
// other engine does, where a direct `eval(src)` call would evaluate the code
// inside this closure and throw the assignment away with it.
//
// TRUSTEDTYPES-1 срез 1: the non-function handler is a Trusted Types script
// sink (HTML LS timer-initialisation steps, TT L2 §4.1) — under
// `require-trusted-types-for 'script'` a plain string must pass through
// `defaultPolicy.createScript` or the call throws, checked synchronously here
// (i.e. at schedule time, when `setTimeout`/`setInterval` calls this
// function), same as the spec's timer initialisation steps run synchronously.
// Compilation itself stays lazy (BUG-831) — only the compliance check moves
// earlier.
function _lumen_timer_string_handler(code, sink) {
    var src = (typeof _lumen_tt_get_compliant_script === 'function')
        ? _lumen_tt_get_compliant_script(code, sink)
        : String(code);
    return function () { (0, eval)(src); };
}

function _lumen_tick_timers() {
    var now = _lumen_now_ms();
    var ready = [];
    var keep = [];
    for (var i = 0; i < _lumen_timers.length; i++) {
        var t = _lumen_timers[i];
        if (t.deadline <= now) {
            ready.push(t);
        } else {
            keep.push(t);
        }
    }
    _lumen_timers = keep;
    // Re-schedule intervals before running callbacks (matches spec §8.6 step 18).
    for (var j = 0; j < ready.length; j++) {
        var r = ready[j];
        if (r.interval !== null) {
            var rn = (r.nesting || 1) + 1;
            var riv = _lumen_clamp_timeout(r.interval, rn);
            _lumen_timers.push({ id: r.id, fn: r.fn, deadline: now + riv, interval: r.interval, nesting: rn });
        }
    }
    // Run callbacks; an uncaught exception is reported (HTML §8.6 step 17
    // "report the exception"), not swallowed -- BUG-591.
    // The callback's nesting level is active while it runs so timers it
    // schedules inherit level+1 (§8.6 step 3).
    for (var k = 0; k < ready.length; k++) {
        _lumen_timer_nesting = ready[k].nesting || 1;
        try { ready[k].fn(); } catch(e) { _lumen_report_exception(e); }
    }
    _lumen_timer_nesting = 0;
    // Notify shell of next wakeup if any timers remain.
    if (_lumen_timers.length > 0) {
        var next = _lumen_timers[0].deadline;
        for (var m = 1; m < _lumen_timers.length; m++) {
            if (_lumen_timers[m].deadline < next) next = _lumen_timers[m].deadline;
        }
        _lumen_request_wakeup(next);
    }
}

function setTimeout(fn, delay) {
    if (typeof fn !== 'function') fn = _lumen_timer_string_handler(fn, 'Window setTimeout');
    var nesting = _lumen_timer_nesting + 1;
    var ms = _lumen_timer_delay(delay);
    ms = _lumen_clamp_timeout(ms, nesting);
    var id = _lumen_timer_seq++;
    var deadline = _lumen_now_ms() + ms;
    _lumen_timers.push({ id: id, fn: fn, deadline: deadline, interval: null, nesting: nesting });
    _lumen_request_wakeup(deadline);
    return id;
}

function clearTimeout(id) {
    var handle = _lumen_timer_handle(id);
    for (var i = 0; i < _lumen_timers.length; i++) {
        if (_lumen_timers[i].id === handle) { _lumen_timers.splice(i, 1); return; }
    }
}

function setInterval(fn, interval) {
    if (typeof fn !== 'function') fn = _lumen_timer_string_handler(fn, 'Window setInterval');
    var nesting = _lumen_timer_nesting + 1;
    var ms = _lumen_timer_delay(interval);
    var first = _lumen_clamp_timeout(ms, nesting);
    var id = _lumen_timer_seq++;
    var deadline = _lumen_now_ms() + first;
    _lumen_timers.push({ id: id, fn: fn, deadline: deadline, interval: ms, nesting: nesting });
    _lumen_request_wakeup(deadline);
    return id;
}

function clearInterval(id) { clearTimeout(id); }

// ── requestAnimationFrame / cancelAnimationFrame (HTML §8.1.5.1) ──────────────
// Callbacks are queued per-frame and called by Rust via _lumen_run_raf_callbacks
// before each paint. Each callback receives a DOMHighResTimeStamp.
var _lumen_raf_seq = 1;
var _lumen_raf_callbacks = [];

function requestAnimationFrame(fn) {
    if (typeof fn !== 'function') return 0;
    var id = _lumen_raf_seq++;
    _lumen_raf_callbacks.push({ id: id, fn: fn });
    _lumen_mark_raf_pending();
    return id;
}

function cancelAnimationFrame(id) {
    id = id | 0;
    for (var i = 0; i < _lumen_raf_callbacks.length; i++) {
        if (_lumen_raf_callbacks[i].id === id) {
            _lumen_raf_callbacks.splice(i, 1);
            return;
        }
    }
}

// Called by the shell event loop before each paint with the frame timestamp.
// Snapshot-pattern per spec: new rAF calls during callbacks go into the NEXT
// frame. Returns true when any callback was invoked (for relayout check).
// timestamp_ms < 0 → use performance.now() (live DOMHighResTimeStamp, EE-5);
// timestamp_ms >= 0 → use as-is (0 = deterministic mode, frozen clock).
// All callbacks in a batch receive the SAME timestamp (captured once at start).
function _lumen_run_raf_callbacks(timestamp_ms) {
    var ts = timestamp_ms < 0 ? performance.now() : +timestamp_ms;
    _wa_current_time = ts;
    var callbacks = _lumen_raf_callbacks.splice(0);
    var ran = false;
    if (callbacks.length !== 0) {
        ran = true;
        for (var i = 0; i < callbacks.length; i++) {
            try { callbacks[i].fn(ts); } catch(e) { _lumen_report_exception(e); }
        }
    }
    // BUG-600: the focus fixup rule runs at the very end of "update the
    // rendering" — after rAF callbacks (and, since `ResizeObserver`'s own
    // delivery loop is itself queued through this same callback array, after
    // resize observations too) — so a callback that reads
    // `document.activeElement` still sees the pre-fixup value.
    _lumen_focus_fixup();
    return ran;
}

var _popstate_listeners = [];

// Dispatches one `popstate` event carrying `state` to `window.onpopstate` and
// every listener registered via `addEventListener('popstate', ...)`. Shared
// by the two paths HTML LS §7.4.6 requires it on: shell-driven traversal
// (`_lumen_deliver_popstate`, below) and forward same-document fragment
// navigation (`_lumen_navigate_or_fragment`, BUG-971) — `pushState`/
// `replaceState` themselves stay excluded, per spec.
function _lumen_dispatch_popstate(state) {
    var ev = new PopStateEvent('popstate', { state: state, bubbles: true });
    if (typeof window.onpopstate === 'function') {
        try { window.onpopstate(ev); } catch (e) { _lumen_report_exception(e); }
    }
    for (var i = 0; i < _popstate_listeners.length; i++) {
        try { _popstate_listeners[i](ev); } catch (e) { _lumen_report_exception(e); }
    }
}

// Called by the shell (via eval_js) when the user navigates back/forward to a
// same-document (pushState) history entry.  Updates location and fires popstate.
// state_json is already valid JSON; url may be empty (means keep current).
// HTML LS §7.4.6: traversing between two entries that differ only in their
// fragment fires popstate AND hashchange (popstate first, hashchange after).
function _lumen_deliver_popstate(state_json, url) {
    var oldHref = _lumen_loc_href;
    var oldHash = oldHref.indexOf('#') >= 0 ? oldHref.slice(oldHref.indexOf('#')) : '';
    // Since BUG-829 an entry URL is absolute by the time it is stored, so this
    // resolve is a no-op for anything the engine itself wrote. It stays because
    // a traversal must never throw: a stale or odd value is resolved leniently
    // here rather than rejected the way `pushState` rejects it.
    var target = url ? _url_resolve(String(url), _lumen_document_base_url()) : '';
    if (target) _lumen_location_update(target);
    // Sync the JS-side HistoryState mirror so history.state reflects the
    // state object delivered by a shell-driven traversal (HTML LS §7.4.6).
    _lumen_history_set_state(state_json);
    var newHref = target ? target : oldHref;
    var newHash = newHref.indexOf('#') >= 0 ? newHref.slice(newHref.indexOf('#')) : '';
    var s;
    try { s = JSON.parse(state_json); } catch(e) { s = null; }
    _lumen_dispatch_popstate(s);
    if (target && oldHash !== newHash) {
        _lumen_fire_hashchange(oldHref, newHref);
    }
}

// HTML LS §7.4.6 «shared history push/replace state steps» step 3: the `url`
// argument of `pushState`/`replaceState` is parsed relative to the DOCUMENT
// BASE URL, and it is the *serialization of the result* — an absolute URL —
// that becomes the entry's, and therefore the document's, URL. Before BUG-829
// the raw argument was handed to `_lumen_location_update` verbatim, so an SPA
// router's first `pushState(s, '', '/products/42')` left `location.href` as
// `/products/42` with an empty `search`: on the *successful* path, with
// nothing in the console, and with every later absolute-link build, query read
// or origin comparison on the page running off that garbage.
// Returns the absolute URL, or throws the `SecurityError` the spec asks for
// (steps 3.2/3.3) when the URL does not parse or this document may not be
// rewritten to it.
function _lumen_history_state_url(url) {
    var resolved;
    try {
        resolved = new URL(String(url), _lumen_document_base_url()).href;
    } catch (e) {
        throw new DOMException(
            'pushState/replaceState: cannot parse ' + String(url) + ' as a URL',
            'SecurityError');
    }
    if (!_lumen_history_can_rewrite_url(resolved)) {
        throw new DOMException(
            'pushState/replaceState: a document at ' + _lumen_loc_href +
            ' cannot have its URL rewritten to ' + resolved,
            'SecurityError');
    }
    return resolved;
}
// HTML LS «can have its URL rewritten»: a target differing from the document
// URL in scheme, credentials, host or port is refused outright; an HTTP(S)
// document may then move anywhere inside its origin (path, query, fragment),
// while under any other scheme (`file:`, `about:`, `data:`, `blob:`) only the
// fragment may differ. The spec names `file:` in a step of its own, but its
// next step covers the query for that scheme too, so both collapse into the
// single comparison below.
function _lumen_history_can_rewrite_url(target) {
    var t = _lumen_parse_url(target), d = _lumen_loc_parts;
    if (t.protocol !== d.protocol || t.username !== d.username
        || t.password !== d.password || t.host !== d.host) return false;
    if (t.protocol === 'http:' || t.protocol === 'https:') return true;
    return t.pathname === d.pathname && t.search === d.search;
}

var history = {
    get length()  {
        var m = _lumen_history_length();
        try { var st = JSON.parse(_lumen_navigation_entries_json()); if (st && st.entries && st.entries.length > m) return st.entries.length; } catch (e) {}
        return m;
    },
    get state()   {
        try { return JSON.parse(_lumen_history_state_json()); } catch(e) { return null; }
    },
    // `url` is a nullable DOMString defaulting to null, so only an omitted (or
    // explicitly null) argument leaves the document URL alone; an empty string
    // is an ordinary relative reference and resolves to the base URL. The
    // resolution runs BEFORE anything is stored, because its `SecurityError`
    // must leave the session history untouched (HTML LS §7.4.6 step 3).
    pushState:    function(state, title, url) {
        var target = (url === undefined || url === null) ? null : _lumen_history_state_url(url);
        var new_state_json = JSON.stringify(state !== undefined ? state : null);
        _lumen_history_push(new_state_json, target === null ? '' : target);
        if (target !== null) {
            _lumen_location_update(target);
            _lumen_history_push_url(target, new_state_json);
        }
    },
    replaceState: function(state, title, url) {
        var target = (url === undefined || url === null) ? null : _lumen_history_state_url(url);
        var new_state_json = JSON.stringify(state !== undefined ? state : null);
        _lumen_history_replace(new_state_json, target === null ? '' : target);
        if (target !== null) {
            _lumen_location_update(target);
            _lumen_history_replace_url(target, new_state_json);
        }
    },
    back:    function() { history.go(-1); },
    forward: function() { history.go(1); },
    go: function(delta) {
        // HTML LS (history traversal): history.go(0) reloads the current document.
        if ((delta | 0) === 0) {
            _lumen_reload();
            return;
        }
        // Non-zero delta: traversal is now SHELL-AUTHORITATIVE. Move the JS
        // read-cache cursor (keeps history.state/length and pushState truncation
        // correct), and on success queue the real traversal so the shell moves
        // its nav_back/nav_fwd stacks and delivers the destination popstate (same-
        // document) or reload (full-document). We no longer fire popstate here —
        // that avoids a double popstate and lets the shell decide same-doc vs reload.
        var d = (delta | 0);
        var ok = _lumen_history_go(d);
        if (ok) {
            _lumen_history_traverse(d);
        } else {
            // The mirror is a same-document read cache; after a cross-document
            // navigation only the shell state knows the full session history.
            try {
                var st = JSON.parse(_lumen_navigation_entries_json());
                if (st && st.entries && st.entries.length > 0 && 0 <= st.index + d && st.index + d < st.entries.length) {
                    _lumen_history_traverse(d);
                }
            } catch (e) {}
        }
    },
};

// ── Server-Sent Events API (HTML Living Standard §9.2) ─────────────────────
// Phase 0 model: synchronous connect; background recv thread queues events;
// JS polls via _lumen_pump_sse(). Mirrors the WebSocket polling model.

var _sse_instances = [];

function _lumen_sse_fire(es, type, ev) {
    ev.type = type;
    es.dispatchEvent(ev);
}

function _lumen_sse_pump_one(es) {
    if (!es._handle) return;
    var raw;
    while ((raw = _lumen_sse_poll(es._handle)) !== null && raw !== undefined) {
        try {
            var ev = JSON.parse(raw);
            if (ev.t === 'open') {
                if (es._readyState === 2) { continue; }
                es._readyState = 1;
                _lumen_sse_fire(es, 'open', new Event('open', { isTrusted: true }));
            } else if (ev.t === 'message') {
                if (es._readyState === 2) { continue; }
                var type = ev.event || 'message';
                var me = new MessageEvent(ev.data != null ? ev.data : '', { isTrusted: true });
                me.type = type;
                me.lastEventId = ev.id != null ? ev.id : '';
                me.origin = es._origin;
                if (me.lastEventId) { es._lastEventId = me.lastEventId; }
                _lumen_sse_fire(es, type, me);
            } else if (ev.t === 'retry') {
                // Server requested a specific reconnect delay (HTML Living Standard §9.2.3).
                if (typeof ev.ms === 'number' && ev.ms >= 0) { es._retryMs = ev.ms; }
            } else if (ev.t === 'reconnecting') {
                // The stream ended and the session is re-establishing the
                // connection (HTML Living Standard §9.2.5 step 1): readyState
                // CONNECTING + `error`. The reconnection itself belongs to the
                // native session — the same handle stays valid and a later
                // 'open' announces the new connection (BUG-844). Doing it here
                // as well would open a second connection per drop.
                if (es._readyState === 2) { continue; }
                es._readyState = 0; // CONNECTING
                _lumen_sse_fire(es, 'error', new Event('error', { isTrusted: true }));
            } else if (ev.t === 'close') {
                // Terminal: the native session stopped producing events (the
                // page called close(), so its recv loop was cancelled). No
                // reconnect — a stream that merely ended reports 'reconnecting'.
                _lumen_sse_close(es._handle);
                es._handle = 0;
                if (es._readyState !== 2) {
                    es._readyState = 2; // CLOSED
                    _lumen_sse_fire(es, 'error', new Event('error', { isTrusted: true }));
                }
                break;
            } else if (ev.t === 'error') {
                // Network or protocol error: fire error and close (no reconnect for hard errors).
                es._readyState = 2;
                var err = new Event('error', { isTrusted: true });
                err.message = ev.message;
                _lumen_sse_fire(es, 'error', err);
                es._handle = 0;
                break;
            }
        } catch(ignore) {}
    }
}

function _lumen_pump_sse() {
    for (var i = _sse_instances.length - 1; i >= 0; i--) {
        _lumen_sse_pump_one(_sse_instances[i]);
        if (_sse_instances[i]._readyState === 2 && !_sse_instances[i]._handle) {
            _sse_instances.splice(i, 1);
        }
    }
}

// EventSource (HTML Living Standard §9.2): extends EventTarget (BUG-363 pt.3)
// so addEventListener/dispatchEvent are the shared mechanism rather than a
// private ad-hoc registry. url/readyState/withCredentials are readonly
// accessor properties backed by private instance fields (pt.4);
// onopen/onmessage/onerror are accessor properties too (pt.5), which lets
// EventTarget.prototype.dispatchEvent's generic `this['on' + type]` lookup
// pick them up without EventSource-specific dispatch code.
function EventSource(url, opts) {
    if (!new.target) {
        throw new TypeError("Failed to construct 'EventSource': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
    }
    EventTarget.call(this);
    var _rawUrl = String(url);
    // Resolve relative to the document's base URL (HTML Living Standard §9.2.2
    // step 3); a URL the parser rejects outright throws SyntaxError (BUG-363
    // pt.6). Note: the shim's URL parser (BUG-693) is lenient about malformed
    // authorities, so some invalid URLs still resolve instead of throwing —
    // that gap is tracked separately, not reopened here.
    var _resolved;
    try { _resolved = new URL(_rawUrl, _lumen_loc_href).href; }
    catch (e) { throw new DOMException("Failed to construct 'EventSource': The URL '" + _rawUrl + "' is invalid.", 'SyntaxError'); }
    this._url = _resolved;
    this._readyState = 0; // CONNECTING
    this._withCredentials = !!(opts && opts.withCredentials);
    this._onopen = null;
    this._onmessage = null;
    this._onerror = null;
    this._handle = 0;
    this._lastEventId = '';
    this._retryMs = 3000; // default reconnect delay (HTML Living Standard §9.2.7)
    // Origin best-effort: scheme+host of the target URL (for MessageEvent.origin).
    this._origin = '';
    var _sep = this._url.indexOf('://');
    if (_sep >= 0) {
        var _rest = this._url.slice(_sep + 3);
        var _end = _rest.length;
        var _slash = _rest.indexOf('/'); if (_slash >= 0 && _slash < _end) _end = _slash;
        var _q = _rest.indexOf('?'); if (_q >= 0 && _q < _end) _end = _q;
        var _hash = _rest.indexOf('#'); if (_hash >= 0 && _hash < _end) _end = _hash;
        this._origin = this._url.slice(0, _sep + 3) + _rest.slice(0, _end);
    }
    var self = this;
    var h = _lumen_sse_connect(this._url);
    if (!h) {
        // No provider, or the connection could not be established. Per spec
        // readyState stays CONNECTING synchronously (BUG-363 pt.7); the queued
        // failure task is what transitions it to CLOSED and fires 'error'.
        // GAP-CSPENF срез 11: read the `connect-src` side channel before the
        // queued task runs — same reasoning as the WebSocket branch above.
        var sseCsp = (typeof _lumen_sse_last_csp_block === 'function') ? _lumen_sse_last_csp_block() : null;
        setTimeout(function() {
            _lumen_fire_connect_src_violation(sseCsp);
            self._readyState = 2; // CLOSED
            var e = new Event('error', { isTrusted: true });
            e.message = 'EventSource connection failed';
            _lumen_sse_fire(self, 'error', e);
        }, 0);
        return;
    }
    this._handle = h;
    _sse_instances.push(this);
    // Phase 0: no persistent event loop — caller must invoke _lumen_pump_sse()
    // after setting onopen/onmessage to receive queued events.
}
EventSource.prototype = Object.create(EventTarget.prototype);
EventSource.prototype.constructor = EventSource;
EventSource.prototype.close = function() {
    if (this._handle) {
        _lumen_sse_close(this._handle);
        this._handle = 0;
    }
    this._readyState = 2; // CLOSED
};
Object.defineProperty(EventSource.prototype, 'url', {
    get: function() { return this._url; }, enumerable: true, configurable: true,
});
Object.defineProperty(EventSource.prototype, 'readyState', {
    get: function() { return this._readyState; }, enumerable: true, configurable: true,
});
Object.defineProperty(EventSource.prototype, 'withCredentials', {
    get: function() { return this._withCredentials; }, enumerable: true, configurable: true,
});
Object.defineProperty(EventSource.prototype, 'onopen', {
    get: function() { return this._onopen; },
    set: function(fn) { this._onopen = (typeof fn === 'function') ? fn : null; },
    enumerable: true, configurable: true,
});
Object.defineProperty(EventSource.prototype, 'onmessage', {
    get: function() { return this._onmessage; },
    set: function(fn) { this._onmessage = (typeof fn === 'function') ? fn : null; },
    enumerable: true, configurable: true,
});
Object.defineProperty(EventSource.prototype, 'onerror', {
    get: function() { return this._onerror; },
    set: function(fn) { this._onerror = (typeof fn === 'function') ? fn : null; },
    enumerable: true, configurable: true,
});
// WebIDL constants live on both the interface object and its prototype
// (BUG-363 pt.1); default descriptor flags (writable:false, configurable:
// false) match the WebIDL constant property attributes.
(function(constants) {
    for (var name in constants) {
        Object.defineProperty(EventSource, name, { value: constants[name], enumerable: true });
        Object.defineProperty(EventSource.prototype, name, { value: constants[name], enumerable: true });
    }
})({ CONNECTING: 0, OPEN: 1, CLOSED: 2 });

// ── IME Composition events (UI Events Specification §5.3) ─────────────────────
// Слушатели compositionstart/compositionupdate/compositionend:
// страница регистрирует их через addEventListener на нужном элементе.
// _lumen_dispatch_composition вызывается Rust-сторона после получения
// Ime::Preedit / Ime::Commit от winit. Диспатч идёт на document.activeElement
// (или document.body как fallback).
var _ime_active_element = null;

function _lumen_set_ime_target(el) {
    _ime_active_element = el || null;
}

function _lumen_dispatch_composition(type, data) {
    var target = _ime_active_element || (typeof document !== 'undefined' && document.body) || null;
    if (!target) return;
    var nid = target.__nid__;
    if (nid === undefined) return;
    var evt = new Event(type, { isTrusted: true });
    evt.data = String(data);
    evt.locale = '';
    _lumen_dispatch(nid, evt);
}

// ── Page lifecycle events: pageshow / pagehide (HTML Living Standard §8.6) ───
// _lumen_bfcache_persisted is set to true by an injected init script when the
// shell restores a page from bfcache. Pages can read event.persisted to detect
// this case and skip expensive re-initialisation.
var _lumen_bfcache_persisted = false;
var _pageshow_listeners = [];
var _pagehide_listeners = [];

function _lumen_fire_page_lifecycle(type, persisted) {
    var evt = new PageTransitionEvent(type, { isTrusted: true, persisted: !!persisted });
    if (type === 'pageshow') {
        // HTML LS §7.4.6 «reactivate a document»: the page becomes showing and
        // visible again BEFORE pageshow, so a listener reading
        // `document.visibilityState` sees 'visible'. A freshly loaded document
        // is already in that state, so both calls are no-ops there; the pair
        // matters for a page coming back out of bfcache in the SAME runtime
        // (BUG-835 parking), which `_lumen_unload_document` had hidden.
        _lumen_page_showing = true;
        _lumen_apply_visibility(false);
    }
    var listeners = type === 'pageshow' ? _pageshow_listeners : _pagehide_listeners;
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) { _lumen_report_exception(e); }
    }
    var handler = type === 'pageshow' ? window.onpageshow : window.onpagehide;
    if (typeof handler === 'function') {
        try { handler(evt); } catch(e) { _lumen_report_exception(e); }
    }
}

// ── Unloading a document (HTML LS §7.4.5–§7.4.6) ─────────────────────────────
// `_lumen_page_showing` mirrors the spec's «page showing» flag. It gates the
// pagehide/visibility half of the unload so a document cannot be hidden twice:
// the shell runs the sequence once per departure, and a page restored from
// bfcache flips the flag back on `pageshow`.
var _lumen_page_showing = true;

// «prompt to unload a document» (HTML LS §7.4.5). Returns true when the page
// asked to stay — `preventDefault()` on the event, or a non-empty
// `returnValue`, including the legacy «return a string from onbeforeunload»
// form (per the event handler processing algorithm, only the on<type> handler's
// return value counts; an addEventListener callback's does not).
// The shell only LOGS that answer. Honouring it means showing a confirm dialog,
// and this engine's `confirm()` is a stub that always answers false, so
// treating «asked to stay» as «cancel» would wedge every page that sets a
// returnValue with no way for the user to say «leave». See BUG-834.
function _lumen_fire_beforeunload() {
    var evt = new BeforeUnloadEvent('beforeunload', { cancelable: true, isTrusted: true });
    var arr = _other_win_listeners['beforeunload'];
    if (arr) {
        arr = arr.slice();
        for (var i = 0; i < arr.length; i++) {
            try { arr[i].call(window, evt); } catch(e) { _lumen_report_exception(e); }
        }
    }
    if (typeof window.onbeforeunload === 'function') {
        try {
            var rv = window.onbeforeunload.call(window, evt);
            if (rv !== undefined && rv !== null) evt.returnValue = String(rv);
        } catch(e) { _lumen_report_exception(e); }
    }
    return !!evt.defaultPrevented || String(evt.returnValue) !== '';
}

// «unload a document» (HTML LS §7.4.6). The order is fixed by the spec:
// pagehide → visibilityState 'hidden' → unload, and `unload` fires ONLY for a
// document that is not salvageable — i.e. one the shell could not retain
// (`persisted === false`). A page carrying an `unload`/`beforeunload` listener
// is denied the freeze by `_lumen_bfcache_blocked()` above, so the two halves
// agree: such a page always reaches the `unload` branch here.
function _lumen_unload_document(persisted) {
    if (_lumen_page_showing) {
        _lumen_page_showing = false;
        _lumen_fire_page_lifecycle('pagehide', persisted);
        _lumen_apply_visibility(true);
    }
    if (persisted) return;
    // `unload` has no on<type>-return-value convention, so the generic branch of
    // `window.dispatchEvent` already does exactly the right thing: listeners in
    // registration order, then `window.onunload`, each guarded by
    // `_lumen_report_exception`.
    window.dispatchEvent(new Event('unload', { isTrusted: true }));
}

// Whether the current page must be denied a full bfcache freeze (HTML Living
// Standard §8.6): an open WebSocket/EventSource connection, or a registered
// `unload`/`beforeunload` handler, would silently hang or never fire while the
// page sits frozen in the cache. `readyState === 1` is OPEN for both
// WebSocket and EventSource (see `_ws_instances`/`_sse_instances` below).
// `unload`/`beforeunload` have no dedicated `addEventListener` case, so
// listeners land in the generic `_other_win_listeners` bucket; `onunload`/
// `onbeforeunload` are plain assignable properties, checked directly.
// A live `Worker` (BUG-988) runs on its own OS thread that nothing pumps and
// nothing tells "the page went away" — `park_current_page` keeps the whole
// runtime, Worker included, alive indefinitely (evicted only once a *later*
// page also becomes park-eligible, which may never happen), so the thread
// keeps ticking its own timers/messages long after the page that created it
// is gone. `_lumen_has_active_worker` is defined in `WORKER_SHIM`
// (crates/js/src/worker.rs); guarded by `typeof` because the v8-backend
// feature (and thus Worker support) is optional.
// Called from the shell via `PersistentJs::has_bfcache_freeze_blocker`.
function _lumen_bfcache_blocked() {
    if (_ws_instances.some(function(w) { return w.readyState === 1; })) return true;
    if (_sse_instances.some(function(s) { return s.readyState === 1; })) return true;
    if (typeof window.onbeforeunload === 'function') return true;
    if (typeof window.onunload === 'function') return true;
    if (_other_win_listeners['beforeunload'] && _other_win_listeners['beforeunload'].length > 0) return true;
    if (_other_win_listeners['unload'] && _other_win_listeners['unload'].length > 0) return true;
    if (typeof _lumen_has_active_worker === 'function' && _lumen_has_active_worker()) return true;
    return false;
}

// ── Fetch API (Fetch Standard §3) ─────────────────────────────────────────────
// AbortController / AbortSignal. abort() records state and fires listeners;
// fetch() checks signal.aborted before issuing the (synchronous) request.
function AbortSignal() {
    this.aborted = false;
    this.reason = undefined;
    this.onabort = null;
    this._listeners = [];
}
AbortSignal.prototype.addEventListener = function(type, fn) {
    if (type === 'abort') this._listeners.push(fn);
};
AbortSignal.prototype.removeEventListener = function(type, fn) {
    if (type !== 'abort') return;
    var i = this._listeners.indexOf(fn);
    if (i >= 0) this._listeners.splice(i, 1);
};
AbortSignal.prototype.throwIfAborted = function() {
    if (this.aborted) throw this.reason || new DOMException('signal is aborted without reason', 'AbortError');
};
// Shared signal-abort steps (DOM §3.2): set state, fire onabort + listeners.
function _lumen_abort_signal_fire(sig, reason) {
    if (sig.aborted) return;
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    var evt = { type: 'abort', target: sig };
    if (typeof sig.onabort === 'function') { try { sig.onabort(evt); } catch(e) { _lumen_report_exception(e); } }
    var listeners = sig._listeners.slice();
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) { _lumen_report_exception(e); }
    }
}

function AbortController() {
    this.signal = new AbortSignal();
}
AbortController.prototype.abort = function(reason) {
    _lumen_abort_signal_fire(this.signal, reason);
};
// AbortSignal.abort(reason) — DOM §3.2.2: returns an already-aborted signal.
AbortSignal.abort = function(reason) {
    var sig = new AbortSignal();
    sig.aborted = true;
    sig.reason = reason !== undefined ? reason
               : new DOMException('signal is aborted without reason', 'AbortError');
    return sig;
};
// AbortSignal.timeout(ms) — DOM §3.2.2: aborts with TimeoutError after the
// shell timer queue (setTimeout shim) fires.
AbortSignal.timeout = function(ms) {
    var sig = new AbortSignal();
    // Recorded so fetch() can enforce the deadline natively: the JS thread is
    // parked inside the synchronous native fetch, so this setTimeout can never
    // fire mid-request — the native deadline thread does the in-flight abort.
    sig._timeoutMs = (typeof ms === 'number' && ms > 0) ? ms : 0;
    setTimeout(function() {
        _lumen_abort_signal_fire(sig, new DOMException('signal timed out', 'TimeoutError'));
    }, ms);
    return sig;
};
// AbortSignal.any(signals) — DOM §3.2.2: races the sources; the result aborts
// with the reason of the first source that aborts.
AbortSignal.any = function(signals) {
    var sig = new AbortSignal();
    var sources = [];
    function onAbort(evt) {
        if (sig.aborted) return;
        // Detach from remaining sources — the race is decided.
        for (var j = 0; j < sources.length; j++) {
            sources[j].removeEventListener('abort', onAbort);
        }
        _lumen_abort_signal_fire(sig, evt && evt.target ? evt.target.reason : undefined);
    }
    if (signals) {
        for (var i = 0; i < signals.length; i++) {
            if (!signals[i]) continue;
            if (signals[i].aborted) {
                sig.aborted = true;
                sig.reason = signals[i].reason;
                return sig;
            }
            sources.push(signals[i]);
            signals[i].addEventListener('abort', onAbort);
        }
    }
    return sig;
};

// ── WHATWG Streams (https://streams.spec.whatwg.org/) §3-5 ───────────────────
// ReadableStream, WritableStream, TransformStream.
//
// The readable side stays eager on purpose: `start()` runs synchronously and the
// first `pull()` follows it in the same turn, because fetch reads its body back
// out of the queue before the page ever sees the response (BUG-703). Everything
// else follows the spec state machine.
//
// BUG-823: the shim used to change a stream state and tell nobody — `writer.closed`
// was created and never settled, an errored controller reached only the read
// requests already standing, a promise returned from `start()` was dropped and the
// sink's `abort()` had no call site at all. The registry below is the fix: every
// promise the spec keeps a record for (the write requests, the close request, the
// abort request, `writer.ready`, `writer.closed`, `reader.closed`) is stored, so a
// transition to `errored`/`closed` settles all of them at once.

// A promise plus its settle functions and a state we can ask about — the spec
// asks «is writer.[[readyPromise]] pending?» in several places.
function _stream_deferred() {
    var d = { state: 'pending' };
    d.promise = new Promise(function(res, rej) { d._res = res; d._rej = rej; });
    d.resolve = function(v) { if (d.state !== 'pending') return; d.state = 'fulfilled'; d._res(v); };
    d.reject = function(e) { if (d.state !== 'pending') return; d.state = 'rejected'; d._rej(e); };
    return d;
}
// Spec «set promise.[[PromiseIsHandled]] to true»: a promise the stream rejects on
// the page's behalf must not surface as an unhandledrejection (BUG-716) just
// because the page never asked for it.
function _stream_mark_handled(p) {
    if (p && typeof p.then === 'function') p.then(undefined, function() {});
    return p;
}
function _stream_settled_deferred(promise, state) {
    return { state: state, promise: promise, resolve: function() {}, reject: function() {} };
}
function _stream_resolved_deferred() {
    return _stream_settled_deferred(Promise.resolve(undefined), 'fulfilled');
}
function _stream_rejected_deferred(e) {
    var d = _stream_settled_deferred(Promise.reject(e), 'rejected');
    _stream_mark_handled(d.promise);
    return d;
}
function _stream_is_thenable(v) {
    return v !== null && v !== undefined
        && (typeof v === 'object' || typeof v === 'function')
        && typeof v.then === 'function';
}

// ── ReadableStream §3 ────────────────────────────────────────────────────────
function ReadableStreamDefaultController(stream) {
    this._stream = stream;
    this._queue = [];
    this._closeRequested = false;
    this.desiredSize = 1;
}
ReadableStreamDefaultController.prototype.enqueue = function(chunk) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    if (stream._rs_reader && stream._rs_reader._readRequests.length > 0) {
        var req = stream._rs_reader._readRequests.shift();
        req({ value: chunk, done: false }, undefined);
    } else {
        this._queue.push(chunk);
    }
};
ReadableStreamDefaultController.prototype.close = function() {
    var stream = this._stream;
    if (!stream || this._closeRequested || stream._rs_state !== 'readable') return;
    this._closeRequested = true;
    if (this._queue.length === 0) _rs_do_close(stream);
};
ReadableStreamDefaultController.prototype.error = function(e) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    _rs_do_error(stream, e);
};

function _rs_do_close(stream) {
    stream._rs_state = 'closed';
    var reader = stream._rs_reader;
    if (!reader) return;
    var reqs = reader._readRequests;
    reader._readRequests = [];
    // A BYOB request answers «done» with an empty view over the caller's own
    // buffer, which its callback builds from the view it captured — so dropping
    // the parallel view list here is safe.
    if (reader._byobViews) reader._byobViews = [];
    for (var i = 0; i < reqs.length; i++) reqs[i]({ value: undefined, done: true }, undefined);
    reader._closedD.resolve(undefined);
}

// Streams §3.9 «ReadableStreamError»: the stored error goes to the standing read
// requests *and* to `reader.closed`, which is what BUG-823 never did.
function _rs_do_error(stream, e) {
    stream._rs_state = 'errored';
    stream._rs_error = e;
    stream._rs_ctrl._queue = [];
    var reader = stream._rs_reader;
    if (!reader) return;
    var reqs = reader._readRequests;
    reader._readRequests = [];
    if (reader._byobViews) reader._byobViews = [];
    for (var i = 0; i < reqs.length; i++) reqs[i](undefined, e);
    reader._closedD.reject(e);
    _stream_mark_handled(reader._closedD.promise);
}

// Demand-driven pull (Streams §3.10 «ReadableStreamDefaultControllerCallPullIfNeeded»):
// pull once per unit of demand, never re-entrantly, and let a rejected pull error
// the stream instead of vanishing.
function _rs_pull_if_needed(stream) {
    if (!stream._rs_started || !stream._rs_pull_fn) return;
    if (stream._rs_state !== 'readable') return;
    var ctrl = stream._rs_ctrl;
    if (ctrl._closeRequested) return;
    var standing = stream._rs_reader ? stream._rs_reader._readRequests.length : 0;
    if (ctrl._queue.length > 0 && standing === 0) return;
    if (stream._rs_pulling) { stream._rs_pullAgain = true; return; }
    stream._rs_pulling = true;
    var result;
    try {
        result = stream._rs_pull_fn(ctrl);
    } catch (e) {
        stream._rs_pulling = false;
        if (stream._rs_state === 'readable') _rs_do_error(stream, e);
        return;
    }
    if (_stream_is_thenable(result)) {
        Promise.resolve(result).then(function() {
            stream._rs_pulling = false;
            if (!stream._rs_pullAgain) return;
            stream._rs_pullAgain = false;
            _rs_pull_if_needed(stream);
        }, function(e) {
            stream._rs_pulling = false;
            if (stream._rs_state === 'readable') _rs_do_error(stream, e);
        });
        return;
    }
    stream._rs_pulling = false;
    if (stream._rs_pullAgain) { stream._rs_pullAgain = false; _rs_pull_if_needed(stream); }
}

function ReadableStream(source, strategy) {
    source = source || {};
    // §3.2.3 step 2: `type` picks the controller. BUG-824: it used to be ignored,
    // so a byte stream was silently an ordinary one and `{mode:'byob'}` degraded
    // to a default reader instead of erroring.
    if (source.type !== undefined && String(source.type) !== 'bytes') {
        throw new TypeError('ReadableStream: invalid underlying source type ' + source.type);
    }
    var isBytes = source.type !== undefined;
    var autoAlloc = source.autoAllocateChunkSize;
    if (isBytes && autoAlloc !== undefined && !(Number(autoAlloc) > 0)) {
        throw new TypeError('ReadableStream: autoAllocateChunkSize must be greater than 0');
    }
    this._rs_state = 'readable';
    this._rs_error = undefined;
    this._rs_reader = null;
    this._rs_bytes = isBytes;
    this._rs_cancel_fn = typeof source.cancel === 'function' ? source.cancel : null;
    this._rs_pull_fn = typeof source.pull === 'function' ? source.pull : null;
    this._rs_ctrl = isBytes
        ? new ReadableByteStreamController(this, autoAlloc === undefined ? 0 : Number(autoAlloc))
        : new ReadableStreamDefaultController(this);
    this._rs_started = false;
    this._rs_pulling = false;
    this._rs_pullAgain = false;
    var self = this;
    var startResult;
    if (typeof source.start === 'function') {
        try {
            startResult = source.start(this._rs_ctrl);
        } catch (e) {
            this._rs_started = true;
            this._rs_ctrl.error(e);
            return;
        }
    }
    // A thenable from start() holds the stream back until it settles, and its
    // rejection errors the stream (BUG-823: the value used to be discarded).
    if (_stream_is_thenable(startResult)) {
        Promise.resolve(startResult).then(function() {
            self._rs_started = true;
            _rs_pull_if_needed(self);
        }, function(e) {
            self._rs_started = true;
            if (self._rs_state === 'readable') _rs_do_error(self, e);
        });
        return;
    }
    this._rs_started = true;
    // Eager fill: fetch drains this queue synchronously (BUG-703).
    _rs_pull_if_needed(this);
}
Object.defineProperty(ReadableStream.prototype, 'locked', {
    get: function() { return this._rs_reader !== null; }
});
ReadableStream.prototype.getReader = function(options) {
    var mode = (options === undefined || options === null) ? undefined : Object(options).mode;
    if (mode !== undefined && String(mode) !== 'byob') {
        throw new TypeError('ReadableStream.getReader: invalid mode ' + mode);
    }
    if (mode !== undefined && !this._rs_bytes) {
        throw new TypeError('ReadableStream.getReader: mode byob requires a byte stream');
    }
    if (this._rs_reader !== null) throw new TypeError('ReadableStream is already locked');
    var reader = mode === undefined
        ? new ReadableStreamDefaultReader(this)
        : new ReadableStreamBYOBReader(this);
    this._rs_reader = reader;
    return reader;
};
ReadableStream.prototype.cancel = function(reason) {
    if (this._rs_reader) return Promise.reject(new TypeError('ReadableStream is locked'));
    return this._rs_do_cancel(reason);
};
ReadableStream.prototype._rs_do_cancel = function(reason) {
    if (this._rs_state === 'closed') return Promise.resolve(undefined);
    if (this._rs_state === 'errored') return Promise.reject(this._rs_error);
    this._rs_ctrl._queue = [];
    _rs_do_close(this);
    if (!this._rs_cancel_fn) return Promise.resolve(undefined);
    // §3.9 «ReadableStreamCancel»: the promise the page gets is the source's own
    // cancel() result, so a throwing or rejecting source is visible to it.
    var result;
    try {
        result = this._rs_cancel_fn(reason);
    } catch (e) {
        return Promise.reject(e);
    }
    return Promise.resolve(result).then(function() { return undefined; });
};
// Streams §3.2.6 «ReadableStreamTee». BUG-824: this used to copy the controller's
// *current* queue into two independent stubs and close the source — so the source
// reported `locked === false`, and everything it enqueued after the call went
// nowhere. Both branches now share one reader on a source that stays locked and
// readable, which is what makes `tee()` usable for its canonical purpose (reading
// a response body twice).
ReadableStream.prototype.tee = function() {
    var reader = this.getReader();
    var reading = false, readAgain = false;
    var canceled1 = false, canceled2 = false, reason1, reason2;
    var ctrl1 = null, ctrl2 = null;
    var cancelD = _stream_deferred();
    _stream_mark_handled(cancelD.promise);
    function pullAlgorithm() {
        // One read in flight for both branches: the second branch's demand is
        // remembered rather than issuing a competing read.
        if (reading) { readAgain = true; return Promise.resolve(undefined); }
        reading = true;
        return reader.read().then(function(res) {
            reading = false;
            if (res.done) {
                if (!canceled1 && ctrl1) ctrl1.close();
                if (!canceled2 && ctrl2) ctrl2.close();
                return undefined;
            }
            if (!canceled1 && ctrl1) ctrl1.enqueue(res.value);
            if (!canceled2 && ctrl2) ctrl2.enqueue(res.value);
            if (!readAgain) return undefined;
            readAgain = false;
            return pullAlgorithm();
        }, function(e) {
            reading = false;
            if (ctrl1) ctrl1.error(e);
            if (ctrl2) ctrl2.error(e);
            return undefined;
        });
    }
    // §3.2.6 step 13: the source is cancelled only once *both* branches are, and
    // with the two reasons aggregated — «canceling both branches should aggregate
    // the cancel reasons» is the first subtest this used to hang on.
    function finishCancel() {
        cancelD.resolve(reader.cancel([reason1, reason2]));
    }
    function cancel1(reason) {
        canceled1 = true;
        reason1 = reason;
        if (canceled2) finishCancel();
        return cancelD.promise;
    }
    function cancel2(reason) {
        canceled2 = true;
        reason2 = reason;
        if (canceled1) finishCancel();
        return cancelD.promise;
    }
    var branch1 = new ReadableStream({
        start: function(c) { ctrl1 = c; },
        pull: pullAlgorithm,
        cancel: cancel1
    });
    var branch2 = new ReadableStream({
        start: function(c) { ctrl2 = c; },
        pull: pullAlgorithm,
        cancel: cancel2
    });
    return [branch1, branch2];
};
// Streams §3.2.6 «ReadableStream.prototype.values»/[@@asyncIterator] — the form
// most modern code reads a stream in. Missing entirely before BUG-824, so
// `for await (const c of stream)` threw «not async iterable».
ReadableStream.prototype.values = function(options) {
    var preventCancel = !!(options !== undefined && options !== null && Object(options).preventCancel);
    var reader = this.getReader();
    var iter = {};
    iter.next = function() {
        return reader.read().then(function(res) {
            if (!res.done) return { value: res.value, done: false };
            // §3.2.6: the lock is released on completion, not kept for good.
            try { reader.releaseLock(); } catch (e) {}
            return { value: undefined, done: true };
        }, function(e) {
            try { reader.releaseLock(); } catch (x) {}
            throw e;
        });
    };
    iter['return'] = function(value) {
        if (preventCancel) {
            try { reader.releaseLock(); } catch (e) {}
            return Promise.resolve({ value: value, done: true });
        }
        var p = reader.cancel(value);
        try { reader.releaseLock(); } catch (e) {}
        return p.then(function() { return { value: value, done: true }; });
    };
    iter[Symbol.asyncIterator] = function() { return this; };
    return iter;
};
Object.defineProperty(ReadableStream.prototype, Symbol.asyncIterator, {
    value: ReadableStream.prototype.values, writable: true, configurable: true
});
ReadableStream.prototype.pipeTo = function(dest, options) {
    options = options || {};
    var preventClose = !!options.preventClose;
    var preventAbort = !!options.preventAbort;
    var preventCancel = !!options.preventCancel;
    var reader, writer;
    try {
        reader = this.getReader();
        writer = dest.getWriter();
    } catch (e) {
        return Promise.reject(e);
    }
    function pump() {
        return writer.ready.then(function() {
            return reader.read();
        }).then(function(result) {
            if (result.done) {
                reader.releaseLock();
                if (preventClose) { writer.releaseLock(); return undefined; }
                return writer.close();
            }
            return writer.write(result.value).then(pump);
        });
    }
    // Both ends are torn down before the pipe promise rejects — otherwise the
    // failure leaves a locked stream and an unsettled `closed` behind.
    return pump().then(undefined, function(e) {
        if (!preventCancel) {
            try { _stream_mark_handled(reader.cancel(e)); } catch (x) {}
        }
        if (!preventAbort) {
            try { _stream_mark_handled(writer.abort(e)); } catch (x) {}
        }
        return Promise.reject(e);
    });
};
ReadableStream.prototype.pipeThrough = function(transform, options) {
    _stream_mark_handled(this.pipeTo(transform.writable, options));
    return transform.readable;
};
ReadableStream.from = function(iterable) {
    var arr = Array.isArray(iterable) ? iterable : (iterable instanceof Uint8Array ? [iterable] : []);
    return new ReadableStream({
        start: function(c) {
            for (var i = 0; i < arr.length; i++) c.enqueue(arr[i]);
            c.close();
        }
    });
};

// ── ReadableStreamDefaultReader §3.7 ─────────────────────────────────────────
function ReadableStreamDefaultReader(stream) {
    this._stream = stream;
    this._readRequests = [];
    this._closedD = _stream_deferred();
    if (stream._rs_state === 'closed') this._closedD.resolve(undefined);
    else if (stream._rs_state === 'errored') {
        this._closedD.reject(stream._rs_error);
        _stream_mark_handled(this._closedD.promise);
    }
}
// `closed`/`cancel`/`releaseLock` are identical for both reader flavours (§3.7,
// §3.8 differ only in `read`), so they are installed from one place rather than
// written out twice.
function _rs_install_reader_common(proto) {
    Object.defineProperty(proto, 'closed', {
        get: function() { return this._closedD.promise; }
    });
    proto.cancel = function(reason) {
        var stream = this._stream;
        if (!stream) return Promise.reject(new TypeError('reader not attached'));
        return stream._rs_do_cancel(reason);
    };
    proto.releaseLock = function() {
        if (!this._stream) return;
        if (this._readRequests.length > 0) throw new TypeError('pending read requests');
        this._stream._rs_reader = null;
        this._stream = null;
        this._closedD.reject(new TypeError('reader released'));
        _stream_mark_handled(this._closedD.promise);
    };
}
_rs_install_reader_common(ReadableStreamDefaultReader.prototype);
ReadableStreamDefaultReader.prototype.read = function() {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('reader not attached to a stream'));
    if (stream._rs_state === 'errored') return Promise.reject(stream._rs_error);
    var ctrl = stream._rs_ctrl;
    if (ctrl._queue.length > 0) {
        var chunk = ctrl._queue.shift();
        if (ctrl._closeRequested && ctrl._queue.length === 0) _rs_do_close(stream);
        else _rs_pull_if_needed(stream);
        return Promise.resolve({ value: chunk, done: false });
    }
    if (stream._rs_state === 'closed') return Promise.resolve({ value: undefined, done: true });
    var self = this;
    var p = new Promise(function(resolve, reject) {
        self._readRequests.push(function(result, err) {
            if (err !== undefined) reject(err); else resolve(result);
        });
    });
    _rs_pull_if_needed(stream);
    return p;
};

// ── ReadableByteStreamController §3.11 / BYOB reading §3.8, §3.9 ─────────────
// A byte stream queues `Uint8Array`s and can answer a read straight into the
// caller's own buffer. BUG-824: none of this existed — `type: 'bytes'` was
// ignored and `getReader({mode:'byob'})` handed back a default reader, i.e. a
// silent change of semantics rather than an error.
//
// One deliberate deviation: the spec transfers (detaches) the caller's buffer
// and hands back a view over the transferred copy. `ArrayBuffer.prototype.transfer`
// is not wired in this engine, so the same buffer is reused — a page that keeps
// its own reference to the pre-read view still sees the bytes, where a spec
// browser would have detached it.
function ReadableByteStreamController(stream, autoAllocateChunkSize) {
    this._stream = stream;
    this._queue = [];
    this._closeRequested = false;
    this._autoAllocate = autoAllocateChunkSize || 0;
    this._byobRequest = null;
    this._autoView = null;
    this.desiredSize = 1;
}
Object.defineProperty(ReadableByteStreamController.prototype, 'byobRequest', {
    get: function() { return _rbs_byob_request(this); }
});
ReadableByteStreamController.prototype.enqueue = function(chunk) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    if (!ArrayBuffer.isView(chunk)) {
        throw new TypeError('ReadableByteStreamController.enqueue expects an ArrayBufferView');
    }
    if (chunk.byteLength > 0) {
        this._queue.push(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
    }
    this._byobRequest = null;
    _rbs_drain(this);
};
ReadableByteStreamController.prototype.close = function() {
    var stream = this._stream;
    if (!stream || this._closeRequested || stream._rs_state !== 'readable') return;
    this._closeRequested = true;
    if (this._queue.length === 0) _rs_do_close(stream);
};
ReadableByteStreamController.prototype.error = function(e) {
    var stream = this._stream;
    if (!stream || stream._rs_state !== 'readable') return;
    _rs_do_error(stream, e);
};

// A view of the caller's own class over `byteLength` bytes of its buffer — a
// BYOB read must give back the same kind of view it was handed.
function _rbs_same_kind(view, byteLength) {
    var Ctor = view.constructor;
    var per = view.BYTES_PER_ELEMENT || 1;
    return new Ctor(view.buffer, view.byteOffset, Math.floor(byteLength / per));
}
// Copy as much of the queue as fits into `view`; a partially consumed head chunk
// stays at the front. §3.11 responds as soon as one element is available rather
// than waiting for the view to fill.
function _rbs_fill(ctrl, view) {
    var dest = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    var written = 0;
    while (written < dest.length && ctrl._queue.length > 0) {
        var head = ctrl._queue[0];
        var n = Math.min(head.length, dest.length - written);
        dest.set(head.subarray(0, n), written);
        written += n;
        if (n === head.length) ctrl._queue.shift();
        else ctrl._queue[0] = head.subarray(n);
    }
    return _rbs_same_kind(view, written);
}
// Hand queued bytes to the standing read requests, whichever reader flavour holds
// the stream.
function _rbs_drain(ctrl) {
    var stream = ctrl._stream;
    var reader = stream && stream._rs_reader;
    if (!reader) return;
    while (reader._readRequests.length > 0 && ctrl._queue.length > 0) {
        var req = reader._readRequests.shift();
        if (reader._byobViews) {
            req({ value: _rbs_fill(ctrl, reader._byobViews.shift()), done: false }, undefined);
        } else {
            req({ value: ctrl._queue.shift(), done: false }, undefined);
        }
    }
    if (stream._rs_state !== 'readable') return;
    if (ctrl._closeRequested) {
        if (ctrl._queue.length === 0) _rs_do_close(stream);
        return;
    }
    if (reader._readRequests.length > 0) _rs_pull_if_needed(stream);
}
// §3.11 `byobRequest`: the buffer the source is invited to write into — either the
// pending BYOB reader's own view, or one allocated from `autoAllocateChunkSize`
// when a default reader is waiting.
function _rbs_byob_request(ctrl) {
    if (ctrl._byobRequest) return ctrl._byobRequest;
    var stream = ctrl._stream;
    var reader = stream && stream._rs_reader;
    if (!reader || ctrl._queue.length > 0) return null;
    var view = null;
    if (reader._byobViews) {
        if (reader._byobViews.length > 0) view = reader._byobViews[0];
    } else if (ctrl._autoAllocate > 0 && reader._readRequests.length > 0) {
        if (!ctrl._autoView) ctrl._autoView = new Uint8Array(ctrl._autoAllocate);
        view = ctrl._autoView;
    }
    if (!view) return null;
    ctrl._byobRequest = new ReadableStreamBYOBRequest(ctrl, view);
    return ctrl._byobRequest;
}
function _rbs_respond(request, bytes) {
    var ctrl = request._ctrl;
    if (!ctrl || ctrl._byobRequest !== request) {
        throw new TypeError('This BYOB request has already been responded to');
    }
    ctrl._byobRequest = null;
    ctrl._autoView = null;
    // The bytes may already live in the pending view's own buffer; queueing them
    // and draining keeps one delivery path for both cases (the copy back into the
    // same range is a no-op).
    if (bytes.byteLength > 0) ctrl._queue.push(bytes);
    _rbs_drain(ctrl);
    if (bytes.byteLength === 0 && ctrl._closeRequested && ctrl._stream
        && ctrl._stream._rs_state === 'readable') {
        _rs_do_close(ctrl._stream);
    }
}

// ── ReadableStreamBYOBRequest §3.10 ─────────────────────────────────────────
function ReadableStreamBYOBRequest(ctrl, view) {
    this._ctrl = ctrl;
    this._view = view;
}
Object.defineProperty(ReadableStreamBYOBRequest.prototype, 'view', {
    get: function() { return this._view; }
});
ReadableStreamBYOBRequest.prototype.respond = function(bytesWritten) {
    var n = Number(bytesWritten);
    if (!(n >= 0)) throw new TypeError('respond expects a non-negative byte count');
    if (n > this._view.byteLength) throw new RangeError('respond: more bytes written than the view holds');
    _rbs_respond(this, new Uint8Array(this._view.buffer, this._view.byteOffset, n));
};
ReadableStreamBYOBRequest.prototype.respondWithNewView = function(view) {
    if (!ArrayBuffer.isView(view)) throw new TypeError('respondWithNewView expects an ArrayBufferView');
    _rbs_respond(this, new Uint8Array(view.buffer, view.byteOffset, view.byteLength));
};

// ── ReadableStreamBYOBReader §3.8 ───────────────────────────────────────────
function ReadableStreamBYOBReader(stream) {
    this._stream = stream;
    this._readRequests = [];
    // Parallel to _readRequests: the view each pending read is to be filled into.
    // Its presence is also what marks this reader as BYOB for the controller.
    this._byobViews = [];
    this._closedD = _stream_deferred();
    if (stream._rs_state === 'closed') this._closedD.resolve(undefined);
    else if (stream._rs_state === 'errored') {
        this._closedD.reject(stream._rs_error);
        _stream_mark_handled(this._closedD.promise);
    }
}
_rs_install_reader_common(ReadableStreamBYOBReader.prototype);
ReadableStreamBYOBReader.prototype.read = function(view) {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('reader not attached to a stream'));
    if (!ArrayBuffer.isView(view)) {
        return Promise.reject(new TypeError('BYOB read expects an ArrayBufferView'));
    }
    if (view.byteLength === 0) {
        return Promise.reject(new TypeError('BYOB read expects a view of non-zero length'));
    }
    if (stream._rs_state === 'errored') return Promise.reject(stream._rs_error);
    var ctrl = stream._rs_ctrl;
    if (ctrl._queue.length > 0) {
        var filled = _rbs_fill(ctrl, view);
        if (ctrl._closeRequested && ctrl._queue.length === 0) _rs_do_close(stream);
        else _rs_pull_if_needed(stream);
        return Promise.resolve({ value: filled, done: false });
    }
    if (stream._rs_state === 'closed') {
        return Promise.resolve({ value: _rbs_same_kind(view, 0), done: true });
    }
    var self = this;
    var p = new Promise(function(resolve, reject) {
        self._readRequests.push(function(result, err) {
            if (err !== undefined) { reject(err); return; }
            if (result.done) { resolve({ value: _rbs_same_kind(view, 0), done: true }); return; }
            resolve(result);
        });
        self._byobViews.push(view);
    });
    _rs_pull_if_needed(stream);
    return p;
};

// ── WritableStream §4 ────────────────────────────────────────────────────────
// State machine per spec: 'writable' | 'erroring' | 'errored' | 'closed'. There is
// no 'closing' state — a pending close lives in `_ws_closeRequest`/`_ws_inFlightClose`,
// which is what lets an error arriving mid-close reject the right promise.
var _WS_CLOSE_SENTINEL = { closeSentinel: true };

function WritableStreamDefaultController(stream, sink, hwm, sizeFn) {
    this._stream = stream;
    this._sink = sink;
    this._queue = [];
    this._queueTotalSize = 0;
    this._started = false;
    this._hwm = hwm;
    this._sizeFn = sizeFn;
    this._writeFn = typeof sink.write === 'function' ? sink.write : null;
    this._closeFn = typeof sink.close === 'function' ? sink.close : null;
    this._abortFn = typeof sink.abort === 'function' ? sink.abort : null;
    var ac = (typeof AbortController === 'function') ? new AbortController() : null;
    this._abortController = ac;
    this.signal = ac ? ac.signal : undefined;
}
WritableStreamDefaultController.prototype.error = function(e) {
    var stream = this._stream;
    if (!stream || stream._ws_state !== 'writable') return;
    _ws_ctrl_error(this, e);
};

// §4.8.3 «ClearAlgorithms»: once the stream is going down, the sink is never
// called again — a dropped reference here is what stops a doomed stream from
// re-entering the page's code.
function _ws_ctrl_clear(controller) {
    controller._writeFn = null;
    controller._closeFn = null;
    controller._abortFn = null;
    controller._sizeFn = null;
}
function _ws_ctrl_desired_size(controller) { return controller._hwm - controller._queueTotalSize; }
function _ws_ctrl_backpressure(controller) { return _ws_ctrl_desired_size(controller) <= 0; }
function _ws_ctrl_reset_queue(controller) { controller._queue = []; controller._queueTotalSize = 0; }
function _ws_ctrl_enqueue(controller, chunk, size) {
    controller._queue.push({ chunk: chunk, size: size });
    controller._queueTotalSize += size;
}
function _ws_ctrl_dequeue(controller) {
    var entry = controller._queue.shift();
    if (!entry) return undefined;
    controller._queueTotalSize -= entry.size;
    if (controller._queueTotalSize < 0) controller._queueTotalSize = 0;
    return entry.chunk;
}
function _ws_ctrl_error(controller, e) {
    _ws_ctrl_clear(controller);
    _ws_start_erroring(controller._stream, e);
}
function _ws_ctrl_error_if_needed(controller, e) {
    if (controller._stream && controller._stream._ws_state === 'writable') _ws_ctrl_error(controller, e);
}

function _ws_ctrl_setup(controller) {
    var stream = controller._stream;
    _ws_update_backpressure(stream, _ws_ctrl_backpressure(controller));
    var startResult;
    try {
        startResult = controller._sink.start ? controller._sink.start.call(controller._sink, controller) : undefined;
    } catch (e) {
        controller._started = true;
        _ws_deal_with_rejection(stream, e);
        return;
    }
    // §4.8.3: the start result is always awaited, so a sink that returns a thenable
    // holds its own queue back until it settles, and a rejection errors the stream.
    Promise.resolve(startResult).then(function() {
        controller._started = true;
        _ws_ctrl_advance(controller);
    }, function(r) {
        controller._started = true;
        _ws_deal_with_rejection(stream, r);
    });
}

function _ws_ctrl_advance(controller) {
    var stream = controller._stream;
    if (!controller._started) return;
    if (stream._ws_inFlightWrite !== null) return;
    if (stream._ws_state === 'erroring') { _ws_finish_erroring(stream); return; }
    if (controller._queue.length === 0) return;
    if (controller._queue[0].chunk === _WS_CLOSE_SENTINEL) _ws_ctrl_process_close(controller);
    else _ws_ctrl_process_write(controller, controller._queue[0].chunk);
}
function _ws_ctrl_process_close(controller) {
    var stream = controller._stream;
    stream._ws_inFlightClose = stream._ws_closeRequest;
    stream._ws_closeRequest = null;
    _ws_ctrl_dequeue(controller);
    var closeFn = controller._closeFn;
    var p;
    try {
        p = closeFn ? Promise.resolve(closeFn.call(controller._sink, controller)) : Promise.resolve(undefined);
    } catch (e) {
        p = Promise.reject(e);
    }
    _ws_ctrl_clear(controller);
    p.then(function() {
        _ws_finish_in_flight_close(stream);
    }, function(reason) {
        _ws_finish_in_flight_close_with_error(stream, reason);
    });
}
function _ws_ctrl_process_write(controller, chunk) {
    var stream = controller._stream;
    stream._ws_inFlightWrite = stream._ws_writeRequests.shift();
    var writeFn = controller._writeFn;
    var p;
    try {
        p = writeFn ? Promise.resolve(writeFn.call(controller._sink, chunk, controller)) : Promise.resolve(undefined);
    } catch (e) {
        p = Promise.reject(e);
    }
    p.then(function() {
        _ws_finish_in_flight_write(stream);
        var state = stream._ws_state;
        _ws_ctrl_dequeue(controller);
        if (!_ws_close_queued_or_in_flight(stream) && state === 'writable') {
            _ws_update_backpressure(stream, _ws_ctrl_backpressure(controller));
        }
        _ws_ctrl_advance(controller);
    }, function(reason) {
        if (stream._ws_state === 'writable') _ws_ctrl_clear(controller);
        _ws_finish_in_flight_write_with_error(stream, reason);
    });
}

function _ws_close_queued_or_in_flight(stream) {
    return stream._ws_closeRequest !== null || stream._ws_inFlightClose !== null;
}
function _ws_has_operation_in_flight(stream) {
    return stream._ws_inFlightWrite !== null || stream._ws_inFlightClose !== null;
}
function _ws_add_write_request(stream) {
    var d = _stream_deferred();
    stream._ws_writeRequests.push(d);
    return d.promise;
}
function _ws_deal_with_rejection(stream, error) {
    if (stream._ws_state === 'writable') { _ws_start_erroring(stream, error); return; }
    _ws_finish_erroring(stream);
}
function _ws_start_erroring(stream, reason) {
    if (stream._ws_state !== 'writable') return;
    stream._ws_error = reason;
    stream._ws_state = 'erroring';
    var writer = stream._ws_writer;
    if (writer) _ws_ensure_ready_rejected(writer, reason);
    if (!_ws_has_operation_in_flight(stream) && stream._ws_ctrl._started) _ws_finish_erroring(stream);
}
// §4.4 «WritableStreamFinishErroring» — the broadcast BUG-823 was missing: every
// standing write request, the close request, `writer.closed` and the abort request
// are settled here, in that order.
function _ws_finish_erroring(stream) {
    if (stream._ws_state !== 'erroring') return;
    stream._ws_state = 'errored';
    _ws_ctrl_reset_queue(stream._ws_ctrl);
    var storedError = stream._ws_error;
    var reqs = stream._ws_writeRequests;
    stream._ws_writeRequests = [];
    for (var i = 0; i < reqs.length; i++) reqs[i].reject(storedError);
    var abortRequest = stream._ws_pendingAbort;
    if (abortRequest === null) { _ws_reject_close_and_closed_if_needed(stream); return; }
    stream._ws_pendingAbort = null;
    if (abortRequest.wasAlreadyErroring) {
        abortRequest.deferred.reject(storedError);
        _ws_reject_close_and_closed_if_needed(stream);
        return;
    }
    var controller = stream._ws_ctrl;
    var abortFn = controller._abortFn;
    var p;
    try {
        p = abortFn ? Promise.resolve(abortFn.call(controller._sink, abortRequest.reason)) : Promise.resolve(undefined);
    } catch (e) {
        p = Promise.reject(e);
    }
    _ws_ctrl_clear(controller);
    p.then(function() {
        abortRequest.deferred.resolve(undefined);
        _ws_reject_close_and_closed_if_needed(stream);
    }, function(reason) {
        abortRequest.deferred.reject(reason);
        _ws_reject_close_and_closed_if_needed(stream);
    });
}
function _ws_reject_close_and_closed_if_needed(stream) {
    var storedError = stream._ws_error;
    if (stream._ws_closeRequest !== null) {
        stream._ws_closeRequest.reject(storedError);
        stream._ws_closeRequest = null;
    }
    var writer = stream._ws_writer;
    if (writer) _ws_ensure_closed_rejected(writer, storedError);
}
function _ws_finish_in_flight_write(stream) {
    stream._ws_inFlightWrite.resolve(undefined);
    stream._ws_inFlightWrite = null;
}
function _ws_finish_in_flight_write_with_error(stream, error) {
    stream._ws_inFlightWrite.reject(error);
    stream._ws_inFlightWrite = null;
    _ws_deal_with_rejection(stream, error);
}
function _ws_finish_in_flight_close(stream) {
    stream._ws_inFlightClose.resolve(undefined);
    stream._ws_inFlightClose = null;
    if (stream._ws_state === 'erroring') {
        // A close that made it through outranks the pending error (§4.4).
        stream._ws_error = undefined;
        if (stream._ws_pendingAbort !== null) {
            stream._ws_pendingAbort.deferred.resolve(undefined);
            stream._ws_pendingAbort = null;
        }
    }
    stream._ws_state = 'closed';
    var writer = stream._ws_writer;
    if (writer) writer._closedD.resolve(undefined);
}
function _ws_finish_in_flight_close_with_error(stream, error) {
    stream._ws_inFlightClose.reject(error);
    stream._ws_inFlightClose = null;
    if (stream._ws_pendingAbort !== null) {
        stream._ws_pendingAbort.deferred.reject(error);
        stream._ws_pendingAbort = null;
    }
    _ws_deal_with_rejection(stream, error);
}
function _ws_update_backpressure(stream, backpressure) {
    var writer = stream._ws_writer;
    if (writer && backpressure !== stream._ws_backpressure) {
        if (backpressure) writer._readyD = _stream_deferred();
        else writer._readyD.resolve(undefined);
    }
    stream._ws_backpressure = backpressure;
}
function _ws_ensure_ready_rejected(writer, error) {
    if (writer._readyD.state === 'pending') writer._readyD.reject(error);
    else writer._readyD = _stream_rejected_deferred(error);
    _stream_mark_handled(writer._readyD.promise);
}
function _ws_ensure_closed_rejected(writer, error) {
    if (writer._closedD.state === 'pending') writer._closedD.reject(error);
    else writer._closedD = _stream_rejected_deferred(error);
    _stream_mark_handled(writer._closedD.promise);
}
function _ws_abort(stream, reason) {
    if (stream._ws_state === 'closed' || stream._ws_state === 'errored') return Promise.resolve(undefined);
    if (stream._ws_ctrl._abortController) {
        try { stream._ws_ctrl._abortController.abort(reason); } catch (e) {}
    }
    var state = stream._ws_state;
    if (state === 'closed' || state === 'errored') return Promise.resolve(undefined);
    if (stream._ws_pendingAbort !== null) return stream._ws_pendingAbort.deferred.promise;
    var wasAlreadyErroring = false;
    if (state === 'erroring') { wasAlreadyErroring = true; reason = undefined; }
    var d = _stream_deferred();
    stream._ws_pendingAbort = { deferred: d, reason: reason, wasAlreadyErroring: wasAlreadyErroring };
    if (!wasAlreadyErroring) _ws_start_erroring(stream, reason);
    return d.promise;
}
function _ws_close(stream) {
    var state = stream._ws_state;
    if (state === 'closed' || state === 'errored') {
        return Promise.reject(new TypeError('cannot close a stream that is already ' + state));
    }
    var d = _stream_deferred();
    stream._ws_closeRequest = d;
    var writer = stream._ws_writer;
    if (writer && stream._ws_backpressure && state === 'writable') writer._readyD.resolve(undefined);
    _ws_ctrl_enqueue(stream._ws_ctrl, _WS_CLOSE_SENTINEL, 0);
    _ws_ctrl_advance(stream._ws_ctrl);
    return d.promise;
}

function WritableStream(sink, strategy) {
    sink = sink || {};
    strategy = strategy || {};
    this._ws_state = 'writable';
    this._ws_error = undefined;
    this._ws_writer = null;
    this._ws_writeRequests = [];
    this._ws_inFlightWrite = null;
    this._ws_closeRequest = null;
    this._ws_inFlightClose = null;
    this._ws_pendingAbort = null;
    this._ws_backpressure = false;
    var hwm = strategy.highWaterMark === undefined ? 1 : Number(strategy.highWaterMark);
    if (hwm !== hwm || hwm < 0) throw new RangeError('invalid highWaterMark');
    var sizeFn = typeof strategy.size === 'function' ? strategy.size : null;
    this._ws_ctrl = new WritableStreamDefaultController(this, sink, hwm, sizeFn);
    _ws_ctrl_setup(this._ws_ctrl);
}
Object.defineProperty(WritableStream.prototype, 'locked', {
    get: function() { return this._ws_writer !== null; }
});
WritableStream.prototype.getWriter = function() {
    return new WritableStreamDefaultWriter(this);
};
WritableStream.prototype.abort = function(reason) {
    if (this._ws_writer) return Promise.reject(new TypeError('WritableStream is locked'));
    return _ws_abort(this, reason);
};
WritableStream.prototype.close = function() {
    if (this._ws_writer) return Promise.reject(new TypeError('WritableStream is locked'));
    if (_ws_close_queued_or_in_flight(this)) return Promise.reject(new TypeError('close already requested'));
    return _ws_close(this);
};

// ── WritableStreamDefaultWriter §4.6 ─────────────────────────────────────────
function WritableStreamDefaultWriter(stream) {
    if (!stream || typeof stream._ws_state !== 'string') {
        throw new TypeError('WritableStreamDefaultWriter requires a WritableStream');
    }
    if (stream._ws_writer !== null) throw new TypeError('WritableStream is already locked');
    this._stream = stream;
    stream._ws_writer = this;
    var state = stream._ws_state;
    if (state === 'writable') {
        this._readyD = (!_ws_close_queued_or_in_flight(stream) && stream._ws_backpressure)
            ? _stream_deferred() : _stream_resolved_deferred();
        this._closedD = _stream_deferred();
    } else if (state === 'erroring') {
        this._readyD = _stream_rejected_deferred(stream._ws_error);
        this._closedD = _stream_deferred();
    } else if (state === 'closed') {
        this._readyD = _stream_resolved_deferred();
        this._closedD = _stream_resolved_deferred();
    } else {
        this._readyD = _stream_rejected_deferred(stream._ws_error);
        this._closedD = _stream_rejected_deferred(stream._ws_error);
    }
}
Object.defineProperty(WritableStreamDefaultWriter.prototype, 'closed', {
    get: function() { return this._closedD.promise; }
});
Object.defineProperty(WritableStreamDefaultWriter.prototype, 'ready', {
    get: function() { return this._readyD.promise; }
});
Object.defineProperty(WritableStreamDefaultWriter.prototype, 'desiredSize', {
    get: function() {
        var s = this._stream;
        if (!s) throw new TypeError('writer has no stream');
        if (s._ws_state === 'errored' || s._ws_state === 'erroring') return null;
        if (s._ws_state === 'closed') return 0;
        return _ws_ctrl_desired_size(s._ws_ctrl);
    }
});
WritableStreamDefaultWriter.prototype.write = function(chunk) {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('writer has no stream'));
    var controller = stream._ws_ctrl;
    var chunkSize = 1;
    if (controller._sizeFn) {
        try {
            chunkSize = Number(controller._sizeFn(chunk));
        } catch (e) {
            _ws_ctrl_error_if_needed(controller, e);
            return Promise.reject(e);
        }
    }
    var state = stream._ws_state;
    if (state === 'errored') return Promise.reject(stream._ws_error);
    if (_ws_close_queued_or_in_flight(stream) || state === 'closed') {
        return Promise.reject(new TypeError('cannot write to a closing or closed stream'));
    }
    if (state === 'erroring') return Promise.reject(stream._ws_error);
    var promise = _ws_add_write_request(stream);
    _ws_ctrl_enqueue(controller, chunk, chunkSize);
    if (!_ws_close_queued_or_in_flight(stream) && stream._ws_state === 'writable') {
        _ws_update_backpressure(stream, _ws_ctrl_backpressure(controller));
    }
    _ws_ctrl_advance(controller);
    return promise;
};
WritableStreamDefaultWriter.prototype.close = function() {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('writer has no stream'));
    if (_ws_close_queued_or_in_flight(stream)) return Promise.reject(new TypeError('close already requested'));
    return _ws_close(stream);
};
WritableStreamDefaultWriter.prototype.abort = function(reason) {
    var stream = this._stream;
    if (!stream) return Promise.reject(new TypeError('writer has no stream'));
    return _ws_abort(stream, reason);
};
WritableStreamDefaultWriter.prototype.releaseLock = function() {
    var stream = this._stream;
    if (!stream) return;
    var released = new TypeError('writer was released and can no longer be used to monitor the stream state');
    _ws_ensure_ready_rejected(this, released);
    _ws_ensure_closed_rejected(this, released);
    stream._ws_writer = null;
    this._stream = null;
};

// ── TransformStream §5 ───────────────────────────────────────────────────────
// The two halves are wired to each other in both directions: an error on either
// side takes the other down, which is what «errors thrown in transform put the
// writable and readable in an errored state» asks for.
function TransformStreamDefaultController(ts) {
    this._ts = ts;
}
Object.defineProperty(TransformStreamDefaultController.prototype, 'desiredSize', {
    get: function() {
        var c = this._ts._ts_readableCtrl;
        return c ? c.desiredSize : null;
    }
});
TransformStreamDefaultController.prototype.enqueue = function(chunk) {
    var ctrl = this._ts._ts_readableCtrl;
    if (ctrl) ctrl.enqueue(chunk);
};
TransformStreamDefaultController.prototype.terminate = function() {
    var ts = this._ts;
    if (ts._ts_readableCtrl) ts._ts_readableCtrl.close();
    _ts_error_writable(ts, new TypeError('TransformStream terminated'));
};
TransformStreamDefaultController.prototype.error = function(e) {
    _ts_error(this._ts, e);
};

function _ts_error(ts, e) {
    if (ts.readable && ts.readable._rs_state === 'readable' && ts._ts_readableCtrl) {
        ts._ts_readableCtrl.error(e);
    }
    _ts_error_writable(ts, e);
}
function _ts_error_writable(ts, e) {
    if (ts.writable) _ws_ctrl_error_if_needed(ts.writable._ws_ctrl, e);
}
function _ts_transform(ts, chunk) {
    var transformer = ts._ts_transformer;
    if (typeof transformer.transform !== 'function') {
        try {
            ts._ts_ctrl.enqueue(chunk);
        } catch (e) {
            _ts_error(ts, e);
            return Promise.reject(e);
        }
        return Promise.resolve(undefined);
    }
    var result;
    try {
        result = transformer.transform(chunk, ts._ts_ctrl);
    } catch (e) {
        _ts_error(ts, e);
        return Promise.reject(e);
    }
    return Promise.resolve(result).then(function() { return undefined; }, function(e) {
        _ts_error(ts, e);
        return Promise.reject(e);
    });
}
function _ts_flush(ts) {
    var transformer = ts._ts_transformer;
    var result;
    try {
        result = typeof transformer.flush === 'function' ? transformer.flush(ts._ts_ctrl) : undefined;
    } catch (e) {
        _ts_error(ts, e);
        return Promise.reject(e);
    }
    return Promise.resolve(result).then(function() {
        if (ts.readable._rs_state === 'readable' && ts._ts_readableCtrl) ts._ts_readableCtrl.close();
    }, function(e) {
        _ts_error(ts, e);
        return Promise.reject(e);
    });
}

function TransformStream(transformer, writableStrategy, readableStrategy) {
    transformer = transformer || {};
    var self = this;
    this._ts_transformer = transformer;
    this._ts_ctrl = new TransformStreamDefaultController(this);
    this._ts_readableCtrl = null;
    this.readable = new ReadableStream({
        start: function(ctrl) { self._ts_readableCtrl = ctrl; },
        // §5.3: cancelling the readable end errors the writable one, so a writer
        // waiting on `closed` after `readable.cancel()` hears about it.
        cancel: function(reason) { _ts_error_writable(self, reason); }
    }, readableStrategy);
    var startResult;
    try {
        startResult = typeof transformer.start === 'function' ? transformer.start(this._ts_ctrl) : undefined;
        this._ts_startPromise = Promise.resolve(startResult);
    } catch (e) {
        this._ts_startPromise = Promise.reject(e);
    }
    this.writable = new WritableStream({
        start: function() { return self._ts_startPromise; },
        write: function(chunk) { return _ts_transform(self, chunk); },
        close: function() { return _ts_flush(self); },
        abort: function(reason) { _ts_error(self, reason); }
    }, writableStrategy);
    // The writable half hears about a failed start() through its own sink; the
    // readable half needs telling separately.
    _stream_mark_handled(this._ts_startPromise.then(undefined, function(e) { _ts_error(self, e); }));
}

// ── TextDecoderStream / TextEncoderStream (Encoding Standard §5.1) ───────────
function TextDecoderStream(label, options) {
    var dec = new TextDecoder(label, options);
    TransformStream.call(this, {
        transform: function(chunk, c) {
            var str = dec.decode(chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk), { stream: true });
            if (str.length > 0) c.enqueue(str);
        },
        flush: function(c) {
            var str = dec.decode();
            if (str.length > 0) c.enqueue(str);
        }
    });
    this.encoding = dec.encoding;
    this.fatal = dec.fatal;
    this.ignoreBOM = dec.ignoreBOM;
}
TextDecoderStream.prototype = Object.create(TransformStream.prototype);
TextDecoderStream.prototype.constructor = TextDecoderStream;

function TextEncoderStream() {
    var enc = new TextEncoder();
    TransformStream.call(this, {
        transform: function(chunk, c) {
            c.enqueue(enc.encode(String(chunk)));
        }
    });
    this.encoding = 'utf-8';
}
TextEncoderStream.prototype = Object.create(TransformStream.prototype);
TextEncoderStream.prototype.constructor = TextEncoderStream;

// ── CompressionStream / DecompressionStream (WHATWG Compression Streams) ─────
// https://compression.spec.whatwg.org/
// Formats: 'deflate-raw' (raw DEFLATE RFC 1951), 'deflate' (zlib RFC 1950), 'gzip'.
//
// §4/§5 transform algorithm: every chunk goes through a codec that lives in the
// host (`crates/js/src/compression.rs`, keyed by an opaque handle) and whatever
// that chunk produced is enqueued right away. The model used to be
// buffer-then-flush — nothing was decoded until `writer.close()` — so the
// reflexive «write a chunk, read the result» never resolved (BUG-846). `flush`
// now only ends the stream.
var _COMPRESSION_FORMATS = ['deflate-raw', 'deflate', 'gzip'];

// Status byte prefixed to every `_lumen_cs_*` reply, see `compression.rs`.
var _CS_ERROR = 0, _CS_OK = 1, _CS_TRAILING_JUNK = 2;

function _csToU8(chunk) {
    if (chunk instanceof Uint8Array) return chunk;
    if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
    if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    // §4 takes a BufferSource. Anything else is a WebIDL conversion failure,
    // which must error both sides of the stream rather than being read as an
    // empty chunk (`compression-bad-chunks`/`decompression-bad-chunks`).
    throw new TypeError('Compression stream: chunk is not a BufferSource');
}

// Shared transform/flush for both directions: `st` is {h, label, format}.
function _csTransform(st, chunk, c) {
    var bytes = _csToU8(chunk);
    if (!st.h) throw new TypeError(st.label + ': stream is already errored');
    var raw = _lumen_cs_push(st.h, bytes);
    if (raw[0] === _CS_ERROR) {
        st.h = 0; // the host dropped the codec along with the error
        throw new TypeError(st.label + ': corrupt or truncated ' + st.format + ' input');
    }
    // The output has to reach the reader BEFORE the stream is errored: a read
    // request standing at this moment is fulfilled directly, while erroring
    // first would reset the queue and lose it (`decompression-extra-input`
    // asserts the decoded value arrives and only the *next* read rejects).
    if (raw.length > 1) c.enqueue(new Uint8Array(raw.slice(1)));
    if (raw[0] === _CS_TRAILING_JUNK) {
        _lumen_cs_free(st.h);
        st.h = 0;
        throw new TypeError(st.label + ': junk found after the end of the ' + st.format + ' stream');
    }
}
function _csFlush(st, c) {
    if (!st.h) return;
    var raw = _lumen_cs_finish(st.h);
    st.h = 0;
    if (raw[0] !== _CS_OK) {
        throw new TypeError(st.label + ': corrupt or truncated ' + st.format + ' input');
    }
    if (raw.length > 1) c.enqueue(new Uint8Array(raw.slice(1)));
}
function _csInit(self, format, label, decompress) {
    if (_COMPRESSION_FORMATS.indexOf(format) === -1)
        throw new TypeError(label + ': unsupported format: ' + format);
    var st = { h: _lumen_cs_new(format, decompress), label: label, format: format };
    if (!st.h) throw new TypeError(label + ': unsupported format: ' + format);
    TransformStream.call(self, {
        transform: function(chunk, c) { _csTransform(st, chunk, c); },
        flush: function(c) { _csFlush(st, c); }
    });
    self.format = format;
}

function CompressionStream(format) {
    _csInit(this, format, 'CompressionStream', false);
}
CompressionStream.prototype = Object.create(TransformStream.prototype);
CompressionStream.prototype.constructor = CompressionStream;

function DecompressionStream(format) {
    _csInit(this, format, 'DecompressionStream', true);
}
DecompressionStream.prototype = Object.create(TransformStream.prototype);
DecompressionStream.prototype.constructor = DecompressionStream;

// ── ByteLengthQueuingStrategy / CountQueuingStrategy §6 ──────────────────────
function ByteLengthQueuingStrategy(init) {
    this.highWaterMark = (init && typeof init.highWaterMark === 'number') ? init.highWaterMark : 1;
}
ByteLengthQueuingStrategy.prototype.size = function(chunk) {
    return (chunk && chunk.byteLength) ? chunk.byteLength : 0;
};
function CountQueuingStrategy(init) {
    this.highWaterMark = (init && typeof init.highWaterMark === 'number') ? init.highWaterMark : 1;
}
CountQueuingStrategy.prototype.size = function() { return 1; };

