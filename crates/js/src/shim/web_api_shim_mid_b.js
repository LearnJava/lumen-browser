
var _lumen_loc_parts = _lumen_parse_url(typeof _LUMEN_PAGE_URL !== 'undefined' ? _LUMEN_PAGE_URL : '');
var _lumen_loc_href  = _lumen_loc_parts.href;
var _lumen_loc_hash  = _lumen_loc_parts.hash;

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
// without a page reload and fires `hashchange`. Internal updates use the
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
    _lumen_fire_hashchange(oldHref, newHref);
}
// HTML LS navigation entry point for location.href= / assign() / replace().
// If the resolved target differs from the current URL only in its fragment,
// performs a same-document fragment navigation (no reload): updates location,
// pushes/replaces a same-document history entry, and fires `hashchange`.
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
    serviceWorker: _sw_container,
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
        try { return _lumen_send_beacon(url, body, ct); } catch(e) { return false; }
    },
};

// ── Clipboard API (W3C Clipboard API §4) ─────────────────────────────────────
// navigator.clipboard.readText()  → Promise<string>
// navigator.clipboard.writeText(text) → Promise<void>
// navigator.clipboard.read()  → Promise<ClipboardItems> stub (empty array)
// navigator.clipboard.write() → Promise<void> stub
//
// readText/writeText delegate to native bindings (_lumen_clipboard_read /
// _lumen_clipboard_write) when the shell wires them.  Until then readText
// returns '' and writeText silently succeeds.
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
function _lumen_timer_string_handler(code) {
    var src = String(code);
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
    if (typeof fn !== 'function') fn = _lumen_timer_string_handler(fn);
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
    if (typeof fn !== 'function') fn = _lumen_timer_string_handler(fn);
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
    if (callbacks.length === 0) return false;
    for (var i = 0; i < callbacks.length; i++) {
        try { callbacks[i].fn(ts); } catch(e) { _lumen_report_exception(e); }
    }
    return true;
}

var _popstate_listeners = [];

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
    var ev = new PopStateEvent('popstate', { state: s, bubbles: true });
    if (typeof window.onpopstate === 'function') {
        try { window.onpopstate(ev); } catch(e) { _lumen_report_exception(e); }
    }
    for (var i = 0; i < _popstate_listeners.length; i++) {
        try { _popstate_listeners[i](ev); } catch(e) { _lumen_report_exception(e); }
    }
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
        setTimeout(function() {
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
// Called from the shell via `PersistentJs::has_bfcache_freeze_blocker`.
function _lumen_bfcache_blocked() {
    if (_ws_instances.some(function(w) { return w.readyState === 1; })) return true;
    if (_sse_instances.some(function(s) { return s.readyState === 1; })) return true;
    if (typeof window.onbeforeunload === 'function') return true;
    if (typeof window.onunload === 'function') return true;
    if (_other_win_listeners['beforeunload'] && _other_win_listeners['beforeunload'].length > 0) return true;
    if (_other_win_listeners['unload'] && _other_win_listeners['unload'].length > 0) return true;
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

// Headers (Fetch Standard §2.2) — BUG-369.
// A WebIDL interface, not an ES5 constructor: the header list and the Fetch guard
// live in a WeakMap private to the closure below (a page can neither read nor
// clobber them), the prototype carries only non-enumerable methods, iteration
// follows the spec's «sort and combine» order and `Symbol.iterator === entries`.
//
// The Fetch guard is not part of the public API, so Response/Request reach the
// two internal helpers assigned from inside the closure:
//   _lumen_headers_new(init, guard)      — construct with the guard already applied
//                                          (so filling from `init` is guard-checked),
//   _lumen_headers_set_guard(h, guard)   — apply the guard after the list is filled
//                                          (the network path, where the header list
//                                          is set directly rather than appended).
var _lumen_headers_new;
var _lumen_headers_set_guard;
var Headers = (function() {
    // name → per-instance { list: [[lowercased name, value], …], guard }.
    var STATE = new WeakMap();
    function stateOf(h) {
        var st = STATE.get(h);
        if (!st) throw new TypeError('Illegal invocation: receiver is not a Headers object');
        return st;
    }
    // RFC 7230 tchar codes outside ALPHA/DIGIT: ! # $ % & ' * + - . ^ _ ` | ~
    var TOKEN_CODES = [33, 35, 36, 37, 38, 39, 42, 43, 45, 46, 94, 95, 96, 124, 126];
    // Fetch §2.2.1: a header name must be a valid HTTP token.
    function isName(s) {
        if (s.length === 0) return false;
        for (var i = 0; i < s.length; i++) {
            var c = s.charCodeAt(i);
            if ((c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) continue;
            if (TOKEN_CODES.indexOf(c) < 0) return false;
        }
        return true;
    }
    function isHttpWs(c) { return c === 9 || c === 10 || c === 13 || c === 32; }
    // Fetch §2.2.1 «normalize a header value»: strip leading/trailing HTTP whitespace.
    function normalizeValue(value) {
        var s = String(value);
        var a = 0, b = s.length;
        while (a < b && isHttpWs(s.charCodeAt(a))) a++;
        while (b > a && isHttpWs(s.charCodeAt(b - 1))) b--;
        return s.slice(a, b);
    }
    // Fetch §2.2.1: a header value must not contain NUL, CR or LF.
    function isValue(s) {
        for (var i = 0; i < s.length; i++) {
            var c = s.charCodeAt(i);
            if (c === 0 || c === 10 || c === 13) return false;
        }
        return true;
    }
    // Fetch §2.2.2 forbidden request-header names (plus the proxy-/sec- prefixes).
    var FORBIDDEN_REQUEST = ['accept-charset', 'accept-encoding',
        'access-control-request-headers', 'access-control-request-method',
        'access-control-request-private-network', 'connection', 'content-length',
        'cookie', 'cookie2', 'date', 'dnt', 'expect', 'host', 'keep-alive',
        'origin', 'referer', 'set-cookie', 'te', 'trailer', 'transfer-encoding',
        'upgrade', 'via'];
    function isForbiddenRequestName(name) {
        if (FORBIDDEN_REQUEST.indexOf(name) >= 0) return true;
        return name.slice(0, 6) === 'proxy-' || name.slice(0, 4) === 'sec-';
    }
    // Fetch §2.2.2 forbidden response-header names.
    var FORBIDDEN_RESPONSE = ['set-cookie', 'set-cookie2'];
    // Fetch §2.2.2 no-CORS-safelisted request-header names.
    var NO_CORS_NAMES = ['accept', 'accept-language', 'content-language', 'content-type'];
    function isNoCorsSafelisted(name, value) {
        if (NO_CORS_NAMES.indexOf(name) < 0) return false;
        if (value.length > 128) return false;
        if (name !== 'content-type') return true;
        var essence = value.split(';')[0].trim().toLowerCase();
        return essence === 'application/x-www-form-urlencoded'
            || essence === 'multipart/form-data'
            || essence === 'text/plain';
    }
    // Fetch §2.2.5 append/set steps 3-6: throw on an immutable guard, silently
    // drop the write when the guard forbids this particular header.
    function mayWrite(st, name, value) {
        if (st.guard === 'immutable') throw new TypeError('Headers object is immutable');
        if (st.guard === 'request') return !isForbiddenRequestName(name);
        if (st.guard === 'request-no-cors') return isNoCorsSafelisted(name, value);
        if (st.guard === 'response') return FORBIDDEN_RESPONSE.indexOf(name) < 0;
        return true;
    }
    // Same gate for delete(), which has no value to test against the no-cors safelist.
    function mayDelete(st, name) {
        if (st.guard === 'immutable') throw new TypeError('Headers object is immutable');
        if (st.guard === 'request') return !isForbiddenRequestName(name);
        if (st.guard === 'request-no-cors') return NO_CORS_NAMES.indexOf(name) >= 0;
        if (st.guard === 'response') return FORBIDDEN_RESPONSE.indexOf(name) < 0;
        return true;
    }
    // Validates (name, value) and returns the lowercased name, or throws TypeError.
    function checkPair(name, value) {
        var n = String(name);
        var v = normalizeValue(value);
        if (!isName(n)) throw new TypeError('Invalid header name: ' + n);
        if (!isValue(v)) throw new TypeError('Invalid value for header ' + n);
        return [n.toLowerCase(), v];
    }
    function checkName(name) {
        var n = String(name);
        if (!isName(n)) throw new TypeError('Invalid header name: ' + n);
        return n.toLowerCase();
    }
    function appendTo(st, name, value) {
        var kv = checkPair(name, value);
        if (!mayWrite(st, kv[0], kv[1])) return;
        st.list.push(kv);
    }
    // Fetch §2.2.5 «fill»: `init` is another Headers, a sequence of pairs, or a record.
    function fill(headers, init) {
        var st = stateOf(headers);
        if (STATE.has(init)) {
            var src = STATE.get(init).list;
            for (var i = 0; i < src.length; i++) appendTo(st, src[i][0], src[i][1]);
            return;
        }
        if (init !== null && typeof init === 'object' && typeof init[Symbol.iterator] === 'function') {
            var seq = Array.from(init);
            for (var j = 0; j < seq.length; j++) {
                var pair = seq[j];
                if (pair === null || typeof pair !== 'object' || typeof pair[Symbol.iterator] !== 'function') {
                    throw new TypeError('Headers init sequence element is not an iterable pair');
                }
                var kv = Array.from(pair);
                if (kv.length !== 2) throw new TypeError('Headers init sequence element must contain exactly 2 items');
                appendTo(st, kv[0], kv[1]);
            }
            return;
        }
        if (init !== null && typeof init === 'object') {
            var keys = Object.keys(init);
            for (var k = 0; k < keys.length; k++) appendTo(st, keys[k], init[keys[k]]);
            return;
        }
        throw new TypeError('Headers init must be a sequence of pairs or a record');
    }
    // Fetch §2.2.1 «get, decode and split» combining rule: values joined with ', '.
    function combine(st, name) {
        var vals = [];
        for (var i = 0; i < st.list.length; i++) {
            if (st.list[i][0] === name) vals.push(st.list[i][1]);
        }
        return vals.length ? vals.join(', ') : null;
    }
    // Fetch §2.2.3 «sort and combine» — the iteration order of the whole interface:
    // unique names sorted byte-wise, one combined entry each, except set-cookie
    // which contributes one entry per value.
    function sortAndCombine(st) {
        var names = [];
        for (var i = 0; i < st.list.length; i++) {
            if (names.indexOf(st.list[i][0]) < 0) names.push(st.list[i][0]);
        }
        names.sort();
        var out = [];
        for (var n = 0; n < names.length; n++) {
            var name = names[n];
            if (name === 'set-cookie') {
                for (var j = 0; j < st.list.length; j++) {
                    if (st.list[j][0] === name) out.push([name, st.list[j][1]]);
                }
            } else {
                out.push([name, combine(st, name)]);
            }
        }
        return out;
    }
    // Shared prototype of the `entries()`/`keys()`/`values()` iterator objects.
    var IterProto = {};
    Object.defineProperty(IterProto, Symbol.toStringTag, { value: 'Headers Iterator', configurable: true });
    Object.defineProperty(IterProto, Symbol.iterator, {
        value: function() { return this; }, writable: true, configurable: true,
    });
    function makeIterator(pairs, kind) {
        var i = 0;
        var it = Object.create(IterProto);
        Object.defineProperty(it, 'next', {
            value: function() {
                if (i >= pairs.length) return { value: undefined, done: true };
                var p = pairs[i++];
                var v = kind === 'key' ? p[0] : (kind === 'value' ? p[1] : [p[0], p[1]]);
                return { value: v, done: false };
            },
            writable: true, configurable: true,
        });
        return it;
    }
    // `init` is read from `arguments` rather than declared, so Headers.length is 0
    // (WebIDL: the single argument is optional).
    function Headers() {
        if (new.target === undefined) {
            throw new TypeError('Failed to construct Headers: please use the new operator');
        }
        STATE.set(this, { list: [], guard: 'none' });
        var init = arguments[0];
        if (init !== undefined && init !== null) fill(this, init);
    }
    function def(obj, key, value) {
        Object.defineProperty(obj, key, { value: value, writable: true, enumerable: false, configurable: true });
    }
    def(Headers.prototype, 'append', function(name, value) { appendTo(stateOf(this), name, value); });
    def(Headers.prototype, 'set', function(name, value) {
        var st = stateOf(this);
        var kv = checkPair(name, value);
        if (!mayWrite(st, kv[0], kv[1])) return;
        var out = [], replaced = false;
        for (var i = 0; i < st.list.length; i++) {
            if (st.list[i][0] !== kv[0]) { out.push(st.list[i]); continue; }
            if (!replaced) { out.push(kv); replaced = true; }
        }
        if (!replaced) out.push(kv);
        st.list = out;
    });
    def(Headers.prototype, 'get', function(name) { return combine(stateOf(this), checkName(name)); });
    def(Headers.prototype, 'getSetCookie', function() {
        var st = stateOf(this), out = [];
        for (var i = 0; i < st.list.length; i++) {
            if (st.list[i][0] === 'set-cookie') out.push(st.list[i][1]);
        }
        return out;
    });
    def(Headers.prototype, 'has', function(name) {
        var st = stateOf(this), k = checkName(name);
        for (var i = 0; i < st.list.length; i++) if (st.list[i][0] === k) return true;
        return false;
    });
    def(Headers.prototype, 'delete', function(name) {
        var st = stateOf(this), k = checkName(name);
        if (!mayDelete(st, k)) return;
        var out = [];
        for (var i = 0; i < st.list.length; i++) if (st.list[i][0] !== k) out.push(st.list[i]);
        st.list = out;
    });
    def(Headers.prototype, 'forEach', function(cb, thisArg) {
        if (typeof cb !== 'function') throw new TypeError('Headers.forEach requires a function callback');
        var pairs = sortAndCombine(stateOf(this));
        for (var i = 0; i < pairs.length; i++) cb.call(thisArg, pairs[i][1], pairs[i][0], this);
    });
    def(Headers.prototype, 'keys', function() { return makeIterator(sortAndCombine(stateOf(this)), 'key'); });
    def(Headers.prototype, 'values', function() { return makeIterator(sortAndCombine(stateOf(this)), 'value'); });
    def(Headers.prototype, 'entries', function() { return makeIterator(sortAndCombine(stateOf(this)), 'entry'); });
    // WebIDL `iterable<ByteString, ByteString>`: @@iterator is the very same
    // function object as entries(), not a copy.
    def(Headers.prototype, Symbol.iterator, Headers.prototype.entries);
    Object.defineProperty(Headers.prototype, Symbol.toStringTag, { value: 'Headers', configurable: true });

    _lumen_headers_new = function(init, guard) {
        var h = new Headers();
        stateOf(h).guard = guard;
        if (init !== undefined && init !== null) fill(h, init);
        return h;
    };
    _lumen_headers_set_guard = function(h, guard) { stateOf(h).guard = guard; return h; };
    return Headers;
})();

// _rs_make_body_stream(bodyBytes, respRef) — builds a pull()-based ReadableStream
// that delivers bodyBytes in 64 KiB chunks (Fetch Standard §2.2, WHATWG Streams §3.4.4).
// Intercepting getReader() marks respRef.bodyUsed = true so subsequent .text() etc. reject.
var _RS_CHUNK = 65536;
function _rs_make_body_stream(bodyBytes, respRef) {
    var pos = 0;
    var stream = new ReadableStream({
        pull: function(c) {
            if (pos >= bodyBytes.length) { c.close(); return; }
            var end = Math.min(pos + _RS_CHUNK, bodyBytes.length);
            c.enqueue(bodyBytes.subarray(pos, end));
            pos = end;
        },
        cancel: function() { pos = bodyBytes.length; }
    });
    var _orig = stream.getReader.bind(stream);
    stream.getReader = function(opts) {
        if (respRef.bodyUsed) throw new TypeError('body already consumed');
        respRef.bodyUsed = true;
        return _orig(opts);
    };
    return stream;
}

// Reads a ReadableStream to completion and joins it into one Uint8Array. Needed
// because a Request/Response built over a stream body has no bytes to hand out
// until the stream has been drained (BUG-824: `new Response(rs).arrayBuffer()`
// used to resolve with zero bytes, because the constructor built a *fresh* empty
// body stream and dropped the one it was given).
function _rs_drain_to_bytes(stream) {
    var reader = stream.getReader();
    var chunks = [], total = 0;
    function step() {
        return reader.read().then(function(res) {
            if (res.done) {
                var out = new Uint8Array(total), off = 0;
                for (var i = 0; i < chunks.length; i++) { out.set(chunks[i], off); off += chunks[i].length; }
                return out;
            }
            // Fetch §2.2: a body stream yields BufferSource chunks; anything else
            // is a TypeError rather than a silently dropped chunk.
            if (!(res.value instanceof ArrayBuffer) && !ArrayBuffer.isView(res.value)) {
                throw new TypeError('body stream yielded a chunk that is not a BufferSource');
            }
            var u = _csToU8(res.value);
            chunks.push(u);
            total += u.length;
            return step();
        });
    }
    return step();
}

// ── Body mixin, Response, Request (Fetch Standard §2.3-2.6) — BUG-370 ────────
// Both interfaces share one closure because they share the Body mixin and,
// like Headers (BUG-369), they are WebIDL interfaces rather than ES5
// constructors: every attribute is a read-only accessor on the prototype, every
// operation is a prototype property, and the internal slots (byte buffer, Rust
// stream handle, body source) live in WeakMaps the page cannot reach — so
// JSON.stringify() on either yields {} instead of dumping the shim internals.
//
// The shim's own fetch()/Cache code needs two of those slots, so the closure
// assigns them to pre-declared globals:
//   _lumen_response_from_fetch_cache(status, statusText, headers, url)
//        — the network-path factory: the body stays in the Rust FetchCache and
//          is pulled lazily, so large bodies are never copied into JS eagerly;
//   _lumen_body_source(obj)
//        — the unserialised body a Request/Response was built from. fetch()
//          needs it because Request.body is now a ReadableStream (Body mixin),
//          not the raw string/FormData the caller passed.
var _lumen_response_from_fetch_cache;
var _lumen_body_source;
var Response;
var Request;
(function() {
    // instance → private slots. Absent ⇒ the receiver is not one of ours.
    var RSTATE = new WeakMap();
    var QSTATE = new WeakMap();
    function rstate(r) {
        var st = RSTATE.get(r);
        if (!st) throw new TypeError('Illegal invocation: receiver is not a Response object');
        return st;
    }
    function qstate(q) {
        var st = QSTATE.get(q);
        if (!st) throw new TypeError('Illegal invocation: receiver is not a Request object');
        return st;
    }
    // WebIDL §3.7: prototype operations are {writable, enumerable, configurable},
    // attributes are enumerable+configurable accessors with no setter.
    function op(proto, name, fn) {
        Object.defineProperty(proto, name, { value: fn, writable: true, enumerable: true, configurable: true });
    }
    function attr(proto, name, get) {
        Object.defineProperty(proto, name, { get: get, enumerable: true, configurable: true });
    }
    function stat(ctor, name, fn) {
        Object.defineProperty(ctor, name, { value: fn, writable: true, enumerable: true, configurable: true });
    }

    // Fetch §7.1 «extract a body» → {bytes, type, source}, or null for no body.
    // `type` is the Content-Type the body implies; null when it implies none.
    function extractBody(source) {
        if (source === undefined || source === null) return null;
        var bytes = null, type = null;
        if (typeof source === 'string') {
            bytes = new TextEncoder().encode(source);
            type = 'text/plain;charset=UTF-8';
        } else if (source instanceof URLSearchParams) {
            bytes = new TextEncoder().encode(source.toString());
            type = 'application/x-www-form-urlencoded;charset=UTF-8';
        } else if (source instanceof FormData) {
            var boundary = '----LumenFormBoundary' + Math.random().toString(36).slice(2, 10).toUpperCase();
            bytes = source._toMultipart(boundary);
            type = 'multipart/form-data; boundary=' + boundary;
        } else if (source instanceof Blob) {
            bytes = new Uint8Array(source._bytes);
            if (source.type) type = source.type;
        } else if (source instanceof ArrayBuffer) {
            bytes = new Uint8Array(source.slice(0));
        } else if (ArrayBuffer.isView(source)) {
            bytes = new Uint8Array(source.buffer.slice(source.byteOffset, source.byteOffset + source.byteLength));
        } else if (typeof source.getReader === 'function') {
            // Fetch §7.1: for a ReadableStream the body's stream IS the given
            // stream. It cannot be drained synchronously, so the bytes stay unset
            // and every consumer goes through _rs_drain_to_bytes instead
            // (BUG-824: the shim used to substitute an empty body outright).
            return { bytes: new Uint8Array(0), type: null, source: source, stream: source };
        } else {
            bytes = new TextEncoder().encode(String(source));
            type = 'text/plain;charset=UTF-8';
        }
        return { bytes: bytes, type: type, source: source, stream: null };
    }

    // Materialises the body bytes. `drain` frees the Rust stream slot; a peek
    // (clone()) must leave it in place for the original to consume later.
    function readBytes(st, drain) {
        if (st.bytes !== null) return st.bytes;
        var h = st.streamHandle || 0;
        if (h > 0) {
            var len = _lumen_stream_length(h);
            var out = len > 0 ? new Uint8Array(_lumen_stream_chunk(h, 0, len)) : new Uint8Array(0);
            if (drain) { _lumen_stream_free(h); st.streamHandle = 0; }
            return out;
        }
        // BUG-703: the per-response slot is released the moment every byte sits
        // in this stream's own queue — a body up to _RS_CHUNK drains on the eager
        // pull the ReadableStream constructor performs, i.e. before fetch() has
        // even resolved. Read the bytes back from that queue. Falling through to
        // the process-wide FetchCache slot below instead handed the response the
        // body of whatever request finished last: on a page with concurrent
        // fetches (webpack chunk loaders) scripts arrived as each other's bodies.
        if (st.fromFetchCache) {
            var q = (st.stream && st.stream._rs_ctrl) ? st.stream._rs_ctrl._queue : [];
            var total = 0, i;
            for (i = 0; i < q.length; i++) { total += q[i].length; }
            var joined = new Uint8Array(total), off = 0;
            for (i = 0; i < q.length; i++) { joined.set(q[i], off); off += q[i].length; }
            return joined;
        }
        // Fallback for legacy callers that left the body in the shared slot.
        var len2 = _lumen_fetch_body_length();
        return len2 > 0 ? new Uint8Array(_lumen_fetch_body_chunk(0, len2)) : new Uint8Array(0);
    }

    // Fetch §2.3 «consume body»: one-shot, and a locked stream blocks it.
    function consume(st) {
        if (st.bodyUsed) return Promise.reject(new TypeError('body already consumed'));
        if (st.stream && st.stream.locked) return Promise.reject(new TypeError('body stream is locked'));
        st.bodyUsed = true;
        // A body built from a page-supplied ReadableStream has no bytes until the
        // stream is drained; readBytes() cannot see them (BUG-824).
        if (st.streamSource) return _rs_drain_to_bytes(st.stream);
        return Promise.resolve(readBytes(st, true));
    }

    // ── multipart/form-data + urlencoded parsing for body.formData() ─────────
    function multipartBoundary(contentType) {
        var params = contentType.split(';');
        for (var i = 1; i < params.length; i++) {
            var p = params[i].trim();
            if (p.slice(0, 9).toLowerCase() !== 'boundary=') continue;
            var v = p.slice(9).trim();
            if (v.length >= 2 && v[0] === '"' && v[v.length - 1] === '"') v = v.slice(1, -1);
            return v;
        }
        return null;
    }
    function contentDispositionName(head) {
        var lines = head.split('\r\n');
        for (var i = 0; i < lines.length; i++) {
            if (lines[i].slice(0, 20).toLowerCase() !== 'content-disposition:') continue;
            var parts = lines[i].split(';');
            for (var j = 1; j < parts.length; j++) {
                var p = parts[j].trim();
                if (p.slice(0, 5).toLowerCase() !== 'name=') continue;
                var v = p.slice(5).trim();
                if (v.length >= 2 && v[0] === '"' && v[v.length - 1] === '"') v = v.slice(1, -1);
                return v;
            }
        }
        return null;
    }
    // Fetch §2.3 body.formData(): only urlencoded and multipart bodies parse.
    function parseFormData(bytes, contentType) {
        var essence = contentType.split(';')[0].trim().toLowerCase();
        var text = new TextDecoder().decode(bytes);
        var fd = new FormData();
        if (essence === 'application/x-www-form-urlencoded') {
            new URLSearchParams(text).forEach(function(v, k) { fd.append(k, v); });
            return fd;
        }
        if (essence === 'multipart/form-data') {
            var boundary = multipartBoundary(contentType);
            if (boundary === null) throw new TypeError('multipart/form-data body has no boundary parameter');
            var parts = text.split('--' + boundary);
            for (var i = 1; i < parts.length; i++) {
                var part = parts[i];
                if (part.slice(0, 2) === '--') break; // closing delimiter
                var sep = part.indexOf('\r\n\r\n');
                if (sep < 0) continue;
                var value = part.slice(sep + 4);
                if (value.slice(-2) === '\r\n') value = value.slice(0, -2);
                var name = contentDispositionName(part.slice(0, sep));
                if (name !== null) fd.append(name, value);
            }
            return fd;
        }
        throw new TypeError('Failed to parse body as FormData: unsupported Content-Type ' + contentType);
    }

    // Installs the Body mixin (Fetch §2.3) on an interface prototype. `stateOf`
    // resolves the receiver's private slots — the very same seven members must
    // appear on Request and on Response.
    function installBody(proto, stateOf) {
        attr(proto, 'body', function() { return stateOf(this).stream; });
        attr(proto, 'bodyUsed', function() { return stateOf(this).bodyUsed; });
        op(proto, 'arrayBuffer', function() {
            return consume(stateOf(this)).then(function(b) {
                return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
            });
        });
        op(proto, 'bytes', function() {
            return consume(stateOf(this)).then(function(b) { return new Uint8Array(b); });
        });
        op(proto, 'blob', function() {
            var ct = stateOf(this).headers.get('content-type');
            return consume(stateOf(this)).then(function(b) { return new Blob([b], { type: ct || '' }); });
        });
        op(proto, 'text', function() {
            return consume(stateOf(this)).then(function(b) { return new TextDecoder().decode(b); });
        });
        op(proto, 'json', function() {
            return consume(stateOf(this)).then(function(b) { return JSON.parse(new TextDecoder().decode(b)); });
        });
        op(proto, 'formData', function() {
            var ct = stateOf(this).headers.get('content-type') || '';
            return consume(stateOf(this)).then(function(b) { return parseFormData(b, ct); });
        });
    }

    // ── Response (Fetch Standard §2.6) ───────────────────────────────────────
    // Statuses that must not carry a body, and the redirect status set.
    function isNullBodyStatus(s) { return s === 101 || s === 103 || s === 204 || s === 205 || s === 304; }
    var REDIRECT_STATUSES = [301, 302, 303, 307, 308];

    function responseSlots(headers) {
        return { status: 200, statusText: '', headers: headers, redirected: false,
                 type: 'default', url: '', bodyUsed: false, bytes: new Uint8Array(0),
                 source: null, stream: null, streamSource: false,
                 fromFetchCache: false, streamHandle: 0 };
    }
    // Wraps ready-made slots, bypassing the constructor's validation — error(),
    // redirect(), clone() and the network factory all produce states the public
    // constructor rejects (status 0, immutable headers, type 'error', …).
    function rawResponse(slots) {
        var r = Object.create(Response.prototype);
        RSTATE.set(r, slots);
        return r;
    }

    // `body`/`init` come off `arguments`, so Response.length is 0 (WebIDL: both
    // arguments are optional).
    Response = function Response() {
        if (new.target === undefined) {
            throw new TypeError('Failed to construct Response: please use the new operator');
        }
        var body = arguments[0];
        var init = arguments[1];
        init = (init === undefined || init === null) ? {} : Object(init);
        var status = init.status === undefined ? 200 : Number(init.status);
        if (!(status >= 200 && status <= 599)) {
            throw new RangeError('Failed to construct Response: status ' + init.status + ' is outside 200-599');
        }
        if (body !== undefined && body !== null && isNullBodyStatus(status)) {
            throw new TypeError('Failed to construct Response: status ' + status + ' must not carry a body');
        }
        // Fetch §2.6: the guard is 'response' before init.headers is filled in,
        // so a page cannot smuggle Set-Cookie through the Response constructor.
        var st = responseSlots(_lumen_headers_new(init.headers === undefined ? [] : init.headers, 'response'));
        st.status = status;
        st.statusText = init.statusText === undefined ? '' : String(init.statusText);
        var extracted = extractBody(body);
        if (extracted !== null) {
            st.bytes = extracted.bytes;
            st.source = extracted.source;
            // Fetch §2.6 step 8: the body's implied Content-Type only fills a gap.
            if (extracted.type !== null && !st.headers.has('content-type')) {
                st.headers.set('content-type', extracted.type);
            }
        }
        RSTATE.set(this, st);
        // A null body stays null (`new Response().body === null`); only a real
        // body gets a ReadableStream — and a body that *was* a stream keeps that
        // very stream as `response.body` (BUG-824).
        if (extracted !== null && extracted.stream !== null) {
            st.stream = extracted.stream;
            st.streamSource = true;
        } else if (extracted !== null) {
            st.stream = _rs_make_body_stream(st.bytes, st);
        }
    };
    attr(Response.prototype, 'type', function() { return rstate(this).type; });
    attr(Response.prototype, 'url', function() { return rstate(this).url; });
    attr(Response.prototype, 'redirected', function() { return rstate(this).redirected; });
    attr(Response.prototype, 'status', function() { return rstate(this).status; });
    attr(Response.prototype, 'ok', function() { var s = rstate(this).status; return s >= 200 && s < 300; });
    attr(Response.prototype, 'statusText', function() { return rstate(this).statusText; });
    attr(Response.prototype, 'headers', function() { return rstate(this).headers; });
    installBody(Response.prototype, rstate);
    op(Response.prototype, 'clone', function() {
        var st = rstate(this);
        if (st.bodyUsed || (st.stream && st.stream.locked)) {
            throw new TypeError('Failed to execute clone on Response: body is already used');
        }
        var bytes = st.streamSource ? new Uint8Array(0) : readBytes(st, false);
        // Fetch §2.6 «clone a response» copies the header list verbatim, so the
        // copy is built guard-free and locked afterwards — going through the
        // Response constructor would have the 'response' guard drop Set-Cookie.
        var copy = responseSlots(_lumen_headers_set_guard(new Headers(st.headers), 'response'));
        copy.status = st.status;
        copy.statusText = st.statusText;
        copy.redirected = st.redirected;
        copy.type = st.type;
        copy.url = st.url;
        copy.bytes = bytes;
        copy.source = st.source;
        var r = rawResponse(copy);
        if (st.streamSource) {
            // Fetch §2.3 «clone a body»: tee the stream and give each side one
            // branch. This is exactly what tee() could not do before BUG-824.
            var branches = st.stream.tee();
            st.stream = branches[0];
            copy.stream = branches[1];
            copy.streamSource = true;
        } else if (st.stream !== null) {
            copy.stream = _rs_make_body_stream(bytes, copy);
        }
        return r;
    });
    Object.defineProperty(Response.prototype, Symbol.toStringTag, { value: 'Response', configurable: true });
    // Fetch §2.6 Response.error(): a network error — status 0, type 'error',
    // immutable headers. Routing through the constructor (as the old shim did)
    // hardcoded type = 'default', so `response.type === 'error'`, the canonical
    // network-error test every CORS-aware caller uses, was never true.
    stat(Response, 'error', function() {
        var st = responseSlots(_lumen_headers_new([], 'immutable'));
        st.status = 0;
        st.type = 'error';
        return rawResponse(st);
    });
    // Fetch §2.6 Response.redirect(url, status = 302): the serialised URL goes
    // into the Location header — without it a redirect response is meaningless.
    stat(Response, 'redirect', function(url) {
        var status = arguments[1];
        var s = status === undefined ? 302 : Number(status);
        if (REDIRECT_STATUSES.indexOf(s) < 0) {
            throw new RangeError('Failed to execute redirect on Response: ' + status + ' is not a redirect status');
        }
        var loc = _url_resolve(String(url), _lumen_document_base_url());
        if (!loc) throw new TypeError('Failed to execute redirect on Response: cannot parse URL ' + url);
        // Filled first, locked second: an immutable guard rejects every write.
        var headers = _lumen_headers_new([], 'none');
        headers.set('Location', loc);
        var st = responseSlots(_lumen_headers_set_guard(headers, 'immutable'));
        st.status = s;
        return rawResponse(st);
    });
    // Fetch §2.6 Response.json(data, init) — the 2022 static factory, a
    // different thing from the Response.prototype.json() body parser.
    stat(Response, 'json', function(data) {
        var text = JSON.stringify(data);
        if (text === undefined) {
            throw new TypeError('Failed to execute json on Response: value is not JSON-serializable');
        }
        // Handed over as bytes so extractBody implies no Content-Type of its own
        // and `init.headers` keeps priority over the application/json default.
        var r = new Response(new TextEncoder().encode(text), arguments[1]);
        var st = RSTATE.get(r);
        if (!st.headers.has('content-type')) st.headers.set('content-type', 'application/json');
        return r;
    });

    // Network path: the header list comes off the wire (Set-Cookie included), so
    // it is filled first and only then locked behind the 'response' guard.
    // _lumen_stream_alloc() copies the body out of the single FetchCache slot
    // into a dedicated entry, so later fetch() calls cannot clobber this body.
    _lumen_response_from_fetch_cache = function(status, statusText, headers, url) {
        var st = responseSlots(_lumen_headers_set_guard(new Headers(headers), 'response'));
        st.status = status;
        st.statusText = statusText;
        st.url = url;
        st.bytes = null; // consumed via the stream slot
        st.fromFetchCache = true;
        var r = rawResponse(st);
        var handle = _lumen_stream_alloc();
        st.streamHandle = handle;
        var totalLen = _lumen_stream_length(handle);
        var pos = 0, freed = false;
        function freeHandle() {
            if (!freed && handle > 0) { freed = true; _lumen_stream_free(handle); st.streamHandle = 0; }
        }
        var stream = new ReadableStream({
            pull: function(c) {
                if (pos >= totalLen) { freeHandle(); c.close(); return; }
                var size = Math.min(_RS_CHUNK, totalLen - pos);
                c.enqueue(new Uint8Array(_lumen_stream_chunk(handle, pos, size)));
                pos += size;
                if (pos >= totalLen) freeHandle();
            },
            cancel: function() { freeHandle(); pos = totalLen; }
        });
        var origGetReader = stream.getReader.bind(stream);
        stream.getReader = function(opts) {
            if (st.bodyUsed) throw new TypeError('body already consumed');
            st.bodyUsed = true;
            return origGetReader(opts);
        };
        st.stream = stream;
        return r;
    };

    // ── Request (Fetch Standard §2.5) ────────────────────────────────────────
    // Fetch §5 «normalize a method»: uppercase only these six.
    var NORMALIZED_METHODS = ['DELETE', 'GET', 'HEAD', 'OPTIONS', 'POST', 'PUT'];
    var FORBIDDEN_METHODS = ['CONNECT', 'TRACE', 'TRACK'];
    var METHOD_TOKEN = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;
    function normalizeMethod(m) {
        var s = String(m);
        if (!METHOD_TOKEN.test(s)) {
            throw new TypeError('Failed to construct Request: ' + s + ' is not a valid HTTP method');
        }
        var up = s.toUpperCase();
        if (FORBIDDEN_METHODS.indexOf(up) >= 0) {
            throw new TypeError('Failed to construct Request: forbidden method ' + s);
        }
        return NORMALIZED_METHODS.indexOf(up) >= 0 ? up : s;
    }
    function requestSlots() {
        return { url: '', method: 'GET', headers: null, destination: '', referrer: 'about:client',
                 referrerPolicy: '', mode: 'cors', credentials: 'same-origin', cache: 'default',
                 redirect: 'follow', integrity: '', keepalive: false, signal: null,
                 bodyUsed: false, bytes: null, source: null, stream: null,
                 streamSource: false, fromFetchCache: false, streamHandle: 0 };
    }
    // Only `input` is declared, so Request.length is 1 (WebIDL: `init` is optional).
    Request = function Request(input) {
        if (new.target === undefined) {
            throw new TypeError('Failed to construct Request: please use the new operator');
        }
        var init = arguments[1];
        init = (init === undefined || init === null) ? {} : Object(init);
        // A Request input contributes every unset member; anything else is a URL.
        var src = QSTATE.get(input) || null;
        var st = requestSlots();
        // Fetch §5 step 6: the request URL is parsed against the API base URL
        // (the document base), the same resolution fetch() applies (BUG-347).
        st.url = src !== null ? src.url : _url_resolve(String(input), _lumen_document_base_url());
        st.mode = init.mode !== undefined ? String(init.mode) : (src !== null ? src.mode : 'cors');
        st.credentials = init.credentials !== undefined ? String(init.credentials) : (src !== null ? src.credentials : 'same-origin');
        st.cache = init.cache !== undefined ? String(init.cache) : (src !== null ? src.cache : 'default');
        st.redirect = init.redirect !== undefined ? String(init.redirect) : (src !== null ? src.redirect : 'follow');
        st.referrer = init.referrer !== undefined ? String(init.referrer) : (src !== null ? src.referrer : 'about:client');
        st.referrerPolicy = init.referrerPolicy !== undefined ? String(init.referrerPolicy) : (src !== null ? src.referrerPolicy : '');
        st.integrity = init.integrity !== undefined ? String(init.integrity) : (src !== null ? src.integrity : '');
        st.keepalive = init.keepalive !== undefined ? !!init.keepalive : (src !== null ? src.keepalive : false);
        st.signal = (init.signal !== undefined && init.signal !== null) ? init.signal
                  : (src !== null ? src.signal : new AbortSignal());
        // Fetch §5 steps 12-13: reject a non-token method and the three forbidden
        // ones, and uppercase only the six normalised names (`patch` stays lower).
        st.method = normalizeMethod(init.method !== undefined ? init.method : (src !== null ? src.method : 'GET'));
        // Fetch §5 step 30: guard 'request', or 'request-no-cors' in no-cors mode,
        // so a page cannot set Host/Cookie/Origin on the request.
        st.headers = _lumen_headers_new(
            init.headers !== undefined ? init.headers : (src !== null ? src.headers : []),
            st.mode === 'no-cors' ? 'request-no-cors' : 'request');
        var body = init.body !== undefined ? init.body : (src !== null ? src.source : null);
        // Fetch §5 step 36: a GET/HEAD request cannot carry a body.
        if (body !== undefined && body !== null && (st.method === 'GET' || st.method === 'HEAD')) {
            throw new TypeError('Failed to construct Request: body is not allowed for ' + st.method);
        }
        var extracted = extractBody(body);
        if (extracted !== null) {
            st.bytes = extracted.bytes;
            st.source = extracted.source;
            if (extracted.type !== null && !st.headers.has('content-type')) {
                st.headers.set('content-type', extracted.type);
            }
        } else {
            st.bytes = new Uint8Array(0);
        }
        QSTATE.set(this, st);
        if (extracted !== null && extracted.stream !== null) {
            st.stream = extracted.stream;
            st.streamSource = true;
        } else if (extracted !== null) {
            st.stream = _rs_make_body_stream(st.bytes, st);
        }
    };
    attr(Request.prototype, 'url', function() { return qstate(this).url; });
    attr(Request.prototype, 'method', function() { return qstate(this).method; });
    attr(Request.prototype, 'headers', function() { return qstate(this).headers; });
    attr(Request.prototype, 'destination', function() { return qstate(this).destination; });
    attr(Request.prototype, 'referrer', function() { return qstate(this).referrer; });
    attr(Request.prototype, 'referrerPolicy', function() { return qstate(this).referrerPolicy; });
    attr(Request.prototype, 'mode', function() { return qstate(this).mode; });
    attr(Request.prototype, 'credentials', function() { return qstate(this).credentials; });
    attr(Request.prototype, 'cache', function() { return qstate(this).cache; });
    attr(Request.prototype, 'redirect', function() { return qstate(this).redirect; });
    attr(Request.prototype, 'integrity', function() { return qstate(this).integrity; });
    attr(Request.prototype, 'keepalive', function() { return qstate(this).keepalive; });
    attr(Request.prototype, 'signal', function() { return qstate(this).signal; });
    installBody(Request.prototype, qstate);
    op(Request.prototype, 'clone', function() {
        var st = qstate(this);
        if (st.bodyUsed || (st.stream && st.stream.locked)) {
            throw new TypeError('Failed to execute clone on Request: body is already used');
        }
        var copy = requestSlots();
        copy.url = st.url; copy.method = st.method; copy.destination = st.destination;
        copy.referrer = st.referrer; copy.referrerPolicy = st.referrerPolicy;
        copy.mode = st.mode; copy.credentials = st.credentials; copy.cache = st.cache;
        copy.redirect = st.redirect; copy.integrity = st.integrity;
        copy.keepalive = st.keepalive; copy.signal = st.signal;
        copy.bytes = st.streamSource ? new Uint8Array(0) : readBytes(st, false);
        copy.source = st.source;
        // As in Response.clone: copy the header list verbatim (guard-free), then
        // lock the copy behind the same guard the original carried.
        copy.headers = _lumen_headers_set_guard(new Headers(st.headers),
            st.mode === 'no-cors' ? 'request-no-cors' : 'request');
        var q = Object.create(Request.prototype);
        QSTATE.set(q, copy);
        if (st.streamSource) {
            var branches = st.stream.tee();
            st.stream = branches[0];
            copy.stream = branches[1];
            copy.streamSource = true;
        } else if (st.stream !== null) {
            copy.stream = _rs_make_body_stream(copy.bytes, copy);
        }
        return q;
    });
    Object.defineProperty(Request.prototype, Symbol.toStringTag, { value: 'Request', configurable: true });

    _lumen_body_source = function(obj) {
        if (obj === null || typeof obj !== 'object') return null;
        var st = QSTATE.get(obj) || RSTATE.get(obj);
        return st ? st.source : null;
    };
})();

// ── FormData (XHR Spec §4 / Fetch Spec) ────────────────────────────────────
// Stores an ordered list of (name, value) pairs. Values are always strings
// (File/Blob support is Phase 2+). Serializes to application/x-www-form-urlencoded.

function FormData(formEl) {
    this._entries = [];
    if (formEl && typeof formEl === 'object' && formEl.tagName === 'FORM') {
        var inputs = formEl.querySelectorAll('input,select,textarea');
        for (var i = 0; i < inputs.length; i++) {
            var el = inputs[i];
            var name = el.getAttribute('name');
            if (!name) { continue; }
            var type = (el.getAttribute('type') || '').toLowerCase();
            if (type === 'checkbox' || type === 'radio') {
                if (!el.checked) { continue; }
            }
            if (type === 'submit' || type === 'reset' || type === 'button' || type === 'image') { continue; }
            this._entries.push([String(name), String(el.value || '')]);
        }
    }
}

FormData.prototype.append = function(name, value) {
    this._entries.push([String(name), String(value)]);
};

FormData.prototype.delete = function(name) {
    var n = String(name);
    this._entries = this._entries.filter(function(e) { return e[0] !== n; });
};

FormData.prototype.get = function(name) {
    var n = String(name);
    for (var i = 0; i < this._entries.length; i++) {
        if (this._entries[i][0] === n) { return this._entries[i][1]; }
    }
    return null;
};

FormData.prototype.getAll = function(name) {
    var n = String(name);
    return this._entries.filter(function(e) { return e[0] === n; }).map(function(e) { return e[1]; });
};

FormData.prototype.has = function(name) {
    var n = String(name);
    return this._entries.some(function(e) { return e[0] === n; });
};

FormData.prototype.set = function(name, value) {
    var n = String(name), v = String(value);
    var found = false;
    this._entries = this._entries.filter(function(e) {
        if (e[0] === n) {
            if (!found) { found = true; e[1] = v; return true; }
            return false;
        }
        return true;
    });
    if (!found) { this._entries.push([n, v]); }
};

FormData.prototype.entries = function() {
    var arr = this._entries.slice();
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.keys = function() {
    var arr = this._entries.map(function(e) { return e[0]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.values = function() {
    var arr = this._entries.map(function(e) { return e[1]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.forEach = function(cb, thisArg) {
    for (var i = 0; i < this._entries.length; i++) {
        cb.call(thisArg, this._entries[i][1], this._entries[i][0], this);
    }
};

FormData.prototype[Symbol.iterator] = function() { return this.entries(); };

/// Serialize to application/x-www-form-urlencoded (RFC 3986 percent-encoding).
FormData.prototype._toUrlEncoded = function() {
    return this._entries.map(function(e) {
        return encodeURIComponent(e[0]) + '=' + encodeURIComponent(e[1]);
    }).join('&');
};

FormData.prototype._toMultipart = function(boundary) {
    var enc = new TextEncoder();
    var parts = [];
    var dash = enc.encode('--');
    var bnd = enc.encode(boundary);
    var crlf = enc.encode('\r\n');
    for (var i = 0; i < this._entries.length; i++) {
        var name = this._entries[i][0];
        var value = this._entries[i][1];
        var safeName = name.replace(/\r/g, '%0D').replace(/\n/g, '%0A').replace(/\x22/g, '%22');
        var disp = 'Content-Disposition: form-data; name=\x22' + safeName + '\x22\r\n\r\n';
        var dispHeader = enc.encode(disp);
        var body = enc.encode(value);
        parts.push(dash, bnd, crlf, dispHeader, body, crlf);
    }
    parts.push(dash, bnd, enc.encode('--'), crlf);
    var totalLen = 0;
    for (var j = 0; j < parts.length; j++) { totalLen += parts[j].length; }
    var out = new Uint8Array(totalLen);
    var off = 0;
    for (var k = 0; k < parts.length; k++) {
        out.set(parts[k], off);
        off += parts[k].length;
    }
    return out;
};

// ── TextEncoder / TextDecoder (WHATWG Encoding §8–9) ─────────────────────────
// encode() stays a pure-JS UTF-8 encoder (the encoder is always UTF-8 per
// spec). Decoding — label canonicalization, RangeError on unknown labels,
// real multi-encoding decode and fatal-mode error detection — is bridged to
// the native `_lumen_text_decode`/`_lumen_text_encoding_for_label` functions
// (crates/js/src/v8_runtime.rs), backed by `lumen_encoding` (BUG-357). That
// decoder is stateless (whole-buffer in, `String` out — no incremental
// decoder object), so streaming reassembly (holding back a byte sequence a
// chunk boundary split mid-character) and the only-strip-a-BOM-on-the-first-
// chunk-of-a-stream rule for `ignoreBOM` are handled here in JS.
//
// Supported encodings match `lumen_encoding::Encoding` — UTF-8/16/32,
// windows-1251, KOI8-R, IBM866 — the Cyrillic-web + Unicode set this browser
// actually implements (`docs/plan/tech-stack.md` deliberately rejects
// `encoding_rs`/hand-porting the full ~40-encoding WHATWG set in favor of
// this crate's own tables). A label for any other real-but-unimplemented
// encoding (Shift_JIS, GBK, windows-1252, …) is treated the same as an
// unknown label: `_lumen_text_encoding_for_label` returns undefined and the
// constructor throws `RangeError` — a deliberate scope decision, not a bug.

function TextEncoder() {}
Object.defineProperty(TextEncoder.prototype, 'encoding', {
    value: 'utf-8', enumerable: true, configurable: true
});
TextEncoder.prototype.encode = function(str) {
    var s = String(str === undefined ? '' : str);
    var bytes = [];
    for (var i = 0; i < s.length; i++) {
        var c = s.charCodeAt(i);
        if (c < 0x80) {
            bytes.push(c);
        } else if (c < 0x800) {
            bytes.push(0xC0 | (c >> 6));
            bytes.push(0x80 | (c & 0x3F));
        } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
            var lo = s.charCodeAt(i + 1);
            var cp = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
            bytes.push(0xF0 | (cp >> 18));
            bytes.push(0x80 | ((cp >> 12) & 0x3F));
            bytes.push(0x80 | ((cp >> 6) & 0x3F));
            bytes.push(0x80 | (cp & 0x3F));
            i++;
        } else {
            bytes.push(0xE0 | (c >> 12));
            bytes.push(0x80 | ((c >> 6) & 0x3F));
            bytes.push(0x80 | (c & 0x3F));
        }
    }
    return new Uint8Array(bytes);
};
// Encoding §6.2 encodeInto — same per-code-unit encoding as encode(), but
// writes directly into `dest` and stops once it runs out of room, reporting
// how many UTF-16 code units of `src` were consumed and bytes written. Never
// splits a surrogate pair or a multi-byte UTF-8 sequence across the boundary.
TextEncoder.prototype.encodeInto = function(src, dest) {
    var s = String(src === undefined ? '' : src);
    var read = 0, written = 0, i = 0;
    while (i < s.length) {
        var c = s.charCodeAt(i);
        var unitLen = 1, out;
        if (c < 0x80) {
            out = [c];
        } else if (c < 0x800) {
            out = [0xC0 | (c >> 6), 0x80 | (c & 0x3F)];
        } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
            var lo = s.charCodeAt(i + 1);
            var cp = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
            out = [0xF0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3F), 0x80 | ((cp >> 6) & 0x3F), 0x80 | (cp & 0x3F)];
            unitLen = 2;
        } else {
            out = [0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F)];
        }
        if (written + out.length > dest.length) break;
        for (var k = 0; k < out.length; k++) dest[written + k] = out[k];
        written += out.length;
        read += unitLen;
        i += unitLen;
    }
    return { read: read, written: written };
};

// Returns how many trailing bytes of `bytes` belong to a code unit/sequence
// the buffer cuts off mid-way, for the multi-byte encoding families where a
// streaming chunk boundary can land inside a character. Those bytes must be
// held back and prepended to the next chunk instead of being decoded now.
// Single-byte encodings (windows-1251/koi8-r/ibm866) never have a pending
// remainder — every byte stands alone.
function _lumenTextPendingTailLen(canonical, bytes) {
    var n = bytes.length;
    if (canonical === 'utf-8') {
        var i = 0;
        while (i < n) {
            var b = bytes[i];
            var seqLen;
            if (b < 0x80) { seqLen = 1; }
            else if ((b & 0xE0) === 0xC0) { seqLen = 2; }
            else if ((b & 0xF0) === 0xE0) { seqLen = 3; }
            else if ((b & 0xF8) === 0xF0) { seqLen = 4; }
            else { i++; continue; } // stray/invalid byte — not a pending lead
            if (i + seqLen > n) { return n - i; }
            i += seqLen;
        }
        return 0;
    }
    if (canonical === 'utf-16le' || canonical === 'utf-16be') { return n % 2; }
    if (canonical === 'utf-32le' || canonical === 'utf-32be') { return n % 4; }
    return 0;
}

function TextDecoder(label, options) {
    var canonical = _lumen_text_encoding_for_label(label === undefined ? 'utf-8' : String(label));
    if (canonical === undefined) {
        throw new RangeError("Failed to construct 'TextDecoder': The encoding label provided ('" + label + "') is invalid.");
    }
    this._encoding = canonical;
    this._fatal = !!(options && options.fatal);
    this._ignoreBOM = !!(options && options.ignoreBOM);
    this._pending = null;   // bytes held back from a previous streaming chunk
    this._sawInput = false; // BOM stripping applies only to a stream's first chunk
}
Object.defineProperty(TextDecoder.prototype, 'encoding', {
    get: function() { return this._encoding; }, enumerable: true, configurable: true
});
Object.defineProperty(TextDecoder.prototype, 'fatal', {
    get: function() { return this._fatal; }, enumerable: true, configurable: true
});
Object.defineProperty(TextDecoder.prototype, 'ignoreBOM', {
    get: function() { return this._ignoreBOM; }, enumerable: true, configurable: true
});
// Encoding Standard §9.1 decode(). The native `_lumen_text_decode` call does
// the actual per-encoding decode and fatal-mode error detection (signalled
// by returning undefined); this wrapper reassembles streaming chunks, keeps
// an incomplete trailing sequence for the next call, and turns the native
// malformed-input signal into the spec-mandated TypeError.
TextDecoder.prototype.decode = function(buf, options) {
    var stream = !!(options && options.stream);
    var input;
    if (buf === undefined || buf === null) {
        input = new Uint8Array(0);
    } else {
        input = buf instanceof Uint8Array ? buf : new Uint8Array(buf instanceof ArrayBuffer ? buf : new ArrayBuffer(0));
    }
    var bytes;
    if (this._pending && this._pending.length > 0) {
        var combined = new Uint8Array(this._pending.length + input.length);
        combined.set(this._pending);
        combined.set(input, this._pending.length);
        bytes = combined;
    } else {
        bytes = input;
    }
    this._pending = null;

    var toDecode = bytes;
    if (stream) {
        var pendLen = _lumenTextPendingTailLen(this._encoding, bytes);
        if (pendLen > 0) {
            this._pending = bytes.slice(bytes.length - pendLen);
            toDecode = bytes.slice(0, bytes.length - pendLen);
        }
    }

    // A BOM is only meaningful at the start of a decode session — pass
    // ignoreBOM=true (suppress stripping) on every chunk after the first so a
    // BOM-like byte sequence arriving mid-stream is decoded as plain content.
    var ignoreBOMForThisCall = this._sawInput ? true : this._ignoreBOM;
    this._sawInput = true;

    var result = _lumen_text_decode(this._encoding, toDecode, ignoreBOMForThisCall, this._fatal);
    if (result === undefined) {
        throw new TypeError('Failed to decode: The encoded data was not valid ' + this._encoding + ' data.');
    }
    if (!stream) {
        // Encoding Standard: a non-streaming decode() always ends the session
        // — the next call, streaming or not, starts fresh.
        this._pending = null;
        this._sawInput = false;
    }
    return result;
};

// fetch() (Fetch Standard §3) — synchronous under the hood, wrapped in Promise.
// Supports request body: FormData → application/x-www-form-urlencoded,
// string → text/plain;charset=UTF-8, Uint8Array/ArrayBuffer → application/octet-stream.
// FormData → multipart/form-data with a generated boundary (Fetch spec §5.4 «extract a body»).
// Declared under an internal name and published through defineProperty below:
// a bare `function fetch()` at global scope lands as configurable:false, which
// blocks every polyfill or test shim that swaps window.fetch out (BUG-370 C4).
// Only `input` is declared, so fetch.length is 1 (WebIDL: `init` is optional).
// Record a Resource Timing entry for a fetch the shim itself performed
// (BUG-839). Reads the response metadata straight out of the native fetch
// cache, so it must be called while that slot still holds this response — that
// is, before anything starts the next request.
//
// Everything the page loads through the shim funnels here: `fetch()` itself,
// `<script src>`, `<link rel=stylesheet>` and the `rel=preload` family all end
// up calling `fetch()` (BUG-826/BUG-703), which is why the initiator type is a
// parameter rather than the constant 'fetch' the URL alone would suggest.
function _perf_rt_record_fetch(url, initiator, startMs, status) {
    if (typeof _lumen_record_resource_timing !== 'function') return;
    var len = 0;
    try { len = _lumen_fetch_body_length(); } catch (e) { len = 0; }
    var ctype = '';
    try {
        var raw = _lumen_fetch_get_headers();
        for (var i = 0; i + 1 < raw.length; i += 2) {
            if (String(raw[i]).toLowerCase() === 'content-type') { ctype = String(raw[i + 1]); break; }
        }
    } catch (e) { ctype = ''; }
    _lumen_record_resource_timing(url, initiator, startMs, performance.now() - startMs,
        { status: status, decodedBodySize: len, encodedBodySize: len, contentType: ctype });
}

function _lumen_fetch(input) {
    var init = arguments[1];
    try {
        // Fetch §4.1 step 13: an already-aborted signal rejects immediately with
        // its reason. Lumen's fetch is synchronous, so this pre-flight check is
        // the only cancellation point (no in-flight abort in Phase 0).
        var fetchSignal = (init && init.signal) ? init.signal
                        : (typeof input === 'object' && input && input.signal ? input.signal : null);
        if (fetchSignal && fetchSignal.aborted) {
            return Promise.reject(
                fetchSignal.reason !== undefined ? fetchSignal.reason
                    : new DOMException('signal is aborted without reason', 'AbortError'));
        }
        var url = typeof input === 'string' ? input : (input && input.url ? input.url : String(input));
        // Fetch §4.1 step 8: the request URL is parsed relative to the API base URL
        // (the document base) — a bare `fetch('resources/x.js')` must resolve against
        // the current page, not fail as an absolute-URL parse (BUG-347).
        url = _url_resolve(String(url), _lumen_document_base_url());
        // Resource Timing L2 §4.1: `startTime` is the moment the fetch starts,
        // i.e. after the URL is known and before anything touches the network.
        // `_lumenInitiatorType` is the shim's own channel — an element loading
        // itself through `fetch()` must report `script`/`link`, not `fetch`.
        var _rtStart = performance.now();
        var _rtInitiator = (init && typeof init._lumenInitiatorType === 'string')
            ? init._lumenInitiatorType : 'fetch';
        var method = (init && init.method) ? String(init.method).toUpperCase() :
                     (typeof input === 'object' && input.method ? input.method.toUpperCase() : 'GET');

        // Fetch §5.4 keepalive flag: request survives page unload (Beacon semantics).
        // Phase 0: accepted syntactically; detachment from page lifecycle is Phase 2.
        // network: keepalive — Phase 2: spawn detached thread, skip response body
        var keepalive = !!(init && init.keepalive);

        // Fetch Priority Hints (WHATWG Fetch §2.2.6): 'high'|'low'|'auto'.
        // Phase 0: parsed and normalised; network priority queue wiring is Phase 2.
        // network: priority queue — lumen-network Phase 2
        var _fetchPriority = (init && init.priority) ? String(init.priority) : 'auto';
        if (_fetchPriority !== 'high' && _fetchPriority !== 'low') { _fetchPriority = 'auto'; }

        // BUG-370: a Request now exposes the Body mixin, so `input.body` is a
        // ReadableStream — the raw string/FormData/bytes the caller handed the
        // constructor comes back through the shim-internal _lumen_body_source.
        var reqBody = (init && init.body !== undefined && init.body !== null) ? init.body
                    : _lumen_body_source(input);

        // BUG-749: author-заголовки запроса. До этого места канала для них не
        // было вовсе — `init.headers` разбирался ровно ради Content-Type, а
        // нативные привязки параметра под заголовки не имели, так что
        // Authorization / X-CSRF / Accept страница выставляла в объект Headers,
        // который никуда не уезжал.
        //
        // Fetch §5.5 шаг 32-33: заданный `init.headers` вытесняет список
        // самого Request-а целиком, иначе берётся список Request-а (он уже
        // построен под guard-ом 'request'). Готовый Headers всё равно
        // перезаливаем через guard: страница могла собрать его конструктором,
        // где guard === 'none' и Host/Cookie/Origin не отсеиваются.
        var authorHeaders = [];   // плоский [name, value, name, value, …]
        var hdrSrc = (init && init.headers !== undefined && init.headers !== null) ? init.headers
                   : ((typeof input === 'object' && input && input.headers) ? input.headers : null);
        if (hdrSrc) {
            _lumen_headers_new(hdrSrc, 'request').forEach(function(v, k) {
                authorHeaders.push(k); authorHeaders.push(v);
            });
        }

        // AbortSignal.timeout(ms) deadline is enforced natively (the JS thread is
        // parked in the synchronous fetch, so the JS setTimeout can't fire): a
        // positive _timeoutMs routes to the cancellable bridge whose deadline
        // thread tears the in-flight socket down (rc === 2 → TimeoutError).
        var _timeoutMs = (fetchSignal && typeof fetchSignal._timeoutMs === 'number' && fetchSignal._timeoutMs > 0) ? fetchSignal._timeoutMs : 0;
        // SRI integrity (W3C SRI §3.3.5), hoisted so both the sync and async paths verify.
        var integrity = (init && init.integrity) ? String(init.integrity)
                      : (typeof input === 'object' && input && input.integrity ? String(input.integrity) : '');
        // Body extraction is hoisted out of the dispatch branch so the async path can
        // reuse it. bodyBytes/contentType stay null when there is no request body.
        var hasBody = (reqBody !== null && reqBody !== undefined);
        var bodyBytes = null, contentType = null;
        if (hasBody) {
            if (reqBody instanceof FormData) {
                // Fetch spec §5.4: FormData body → multipart/form-data with random boundary.
                // Phase 0: deterministic boundary for testability; production boundary is random.
                var boundary = '----LumenFormBoundary' + Math.random().toString(36).slice(2, 10).toUpperCase();
                var multipartBytes = reqBody._toMultipart(boundary);
                bodyBytes = Array.from(multipartBytes);
                contentType = 'multipart/form-data; boundary=' + boundary;
            } else if (typeof reqBody === 'string') {
                bodyBytes = Array.from(new TextEncoder().encode(reqBody));
                contentType = 'text/plain;charset=UTF-8';
            } else if (reqBody instanceof Uint8Array || reqBody instanceof ArrayBuffer) {
                bodyBytes = reqBody instanceof Uint8Array ? Array.from(reqBody) : Array.from(new Uint8Array(reqBody));
                contentType = 'application/octet-stream';
            } else {
                var s = String(reqBody);
                bodyBytes = Array.from(new TextEncoder().encode(s));
                contentType = 'text/plain;charset=UTF-8';
            }
            // Caller may override Content-Type via headers. Читаем из уже
            // собранного author-списка: имена там нормализованы Headers-ом, и
            // это единственная форма, покрывающая все три способа задать
            // заголовки (Headers / массив пар / запись) сразу. Сам заголовок
            // при наличии тела уезжает как Content-Type тела (`RequestBody`),
            // а из author-списка отбрасывается на Rust-стороне — иначе ушёл бы
            // дублем.
            for (var ci = 0; ci + 1 < authorHeaders.length; ci += 2) {
                if (authorHeaders[ci] === 'content-type') { contentType = authorHeaders[ci + 1]; }
            }
        }

        // Async path: a live, non-timeout AbortSignal. Run the request on a worker
        // thread (via the _lumen_fetch_async_* bridges) and resolve/reject through a
        // setTimeout poll loop, so an AbortController.abort() fired *during* the
        // request flips the token and cancels the in-flight socket. Timeout signals
        // keep the synchronous-cancellable path below (already torn down natively).
        var useAsync = fetchSignal && !fetchSignal.aborted && !(_timeoutMs > 0);
        if (useAsync) {
            return new Promise(function(resolve, reject) {
                var handle = _lumen_fetch_async_start(url, method, contentType || '', bodyBytes || [], !!hasBody, authorHeaders);
                if (!handle) {
                    reject(new TypeError('fetch: network error for ' + url));
                    return;
                }
                var settled = false;
                function finish(fn) {
                    if (settled) return;
                    settled = true;
                    try { fetchSignal.removeEventListener('abort', onAbort); } catch (e) {}
                    fn();
                }
                function onAbort() { _lumen_fetch_async_abort(handle); }
                try { fetchSignal.addEventListener('abort', onAbort); } catch (e) {}
                function poll() {
                    if (settled) return;
                    var st = _lumen_fetch_async_poll(handle);
                    if (st === 0) { setTimeout(poll, 1); return; }
                    if (st === 3) {
                        finish(function() {
                            _lumen_fetch_async_free(handle);
                            reject(fetchSignal.reason !== undefined ? fetchSignal.reason : new DOMException('The operation was aborted', 'AbortError'));
                        });
                        return;
                    }
                    if (st === 2) {
                        finish(function() {
                            _lumen_fetch_async_free(handle);
                            reject(new TypeError('fetch: network error for ' + url));
                        });
                        return;
                    }
                    finish(function() {
                        if (!_lumen_fetch_async_commit(handle)) {
                            _lumen_fetch_async_free(handle);
                            reject(new TypeError('fetch: network error for ' + url));
                            return;
                        }
                        _lumen_fetch_async_free(handle);
                        var astatus = _lumen_fetch_get_status();
                        var astatusText = _lumen_fetch_get_status_text();
                        var arawHeaders = _lumen_fetch_get_headers();
                        if (integrity && !_lumen_check_sri_integrity(integrity)) {
                            reject(new TypeError('fetch: SRI integrity check failed for ' + url));
                            return;
                        }
                        var ahdrs = [];
                        for (var i = 0; i + 1 < arawHeaders.length; i += 2) { ahdrs.push([arawHeaders[i], arawHeaders[i + 1]]); }
                        _perf_rt_record_fetch(url, _rtInitiator, _rtStart, astatus);
                        resolve(_lumen_response_from_fetch_cache(astatus, astatusText, ahdrs, url));
                    });
                }
                setTimeout(poll, 0);
            });
        }

        var ok;
        if (hasBody) {
            if (_timeoutMs > 0) {
                var rc = _lumen_fetch_cancellable_with_body(url, method, contentType, bodyBytes, _timeoutMs, authorHeaders);
                if (rc === 2) { return Promise.reject(new DOMException('signal timed out', 'TimeoutError')); }
                ok = (rc === 0);
            } else {
                ok = _lumen_fetch_sync_with_body(url, method, contentType, bodyBytes, authorHeaders);
            }
        } else {
            if (_timeoutMs > 0) {
                var rc2 = _lumen_fetch_cancellable(url, method, _timeoutMs, authorHeaders);
                if (rc2 === 2) { return Promise.reject(new DOMException('signal timed out', 'TimeoutError')); }
                ok = (rc2 === 0);
            } else {
                ok = _lumen_fetch_sync(url, method, authorHeaders);
            }
        }

        if (!ok) {
            return Promise.reject(new TypeError('fetch: network error for ' + url));
        }
        var status = _lumen_fetch_get_status();
        var statusText = _lumen_fetch_get_status_text();
        var rawHeaders = _lumen_fetch_get_headers();
        // SRI integrity check (W3C SRI §3.3.5): verify body hash before exposing response.
        // _lumen_check_sri_integrity reads directly from Rust FetchCache — no JS copy needed.
        if (integrity && !_lumen_check_sri_integrity(integrity)) {
            return Promise.reject(new TypeError('fetch: SRI integrity check failed for ' + url));
        }
        var hdrs = [];
        for (var i = 0; i + 1 < rawHeaders.length; i += 2) {
            hdrs.push([rawHeaders[i], rawHeaders[i + 1]]);
        }
        _perf_rt_record_fetch(url, _rtInitiator, _rtStart, status);
        // Use lazy Rust-side chunk reading: body stays in Rust FetchCache until consumed.
        // This avoids copying large response bodies into JS memory at response construction.
        return Promise.resolve(_lumen_response_from_fetch_cache(status, statusText, hdrs, url));
    } catch(e) {
        return Promise.reject(e);
    }
}
// WebIDL §3.7: an operation on the global object is writable/enumerable/configurable.
Object.defineProperty(_lumen_fetch, 'name', { value: 'fetch', configurable: true });
Object.defineProperty(globalThis, 'fetch', {
    value: _lumen_fetch, writable: true, enumerable: true, configurable: true,
});

// ── WebSocket API (RFC 6455 §§3–7) ─────────────────────────────────────────
// Phase 0 model: synchronous connect; background recv thread queues events;
// JS polls via _lumen_pump_websockets(). Full async delivery (persistent JS
// runtime) is Phase 2+.

var _ws_instances = [];

function CloseEvent(code, reason, wasClean, init) {
    Event.call(this, 'close', init);
    this.code = code || 1000;
    this.reason = reason || '';
    this.wasClean = !!wasClean;
}
CloseEvent.prototype = Object.create(Event.prototype);
CloseEvent.prototype.constructor = CloseEvent;

function MessageEvent(data, init) {
    Event.call(this, 'message', init);
    this.data = data;
    this.origin = '';
    this.lastEventId = '';
}
MessageEvent.prototype = Object.create(Event.prototype);
MessageEvent.prototype.constructor = MessageEvent;

function _lumen_ws_fire(ws, ev) {
    ev.target = ws;
    var prop = 'on' + ev.type;
    if (typeof ws[prop] === 'function') { try { ws[prop](ev); } catch(e) { _lumen_report_exception(e); } }
    var arr = ws._listeners[ev.type];
    if (arr) { for (var i = 0; i < arr.length; i++) { try { arr[i](ev); } catch(e) { _lumen_report_exception(e); } } }
}

function _lumen_ws_pump_one(ws) {
    if (!ws._handle) return;
    var raw;
    while ((raw = _lumen_ws_poll(ws._handle)) !== null && raw !== undefined) {
        try {
            var ev = JSON.parse(raw);
            if (ev.t === 'open') {
                ws.readyState = 1;
                ws.protocol = ev.protocol || '';
                _lumen_ws_fire(ws, new Event('open', { isTrusted: true }));
            } else if (ev.t === 'msg') {
                if (ws.readyState !== 1) { continue; }
                var msgData;
                if (ev.bin) {
                    // Rust encodes binary payload as hex; decode to typed buffer.
                    var hex = ev.data;
                    var len = hex.length >>> 1;
                    var u8 = new Uint8Array(len);
                    for (var bi = 0; bi < len; bi++) {
                        u8[bi] = parseInt(hex.substr(bi * 2, 2), 16);
                    }
                    msgData = ws.binaryType === 'arraybuffer' ? u8.buffer : u8;
                } else {
                    msgData = ev.data;
                }
                _lumen_ws_fire(ws, new MessageEvent(msgData, { isTrusted: true }));
            } else if (ev.t === 'close') {
                ws.readyState = 3;
                // A received Close frame means the closing handshake completed → wasClean.
                _lumen_ws_fire(ws, new CloseEvent(ev.code, ev.reason, true, { isTrusted: true }));
                ws._handle = 0;
                break;
            } else if (ev.t === 'error') {
                var err = new Event('error', { isTrusted: true }); err.message = ev.msg;
                _lumen_ws_fire(ws, err);
                ws.readyState = 3; ws._handle = 0; break;
            }
        } catch(ignore) {}
    }
}

function _lumen_pump_websockets() {
    for (var i = _ws_instances.length - 1; i >= 0; i--) {
        _lumen_ws_pump_one(_ws_instances[i]);
        if (_ws_instances[i].readyState === 3) { _ws_instances.splice(i, 1); }
    }
}

function WebSocket(url, protocols) {
    this.url = String(url || '');
    this.readyState = 0;
    this.protocol = '';
    this.extensions = '';
    this.binaryType = 'blob';
    this.bufferedAmount = 0;
    this.onopen = null; this.onmessage = null;
    this.onclose = null; this.onerror = null;
    this._handle = 0;
    this._listeners = {};
    var self = this;
    var protoCsv = '';
    if (protocols != null) {
        if (Array.isArray(protocols)) {
            protoCsv = protocols.filter(function(p) { return typeof p === 'string' && p.length > 0; }).join(',');
        } else if (typeof protocols === 'string') {
            protoCsv = protocols;
        }
    }
    var h = _lumen_ws_connect(this.url, protoCsv);
    if (!h) {
        this.readyState = 3;
        setTimeout(function() {
            var e = new Event('error', { isTrusted: true }); e.message = 'WebSocket connection failed';
            _lumen_ws_fire(self, e);
            _lumen_ws_fire(self, new CloseEvent(1006, '', false, { isTrusted: true }));
        }, 0);
        return;
    }
    this._handle = h;
    _ws_instances.push(this);
    // Phase 0: no persistent event loop — caller must invoke _lumen_pump_websockets()
    // after setting onopen/onmessage to receive queued events.
}
// Application-data byte length used for bufferedAmount accounting (WHATWG WebSocket).
function _lumen_ws_bytelen(data) {
    if (typeof data === 'string') {
        return new TextEncoder().encode(data).length;
    }
    if (data instanceof ArrayBuffer) {
        return data.byteLength;
    }
    if (typeof data.byteLength === 'number') {
        return data.byteLength;
    }
    return new TextEncoder().encode(String(data)).length;
}

WebSocket.prototype.send = function(data) {
    if (this.readyState === 0) {
        throw new DOMException("Failed to execute 'send' on 'WebSocket': Still in CONNECTING state.", 'InvalidStateError');
    }
    var n = _lumen_ws_bytelen(data);
    if (this.readyState === 1) {
        if (typeof data === 'string') {
            _lumen_ws_send(this._handle, data);
        } else {
            _lumen_ws_send_bin(this._handle, data instanceof Uint8Array ? data : new Uint8Array(data));
        }
    } else if (this.readyState === 2 || this.readyState === 3) {
        // CLOSING/CLOSED: data is discarded but counted (WHATWG §the-websocket-interface send()).
        this.bufferedAmount += n;
    }
};
WebSocket.prototype.close = function(code, reason) {
    if (code !== undefined && code !== null) {
        if (code !== 1000 && (code < 3000 || code > 4999)) {
            throw new DOMException("Failed to execute 'close' on 'WebSocket': The code must be either 1000, or between 3000 and 4999.", 'InvalidAccessError');
        }
    }
    if (typeof reason === 'string' && new TextEncoder().encode(reason).length > 123) {
        throw new DOMException("Failed to execute 'close' on 'WebSocket': The close reason must not be greater than 123 UTF-8 bytes.", 'SyntaxError');
    }
    if (this.readyState === 2 || this.readyState === 3) {
        return;
    }
    this.readyState = 2;
    _lumen_ws_close(this._handle, typeof code === 'number' ? code : 1000, typeof reason === 'string' ? reason : '');
};
WebSocket.prototype.addEventListener = function(type, fn) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
WebSocket.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    var idx = this._listeners[type].indexOf(fn);
    if (idx >= 0) this._listeners[type].splice(idx, 1);
};
WebSocket.CONNECTING = 0; WebSocket.OPEN = 1;
WebSocket.CLOSING = 2;    WebSocket.CLOSED = 3;
WebSocket.prototype.CONNECTING = 0; WebSocket.prototype.OPEN = 1;
WebSocket.prototype.CLOSING = 2;    WebSocket.prototype.CLOSED = 3;

// ── Web Storage (localStorage / sessionStorage) ───────────────────────────────
// Spec: https://html.spec.whatwg.org/multipage/webstorage.html §8
// Both objects share the same factory; backing native functions differ per type.
//
// BUG-773: `Storage` is a WebIDL *legacy platform object*. Its named-property
// getter/setter/deleter make `storage.foo`, `storage['foo'] = x`,
// `delete storage.foo`, `'foo' in storage` and `Object.keys(storage)` exact
// synonyms of `getItem`/`setItem`/`removeItem`/enumerating the real keys — one
// operation reachable through two syntaxes. This used to be a plain object with
// five own methods, so a property-style write created an ordinary JS property
// on the wrapper: invisible to `getItem`/`length`/`key()`, absent from the
// persistent backend and therefore silently lost on the next page load — two
// unconnected planes of data on one object. The interceptor is a `Proxy`; the
// five operations and `length` live on a real, shared `Storage.prototype`,
// which is also what makes them *shadow* a same-named storage key.

function Storage() { throw new TypeError('Illegal constructor'); }

// proxy → its native accessor set. A WeakMap and not a field on the object
// itself: any own property would be page-visible and — worse — would shadow the
// storage key of the same name (see the visibility rule in the factory below).
var _lumen_storage_impl = new WeakMap();

function _lumen_storage_of(o) {
    var impl = _lumen_storage_impl.get(o);
    if (impl === undefined) throw new TypeError('Illegal invocation');
    return impl;
}

// WebIDL arity check: `localStorage.getItem()` must throw a TypeError rather
// than read the key spelled `undefined` (`missing_arguments.window.js`).
function _lumen_storage_arity(have, want, op) {
    if (have < want) {
        throw new TypeError('Storage.' + op + ': ' + want + ' argument' +
                            (want === 1 ? '' : 's') + ' required, but only ' +
                            have + ' present.');
    }
}

// Operations and `length` are writable + enumerable + configurable on the
// interface prototype, exactly as WebIDL prescribes — plain assignment already
// gives that shape.
Storage.prototype.key = function(n) {
    _lumen_storage_arity(arguments.length, 1, 'key');
    return _lumen_u2n(_lumen_storage_of(this).key(n >>> 0));
};
Storage.prototype.getItem = function(key) {
    _lumen_storage_arity(arguments.length, 1, 'getItem');
    return _lumen_u2n(_lumen_storage_of(this).get(String(key)));
};
Storage.prototype.setItem = function(key, value) {
    _lumen_storage_arity(arguments.length, 2, 'setItem');
    _lumen_storage_of(this).set(String(key), String(value));
};
Storage.prototype.removeItem = function(key) {
    _lumen_storage_arity(arguments.length, 1, 'removeItem');
    _lumen_storage_of(this).remove(String(key));
};
Storage.prototype.clear = function() { _lumen_storage_of(this).clear(); };
Object.defineProperty(Storage.prototype, 'length', {
    get: function() { return _lumen_storage_of(this).len(); },
    enumerable: true,
    configurable: true
});
if (typeof Symbol !== 'undefined' && Symbol.toStringTag) {
    Object.defineProperty(Storage.prototype, Symbol.toStringTag, {
        value: 'Storage', writable: false, enumerable: false, configurable: true
    });
}

function _lumen_make_storage(getLen, getKey, getItem, setItem, removeItem, clear) {
    // The object the Proxy wraps carries nothing but the prototype link and any
    // symbol-keyed property a page defines on it — WebIDL routes only *string*
    // names through the named-property hooks.
    var target = Object.create(Storage.prototype);
    var proxy;

    // WebIDL «named property visibility»: `Storage` carries no
    // [LegacyOverrideBuiltIns], so a name already answered by the object or
    // anywhere on its prototype chain hides the storage key of the same name.
    // That is what keeps `storage.length` and `storage.clear` meaning the
    // interface members after `setItem('length', …)`
    // (`storage_functions_not_overwritten.window.js`).
    function visible(prop) {
        return typeof prop === 'string'
            && !Reflect.has(target, prop)
            && getItem(prop) !== undefined;
    }

    proxy = new Proxy(target, {
        get: function(t, prop, receiver) {
            if (visible(prop)) return getItem(prop);
            return Reflect.get(t, prop, receiver);
        },
        set: function(t, prop, value, receiver) {
            // The named property *setter* runs for every string name, shadowed
            // or not — only reads are shadowed. `set.window.js` asserts a
            // same-named setter on the prototype is never invoked.
            if (typeof prop === 'string' && receiver === proxy) {
                setItem(prop, String(value));
                return true;
            }
            return Reflect.set(t, prop, value, receiver);
        },
        has: function(t, prop) {
            if (typeof prop === 'string' && getItem(prop) !== undefined) return true;
            return Reflect.has(t, prop);
        },
        deleteProperty: function(t, prop) {
            if (visible(prop)) { removeItem(prop); return true; }
            return Reflect.deleteProperty(t, prop);
        },
        getOwnPropertyDescriptor: function(t, prop) {
            if (visible(prop)) {
                return { value: getItem(prop), writable: true,
                         enumerable: true, configurable: true };
            }
            return Reflect.getOwnPropertyDescriptor(t, prop);
        },
        defineProperty: function(t, prop, desc) {
            if (typeof prop === 'string') {
                // WebIDL: a named setter accepts a data descriptor only, and
                // routes it into `setItem`. A `configurable: false` request
                // cannot be honoured through a Proxy (the invariant check
                // rejects a non-configurable descriptor for a key that is not a
                // real property of the target) — no spec text or WPT case asks
                // for that combination on `Storage`.
                if ('get' in desc || 'set' in desc) return false;
                if (!('value' in desc) && !('writable' in desc)) return false;
                setItem(prop, String(desc.value));
                return true;
            }
            return Reflect.defineProperty(t, prop, desc);
        },
        ownKeys: function(t) {
            var out = [], n = getLen();
            for (var i = 0; i < n; i++) {
                var k = getKey(i);
                if (k !== undefined) out.push(k);
            }
            // Symbol-keyed own properties must stay in the list or the Proxy
            // invariant check throws for any of them that is non-configurable.
            var own = Reflect.ownKeys(t);
            for (var j = 0; j < own.length; j++) {
                if (out.indexOf(own[j]) === -1) out.push(own[j]);
            }
            return out;
        },
        // WebIDL: a legacy platform object stays extensible and its prototype is
        // immutable.
        preventExtensions: function() { return false; },
        setPrototypeOf: function(t, proto) { return proto === Storage.prototype; }
    });

    _lumen_storage_impl.set(proxy, {
        len: getLen, key: getKey, get: getItem,
        set: setItem, remove: removeItem, clear: clear
    });
    return proxy;
}

var localStorage = _lumen_make_storage(
    _lumen_ls_length, _lumen_ls_key,
    _lumen_ls_get, _lumen_ls_set, _lumen_ls_remove, _lumen_ls_clear
);

var sessionStorage = _lumen_make_storage(
    _lumen_ss_length, _lumen_ss_key,
    _lumen_ss_get, _lumen_ss_set, _lumen_ss_remove, _lumen_ss_clear
);

// ── MutationObserver (WHATWG DOM §4.3.2) ─────────────────────────────────────
// Intercept existing mutation primitives to capture DOM change events.
// Wrapping happens here before the Element API (which calls these primitives)
// is built, so all subsequent setAttribute / innerHTML / appendChild calls
// automatically trigger observer delivery via queueMicrotask.

var _mo_observers = [];
var _mo_delivery_queued = false;

// True if `nid` is `ancestorNid` or a descendant of it (walks the parent chain
// via `_lumen_get_parent`). Scopes `subtree:true` observers to their own subtree
// (DOM §4.3.1) so a mutation elsewhere in the document — e.g. testharness.js's own
// results-table writes — is not misattributed to them (BUG-318).
function _lumen_mo_in_subtree(ancestorNid, nid) {
    var cur = nid;
    while (cur !== undefined && cur !== null) {
        if (cur === ancestorNid) return true;
        cur = _lumen_get_parent(cur);
    }
    return false;
}

function _mo_notify(nid, type, attrName, oldVal, addedNodeIds, removedNodeIds) {
    var hasObs = false;
    for (var oi = 0; oi < _mo_observers.length; oi++) {
        var obs = _mo_observers[oi];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var entry = obs._observations[ei];
            var tnid = entry.target && entry.target.__nid__;
            if (tnid === undefined) continue;
            var opts = entry.opts;
            // DOM §4.3.1: queue a record only if the mutated node is the observed
            // target, or — with subtree:true — a descendant of it. Without the
            // ancestry test, subtree observers captured every document mutation.
            if (opts.subtree) {
                if (!_lumen_mo_in_subtree(tnid, nid)) continue;
            } else if (tnid !== nid) {
                continue;
            }
            if (type === 'attributes' && !opts.attributes) continue;
            if (type === 'childList' && !opts.childList) continue;
            if (type === 'characterData' && !opts.characterData) continue;
            if (type === 'attributes' && opts.attributeFilter &&
                    opts.attributeFilter.indexOf(attrName) < 0) continue;
            var rec = {
                type: type,
                // DOM §4.3.3: target is the mutated node itself — for a subtree
                // observer this is the descendant, not the observation root.
                target: _lumen_make_element(nid),
                attributeName: attrName || null,
                attributeNamespace: null,
                oldValue: (type === 'attributes' && opts.attributeOldValue) ? oldVal :
                          (type === 'characterData' && opts.characterDataOldValue) ? oldVal : null,
                // addedNodes/removedNodes are node ids from the mutation primitives;
                // deliver them as (interned) node wrappers so `record.addedNodes[i]`
                // is `===` the same object scripts see via `firstChild` etc.
                addedNodes: (addedNodeIds || []).map(_lumen_make_element),
                removedNodes: (removedNodeIds || []).map(_lumen_make_element),
                nextSibling: null,
                previousSibling: null,
            };
            // BUG-317: records are MutationRecord instances (DOM §4.3.3).
            Object.setPrototypeOf(rec, MutationRecord.prototype);
            obs._records.push(rec);
            hasObs = true;
        }
    }
    if (hasObs && !_mo_delivery_queued) {
        _mo_delivery_queued = true;
        queueMicrotask(_lumen_flush_mutation_observers);
    }
}

// Synchronous delivery of all pending MutationObserver records.
// Called automatically via queueMicrotask after mutations.
// Can also be called directly by the shell after event dispatch (e.g. after
// _lumen_dispatch) to ensure observer callbacks run before the next paint.
function _lumen_flush_mutation_observers() {
    _mo_delivery_queued = false;
    for (var i = 0; i < _mo_observers.length; i++) {
        var o = _mo_observers[i];
        if (o._records.length === 0) continue;
        var recs = o._records;
        o._records = [];
        try { o._cb(recs, o); } catch(e) { _lumen_report_exception(e); }
    }
}

// BUG-827: nodes the PARSER wrote must queue childList records too. DOM §4.3
// hangs `queue a mutation record` off the insertion step itself, not off the
// API that triggered it, so a node the parser put in the tree owes an observer
// exactly the record `appendChild` owes it. The shell parses the whole document
// before the first script runs, so it replays the insertions it would have made
// here: `pairs` is a flat [parent, child, parent, child, …] list in tree order —
// the order a streaming parser would have inserted them — covering everything
// up to and including the `<script>` that is about to execute.
//
// Called from `crates/shell/src/main.rs` (`flush_parser_inserts`), which skips
// the call entirely while `_lumen_mo_observing()` is false: a record queued
// before anyone called `observe()` is dropped by the spec anyway, and building
// the argument for a whole document is not free.
function _lumen_mo_parser_inserted(pairs) {
    if (_mo_observers.length === 0) return;
    for (var i = 0; i + 1 < pairs.length; i += 2) {
        _mo_notify(pairs[i], 'childList', null, null, [pairs[i + 1]], []);
    }
}

// True once any MutationObserver exists (constructed, not necessarily observing).
// The shell's cheap gate for the call above — see `_lumen_mo_parser_inserted`.
function _lumen_mo_observing() {
    return _mo_observers.length > 0;
}

// Wrap _lumen_set_attr to intercept attribute mutations
var _orig_set_attr = _lumen_set_attr;
_lumen_set_attr = function(nid, name, value) {
    var old = (_mo_observers.length > 0) ? _lumen_get_attr(nid, name) : undefined;
    _orig_set_attr(nid, name, value);
    if (_mo_observers.length > 0) {
        _mo_notify(nid, 'attributes', String(name), old !== undefined ? old : null, null, null);
    }
};

// Wrap _lumen_set_inner_html to intercept childList mutations. BUG-368 fixed
// the setter to actually parse+replace children (was a no-op text stub before),
// so this wrapper now reports the real before/after child lists, mirroring
// _lumen_set_text_content's wrapper below.
var _orig_set_inner_html = _lumen_set_inner_html;
_lumen_set_inner_html = function(nid, html) {
    if (_mo_observers.length === 0) { _orig_set_inner_html(nid, html); return; }
    var before = _lumen_get_children(nid);
    _orig_set_inner_html(nid, html);
    var after = _lumen_get_children(nid);
    _mo_notify(nid, 'childList', null, null, after, before);
};

// Wrap _lumen_append_child to intercept childList mutations
var _orig_append_child = _lumen_append_child;
_lumen_append_child = function(parent, child) {
    _orig_append_child(parent, child);
    if (_mo_observers.length > 0) {
        _mo_notify(parent, 'childList', null, null, [child], []);
    }
};

// Wrap _lumen_remove_child to intercept childList mutations
var _orig_remove_child = _lumen_remove_child;
_lumen_remove_child = function(parent, child) {
    _orig_remove_child(parent, child);
    if (_mo_observers.length > 0) {
        _mo_notify(parent, 'childList', null, null, [], [child]);
    }
};

// Wrap _lumen_set_text_content to intercept mutations. DOM §4.9.1: setting
// textContent on an ELEMENT replaces all its children with (at most) one text
// node — a childList mutation (removedNodes = old children, addedNodes = new
// text node). On a text/CharacterData node it replaces the node's data — a
// characterData mutation (BUG-318).
var _orig_set_text_content = _lumen_set_text_content;
_lumen_set_text_content = function(nid, text) {
    if (_mo_observers.length === 0) { _orig_set_text_content(nid, text); return; }
    if (_lumen_is_text_node(nid) || _lumen_is_comment_node(nid)) {
        var old = _lumen_get_text_content(nid);
        _orig_set_text_content(nid, text);
        _mo_notify(nid, 'characterData', null, old, null, null);
    } else {
        var before = _lumen_get_children(nid);
        _orig_set_text_content(nid, text);
        var after = _lumen_get_children(nid);
        _mo_notify(nid, 'childList', null, null, after, before);
    }
};

function MutationObserver(callback) {
    this._cb = callback;
    this._observations = [];
    this._records = [];
    _mo_observers.push(this);
}
MutationObserver.prototype.observe = function(target, options) {
    if (!target || target.__nid__ === undefined) return;
    // DOM §4.3.1: observe() re-activates the observer. `disconnect()` removes it
    // from `_mo_observers`, so re-observing after a disconnect must re-register it
    // (only the constructor pushed before — BUG-318, WPT MutationObserver-disconnect).
    if (_mo_observers.indexOf(this) < 0) _mo_observers.push(this);
    var opts = options || {};
    var config = {
        target: target,
        opts: {
            childList:               !!opts.childList,
            attributes:              !!(opts.attributes || opts.attributeFilter || opts.attributeOldValue),
            characterData:           !!opts.characterData,
            subtree:                 !!opts.subtree,
            attributeOldValue:       !!opts.attributeOldValue,
            characterDataOldValue:   !!opts.characterDataOldValue,
            attributeFilter:         opts.attributeFilter ? opts.attributeFilter.slice() : null,
        },
    };
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) {
            this._observations[i] = config;
            return;
        }
    }
    this._observations.push(config);
};
MutationObserver.prototype.disconnect = function() {
    var idx = _mo_observers.indexOf(this);
    if (idx >= 0) _mo_observers.splice(idx, 1);
    this._observations = [];
    this._records = [];
};
MutationObserver.prototype.takeRecords = function() {
    var r = this._records;
    this._records = [];
    return r;
};

// DOM §4.3.3 MutationRecord — interface global so records delivered to a
// MutationObserver callback resolve `record instanceof MutationRecord`
// (BUG-317, same family as BUG-314). Not constructible from script; every
// record built in `_mo_notify` gets `MutationRecord.prototype` as its
// [[Prototype]]. The record literal's own data properties take precedence.
function MutationRecord() { throw new TypeError('Illegal constructor'); }

// ── ResizeObserver (W3C Resize Observer §5) ───────────────────────────────────
// Delivers size-change entries after layout; the shell calls
// _lumen_deliver_resize_observers() after each relayout.
//
// BUG-661 §1: the relayout path is not the only trigger. Resize Observer §3.2
// runs the observation loop as part of the update-the-rendering steps, so an
// observation that has never been reported must reach its callback on the next
// turn even when nothing in the document changed — the shell only relayouts on
// a dirty DOM/style, so a page that calls observe() and then sits still used to
// get no callback at all. _ro_schedule_initial() puts the pass on the event
// loop itself (a task in _lumen_timers, the BUG-842 pattern) so «guaranteed
// first delivery» no longer depends on someone else scheduling a reflow.

var _ro_observers = [];

// True while a first-delivery task is queued (the pass is idempotent, so one
// queued task covers any number of observe() calls made before it runs).
var _ro_initial_scheduled = false;
// Turns spent waiting for the first layout snapshot; see _ro_initial_pass.
var _ro_initial_attempts = 0;
var _RO_INITIAL_MAX_ATTEMPTS = 120;

function ResizeObserver(callback) {
    if (typeof callback !== 'function') {
        throw new TypeError('Failed to construct ResizeObserver: parameter 1 is not of type Function.');
    }
    this._cb = callback;
    this._observations = [];
    _ro_observers.push(this);
}
ResizeObserver.prototype.observe = function(target, options) {
    // Resize Observer §3.1: observe() takes an Element; anything else is a
    // TypeError (BUG-661 §2 — this used to return silently, so the WPT
    // «throw exception when observing non-element» assertion saw no throw).
    if (!target || typeof target !== 'object' || target.__nid__ === undefined || target.nodeType !== 1) {
        throw new TypeError('Failed to execute observe on ResizeObserver: parameter 1 is not of type Element.');
    }
    var box = (options && options.box) ? String(options.box) : 'content-box';
    for (var i = 0; i < this._observations.length; i++) {
        if (this._observations[i].target === target) {
            // §3.1 step 2: re-observing removes the existing observation and
            // adds a fresh one, so the target is reported again.
            this._observations[i].box = box;
            this._observations[i].lastW = -1;
            this._observations[i].lastH = -1;
            _ro_initial_attempts = 0;
            _ro_schedule_initial();
            return;
        }
    }
    this._observations.push({ target: target, box: box, lastW: -1, lastH: -1 });
    _ro_initial_attempts = 0;
    _ro_schedule_initial();
};
ResizeObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
ResizeObserver.prototype.disconnect = function() {
    var idx = _ro_observers.indexOf(this);
    if (idx >= 0) _ro_observers.splice(idx, 1);
    this._observations = [];
};

// Queue the first-delivery pass as an event-loop task. Written straight into
// _lumen_timers with nesting 0 rather than through setTimeout so the §8.6 4 ms
// clamp cannot delay it, and _lumen_request_wakeup makes the parked shell loop
// wake for it immediately.
function _ro_schedule_initial() {
    if (_ro_initial_scheduled) return;
    _ro_initial_scheduled = true;
    var deadline = _lumen_now_ms();
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _ro_initial_pass, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

function _ro_has_pending_initial() {
    for (var i = 0; i < _ro_observers.length; i++) {
        var obs = _ro_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            if (obs._observations[j].lastW < 0) return true;
        }
    }
    return false;
}

// True once the shell has published a layout snapshot for this document. An
// observe() from a parse-time script runs before the first push, when every
// element reads back «no box» — reporting 0×0 then would be a wrong first
// entry rather than a missing one, so the pass waits instead. Shared with the
// IntersectionObserver first-delivery pass (BUG-807), which waits on the same
// condition for the same reason.
function _lumen_layout_published() {
    try {
        var root = document.documentElement;
        return !!(root && _lumen_get_bounding_rect(root.__nid__));
    } catch (e) {
        return false;
    }
}

function _ro_initial_pass() {
    _ro_initial_scheduled = false;
    if (!_ro_has_pending_initial()) return;
    if (!_lumen_layout_published() && _ro_initial_attempts < _RO_INITIAL_MAX_ATTEMPTS) {
        _ro_initial_attempts++;
        _ro_schedule_initial();
        return;
    }
    _lumen_deliver_resize_observers();
}

// BUG-661 §4: detaching an observed element destroys its box, which is an
// observable size change even when the element is put back at the same size on
// the very same turn (the classic remove() + appendChild() pair no delivery
// pass ever sees in between). Called from the _lumen_remove_child wrapper
// installed below, while the child is still attached, so an observed
// descendant can be found by walking parents.
function _ro_invalidate_detached(childNid) {
    if (_ro_observers.length === 0) return;
    var touched = false;
    for (var i = 0; i < _ro_observers.length; i++) {
        var obs = _ro_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            var o = obs._observations[j];
            if (o.lastW < 0) continue;
            var cur = o.target.__nid__;
            while (cur !== null && cur !== undefined) {
                if (cur === childNid) {
                    o.lastW = -1; o.lastH = -1;
                    touched = true;
                    break;
                }
                cur = _lumen_u2n(_lumen_get_parent(cur));
            }
        }
    }
    if (touched) {
        _ro_initial_attempts = 0;
        _ro_schedule_initial();
    }
}

// Wrap the native once, by assignment rather than by a hoisted function
// declaration (which would overwrite the native before the alias is taken and
// recurse). Every removal in the shim — including the implicit one inside a
// reparenting appendChild/insertBefore — goes through this single binding.
var _lumen_remove_child_native = (typeof _lumen_remove_child === 'function') ? _lumen_remove_child : null;
if (_lumen_remove_child_native) {
    _lumen_remove_child = function(parentNid, childNid) {
        _ro_invalidate_detached(childNid);
        return _lumen_remove_child_native(parentNid, childNid);
    };
}

// BUG-661 §3: one length of a computed-style string in CSS px. Border widths
// are always published in px; a padding keeps its specified unit, so px/em/rem
// are resolved here and anything else (%, calc(), viewport units) falls back to
// 0 — the pre-BUG-661 behaviour of not subtracting it at all.
function _ro_len(value, fontPx, rootFontPx) {
    if (!value) return 0;
    var n = parseFloat(value);
    if (!isFinite(n)) return 0;
    if (value.slice(-3) === 'rem') return n * rootFontPx;
    if (value.slice(-2) === 'em') return n * fontPx;
    if (value.slice(-2) === 'px' || String(n) === value) return n;
    return 0;
}

// Content-box geometry of a border box: {w, h} of the content area plus the
// {x, y} offset of its top-left corner inside the border box, which is what
// Resize Observer §5.1 calls the entry's contentRect.
function _ro_content_geometry(nid, borderW, borderH) {
    var fontPx = parseFloat(_lumen_get_computed_style(nid, 'font-size')) || 16;
    var rootFontPx = 16;
    try {
        var root = document.documentElement;
        if (root) rootFontPx = parseFloat(_lumen_get_computed_style(root.__nid__, 'font-size')) || 16;
    } catch (e) { rootFontPx = 16; }
    var bl = _ro_len(_lumen_get_computed_style(nid, 'border-left-width'), fontPx, rootFontPx);
    var br = _ro_len(_lumen_get_computed_style(nid, 'border-right-width'), fontPx, rootFontPx);
    var bt = _ro_len(_lumen_get_computed_style(nid, 'border-top-width'), fontPx, rootFontPx);
    var bb = _ro_len(_lumen_get_computed_style(nid, 'border-bottom-width'), fontPx, rootFontPx);
    var pl = _ro_len(_lumen_get_computed_style(nid, 'padding-left'), fontPx, rootFontPx);
    var pr = _ro_len(_lumen_get_computed_style(nid, 'padding-right'), fontPx, rootFontPx);
    var pt = _ro_len(_lumen_get_computed_style(nid, 'padding-top'), fontPx, rootFontPx);
    var pb = _ro_len(_lumen_get_computed_style(nid, 'padding-bottom'), fontPx, rootFontPx);
    var w = borderW - bl - br - pl - pr;
    var h = borderH - bt - bb - pt - pb;
    return { w: w > 0 ? w : 0, h: h > 0 ? h : 0, x: pl, y: pt };
}

// CSS Contain L2 §4.1 (BUG-852) — deliver the shell's batch of
// `content-visibility: auto` state changes. `changes` is an array of
// `[node_index, skipped]` pairs in tree order, computed inside the shell's
// «update the rendering» step, so this call already *is* the queued task: the
// page's own script cannot be on the stack here.
//
// `_lumen_dispatch` sets no target of its own (BUG-873), and a page watching
// several elements through one listener has nothing else to tell them apart —
// so the target is filled in here, the way `_lumen_details_fire_toggle` does.
function _lumen_deliver_cv_state_changes(changes) {
    if (!changes || changes.length === 0) return;
    for (var i = 0; i < changes.length; i++) {
        var nid = changes[i][0];
        var evt = new ContentVisibilityAutoStateChangeEvent('contentvisibilityautostatechange', {
            bubbles: false, cancelable: false, isTrusted: true, skipped: !!changes[i][1]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
    }
}

function _lumen_deliver_resize_observers() {
    if (_ro_observers.length === 0) return;
    var dpr = (typeof devicePixelRatio === 'number' && devicePixelRatio > 0) ? devicePixelRatio : 1;
    for (var oi = 0; oi < _ro_observers.length; oi++) {
        var obs = _ro_observers[oi];
        var entries = [];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            var nid = o.target.__nid__;
            var rect = _lumen_get_bounding_rect(nid);
            // An element with no box (display:none, detached) has a zero-sized
            // box per §5.1 «calculate box size» — reported once, then it stops
            // differing from lastW/lastH.
            var bw = rect ? rect[2] : 0, bh = rect ? rect[3] : 0;
            // The content geometry costs nine computed-style reads, so a
            // border-box observation only pays for it once it has an entry.
            var cg = o.box === 'border-box' ? null : _ro_content_geometry(nid, bw, bh);
            var w = cg ? cg.w : bw;
            var h = cg ? cg.h : bh;
            if (o.lastW >= 0 && Math.abs(w - o.lastW) < 0.5 && Math.abs(h - o.lastH) < 0.5) continue;
            if (!cg) cg = _ro_content_geometry(nid, bw, bh);
            o.lastW = w; o.lastH = h;
            entries.push({
                target: o.target,
                contentRect: { x: cg.x, y: cg.y, width: cg.w, height: cg.h,
                               top: cg.y, left: cg.x, bottom: cg.y + cg.h, right: cg.x + cg.w },
                borderBoxSize:  [{ inlineSize: bw,   blockSize: bh }],
                contentBoxSize: [{ inlineSize: cg.w, blockSize: cg.h }],
                devicePixelContentBoxSize: [{ inlineSize: Math.round(cg.w * dpr), blockSize: Math.round(cg.h * dpr) }],
            });
        }
        if (entries.length > 0) {
            try { obs._cb(entries, obs); } catch(e) { _lumen_report_exception(e); }
        }
    }
}

// ── Canvas CSS resize tracking ────────────────────────────────────────────────
// When a canvas element's CSS layout dimensions change (detected after each
// relayout), the backing bitmap is scaled to the new size and a `resize` event
// is fired on the element (HTML LS §4.12.4 / Resize Observer integration).
//
// The shell calls _lumen_deliver_canvas_css_resize() after update_layout_rects,
// alongside _lumen_deliver_resize_observers and _lumen_deliver_intersection_observers.

// last CSS dimensions per canvas nid (as a string key), set on first observation.
var _canvas_css_dims = {};

function _lumen_deliver_canvas_css_resize() {
    for (var nid_str in _canvas2d_ctxs) {
        var nid = +nid_str;
        var rect = _lumen_get_bounding_rect(nid);
        if (!rect) continue;
        var w = (rect[2] + 0.5) | 0;  // round to integer CSS px
        var h = (rect[3] + 0.5) | 0;
        if (w < 1) w = 1;
        if (h < 1) h = 1;
        var prev = _canvas_css_dims[nid_str];
        if (!prev) {
            // first observation — record dims without firing event
            _canvas_css_dims[nid_str] = [w, h];
            continue;
        }
        if (prev[0] === w && prev[1] === h) continue;
        // CSS dimensions changed: scale pixel buffer and fire event
        _canvas_css_dims[nid_str] = [w, h];
        _lumen_canvas2d_scale_resize(nid, w, h);
        _lumen_dispatch(nid, new Event('resize'));
    }
}

// ── IntersectionObserver (WICG Intersection Observer §4) ─────────────────────
// Delivers intersection entries after layout; the shell calls
// _lumen_deliver_intersection_observers() after each relayout.
//
// BUG-807: the relayout path is not the only trigger. Intersection Observer
// §3.2 requires observe() itself to queue an initial notification, so the
// callback must arrive on its own shortly after the call, with nothing in the
// document changing. The shell only relayouts on a dirty DOM/style, so a page
// that observed a target and then sat still used to get no callback at all —
// any unrelated mutation elsewhere on the page delivered it instead, which is
// what made the «observe and wait» form hang rather than fail.
// _io_schedule_initial() puts the pass on the event loop itself, the same way
// ResizeObserver does since BUG-661.

var _io_observers = [];

// True while a first-delivery task is queued (the pass is idempotent, so one
// queued task covers any number of observe() calls made before it runs).
var _io_initial_scheduled = false;
// Turns spent waiting for the first layout snapshot; see _io_initial_pass.
var _io_initial_attempts = 0;
var _IO_INITIAL_MAX_ATTEMPTS = 120;

function IntersectionObserver(callback, options) {
    this._cb = callback;
    this._options = options || {};
    this._observations = [];
    _io_observers.push(this);
}
IntersectionObserver.prototype.observe = function(target) {
    if (!target || target.__nid__ === undefined) return;
    for (var i = 0; i < this._observations.length; i++) {
        // §3.2 step 1: observing an already-observed target is a no-op, so it
        // queues nothing either.
        if (this._observations[i].target === target) return;
    }
    // lastRatio = -1 means «never delivered» → first delivery always fires
    this._observations.push({ target: target, lastRatio: -1 });
    _io_initial_attempts = 0;
    _io_schedule_initial();
};
IntersectionObserver.prototype.unobserve = function(target) {
    this._observations = this._observations.filter(function(o) { return o.target !== target; });
};
IntersectionObserver.prototype.disconnect = function() {
    var idx = _io_observers.indexOf(this);
    if (idx >= 0) _io_observers.splice(idx, 1);
    this._observations = [];
};

// Queue the first-delivery pass as an event-loop task. Written straight into
// _lumen_timers with nesting 0 rather than through setTimeout so the §8.6 4 ms
// clamp cannot delay it, and _lumen_request_wakeup makes the parked shell loop
// wake for it immediately (the BUG-661/BUG-842 pattern).
function _io_schedule_initial() {
    if (_io_initial_scheduled) return;
    _io_initial_scheduled = true;
    var deadline = _lumen_now_ms();
    _lumen_timers.push({ id: _lumen_timer_seq++, fn: _io_initial_pass, deadline: deadline, interval: null, nesting: 0 });
    _lumen_request_wakeup(deadline);
}

function _io_has_pending_initial() {
    for (var i = 0; i < _io_observers.length; i++) {
        var obs = _io_observers[i];
        for (var j = 0; j < obs._observations.length; j++) {
            if (obs._observations[j].lastRatio < 0) return true;
        }
    }
    return false;
}

function _io_initial_pass() {
    _io_initial_scheduled = false;
    if (!_io_has_pending_initial()) return;
    // Before the first layout snapshot every target reads back «no box», which
    // would deliver a wrong not-intersecting first entry instead of a missing
    // one; the pass waits for the snapshot, bounded by _IO_INITIAL_MAX_ATTEMPTS
    // so a document that never gets one (dump modes) does not re-arm forever.
    if (!_lumen_layout_published() && _io_initial_attempts < _IO_INITIAL_MAX_ATTEMPTS) {
        _io_initial_attempts++;
        _io_schedule_initial();
        return;
    }
    _lumen_deliver_intersection_observers();
}

// Parse CSS margin shorthand into [top, right, bottom, left] px values.
// Only px units are supported; other units resolve to 0.
function _parse_root_margin(str) {
    if (!str) return [0, 0, 0, 0];
    var parts = str.trim().split(/\s+/);
    var vals = parts.map(function(p) {
        return p.indexOf('px') >= 0 ? parseFloat(p) : 0;
    });
    if (vals.length === 1) return [vals[0], vals[0], vals[0], vals[0]];
    if (vals.length === 2) return [vals[0], vals[1], vals[0], vals[1]];
    if (vals.length === 3) return [vals[0], vals[1], vals[2], vals[1]];
    return [vals[0], vals[1], vals[2], vals[3]];
}

function _lumen_deliver_intersection_observers() {
    if (_io_observers.length === 0) return;
    var vp = _lumen_get_viewport_size();
    var vpW = vp[0], vpH = vp[1];
    for (var oi = 0; oi < _io_observers.length; oi++) {
        var obs = _io_observers[oi];
        // Apply rootMargin to expand/contract the intersection root (viewport).
        // Positive margin expands outward; negative contracts inward.
        var rm = _parse_root_margin(obs._options.rootMargin);
        var rootTop = -rm[0], rootLeft = -rm[3];
        var rootRight = vpW + rm[1], rootBottom = vpH + rm[2];
        var t = obs._options.threshold !== undefined ? obs._options.threshold : 0;
        var thresholds = Array.isArray(t) ? t : [t];
        var entries = [];
        for (var ei = 0; ei < obs._observations.length; ei++) {
            var o = obs._observations[ei];
            var nid = o.target.__nid__;
            var rect = _lumen_get_bounding_rect(nid);
            // A target with no box (display:none, detached) still owes its
            // first notification: §3.2.1 reports such a target as an empty box
            // with isIntersecting false rather than as «no observation», and
            // §3.2 makes that notification unconditional. Skipping it here left
            // an observe-and-wait on such a target hanging forever even with
            // the pass now queued (BUG-807). A target that had a box and lost
            // one keeps the old skip — reporting *that* transition is the
            // delivery-content gap of BUG-626/627/628, not this bug.
            if (!rect && o.lastRatio >= 0) continue;
            var ex = rect ? rect[0] : 0, ey = rect ? rect[1] : 0;
            var ew = rect ? rect[2] : 0, eh = rect ? rect[3] : 0;
            var ix = Math.max(ex, rootLeft);
            var iy = Math.max(ey, rootTop);
            var iw = Math.max(0, Math.min(ex + ew, rootRight) - ix);
            var ih = Math.max(0, Math.min(ey + eh, rootBottom) - iy);
            var area = ew * eh;
            var ratio = area > 0 ? (iw * ih) / area : 0;
            var prev = o.lastRatio;
            var crossed = prev < 0; // first observation
            if (!crossed) {
                for (var ti = 0; ti < thresholds.length; ti++) {
                    var thr = thresholds[ti];
                    if ((prev < thr) !== (ratio < thr) ||
                        (prev === 0 && ratio > 0) || (prev > 0 && ratio === 0)) {
                        crossed = true;
                        break;
                    }
                }
            }
            if (!crossed) continue;
            o.lastRatio = ratio;
            entries.push({
                target: o.target,
                isIntersecting: ratio > 0,
                intersectionRatio: ratio,
                boundingClientRect: { x: ex, y: ey, width: ew, height: eh,
                                      top: ey, left: ex, bottom: ey+eh, right: ex+ew },
                intersectionRect:   { x: ix, y: iy, width: iw, height: ih,
                                      top: iy, left: ix, bottom: iy+ih, right: ix+iw },
                rootBounds: { x: rootLeft, y: rootTop,
                              width: rootRight - rootLeft, height: rootBottom - rootTop,
                              top: rootTop, left: rootLeft,
                              bottom: rootBottom, right: rootRight },
                time: typeof performance !== 'undefined' ? performance.now() : 0,
            });
        }
        if (entries.length > 0) {
            try { obs._cb(entries, obs); } catch(e) { _lumen_report_exception(e); }
        }
    }
}

// ── TreeWalker / NodeIterator / NodeFilter (DOM LS §4.4–4.5) ─────────────────
// NodeFilter constants (DOM LS §4.3).
var NodeFilter = {
    FILTER_ACCEPT:  1,
    FILTER_REJECT:  2,
    FILTER_SKIP:    3,
    SHOW_ALL:            0xFFFFFFFF,
    SHOW_ELEMENT:        0x1,
    SHOW_TEXT:           0x4,
    SHOW_CDATA_SECTION:  0x8,
    SHOW_COMMENT:        0x80,
    SHOW_DOCUMENT:       0x100,
    SHOW_DOCUMENT_TYPE:  0x200,
    SHOW_DOCUMENT_FRAGMENT: 0x400,
};

// Returns NodeFilter.FILTER_ACCEPT / SKIP / REJECT for a node nid given
// whatToShow bitmask and an optional filter callback or NodeFilter object.
function _nf_accepts(nid, whatToShow, filter) {
    // whatToShow bitmask check
    var nt = _lumen_is_text_node(nid) ? 3 : (_lumen_is_comment_node(nid) ? 8 : 1); // 1=element, 3=text, 8=comment
    var bit = (nt === 3) ? NodeFilter.SHOW_TEXT : (nt === 8 ? NodeFilter.SHOW_COMMENT : NodeFilter.SHOW_ELEMENT);
    if (!(whatToShow & bit)) return NodeFilter.FILTER_SKIP;
    if (!filter) return NodeFilter.FILTER_ACCEPT;
    var el = _lumen_make_element(nid);
    var result;
    if (typeof filter === 'function') {
        try { result = filter(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else if (filter && typeof filter.acceptNode === 'function') {
        try { result = filter.acceptNode(el); } catch(e) { result = NodeFilter.FILTER_REJECT; }
    } else {
        result = NodeFilter.FILTER_ACCEPT;
    }
    return result;
}

// Collects all nids in subtree of root in document order (pre-order, depth-first).
function _tw_subtree(root_nid) {
    var result = [];
    function visit(n) {
        result.push(n);
        var ch = _lumen_get_children(n);
        for (var i = 0; i < ch.length; i++) visit(ch[i]);
    }
    visit(root_nid);
    return result;
}

// ── TreeWalker (DOM LS §4.5) ─────────────────────────────────────────────────
function _TreeWalker(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this.currentNode = root;
}

_TreeWalker.prototype._root_nid = function() {
    return this.root && this.root.__nid__ !== undefined ? this.root.__nid__ : null;
};

_TreeWalker.prototype._cur_nid = function() {
    return this.currentNode && this.currentNode.__nid__ !== undefined ? this.currentNode.__nid__ : null;
};

// Returns the parent node within the root subtree, or null.
_TreeWalker.prototype.parentNode = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var p = _lumen_u2n(_lumen_get_parent(cur));
    while (p !== null) {
        if (p === root) { break; }
        var pp = _lumen_u2n(_lumen_get_parent(p));
        if (pp === null) { p = null; break; }
        p = pp;
    }
    if (p === null) return null;
    // Walk from root towards cur; find first ancestor that is accepted
    // Actually per spec: parentNode returns the nearest accepted ancestor in root subtree.
    var candidate = _lumen_u2n(_lumen_get_parent(cur));
    while (candidate !== null && candidate !== root) {
        var r = _nf_accepts(candidate, this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(candidate);
            return this.currentNode;
        }
        candidate = _lumen_u2n(_lumen_get_parent(candidate));
    }
    // Check root itself
    if (root !== null && cur !== root) {
        var rr = _nf_accepts(root, this.whatToShow, this.filter);
        if (rr === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = this.root;
            return this.currentNode;
        }
    }
    return null;
};

// Returns the first child of currentNode that passes the filter.
_TreeWalker.prototype.firstChild = function() {
    var children = _lumen_get_children(this._cur_nid() || 0);
    for (var i = 0; i < children.length; i++) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            // SKIP — recurse into its children (DOM spec §4.5.5)
            var saved = this.currentNode;
            this.currentNode = _lumen_make_element(children[i]);
            var found = this.firstChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the last child of currentNode that passes the filter.
_TreeWalker.prototype.lastChild = function() {
    var children = _lumen_get_children(this._cur_nid() || 0);
    for (var i = children.length - 1; i >= 0; i--) {
        var r = _nf_accepts(children[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(children[i]);
            return this.currentNode;
        }
        if (r !== NodeFilter.FILTER_REJECT) {
            var saved = this.currentNode;
            this.currentNode = _lumen_make_element(children[i]);
            var found = this.lastChild();
            if (found) return found;
            this.currentNode = saved;
        }
    }
    return null;
};

// Returns the previous sibling (in root subtree) of currentNode.
_TreeWalker.prototype.previousSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next sibling (in root subtree) of currentNode.
_TreeWalker.prototype.nextSibling = function() {
    var cur = this._cur_nid();
    var root = this._root_nid();
    if (cur === null || cur === root) return null;
    var pid = _lumen_u2n(_lumen_get_parent(cur));
    if (pid === null) return null;
    var sibs = _lumen_get_children(pid);
    var idx  = sibs.indexOf(cur);
    for (var i = idx + 1; i < sibs.length; i++) {
        var r = _nf_accepts(sibs[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(sibs[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the previous node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.previousNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (cur === null || cur === root) return null;
    var all = _tw_subtree(root);
    var idx = all.indexOf(cur);
    for (var i = idx - 1; i >= 0; i--) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// Returns the next node in document order (depth-first pre-order) that passes filter.
_TreeWalker.prototype.nextNode = function() {
    var root = this._root_nid();
    var cur  = this._cur_nid();
    if (root === null) return null;
    var all = _tw_subtree(root);
    var idx = cur !== null ? all.indexOf(cur) : -1;
    for (var i = idx + 1; i < all.length; i++) {
        var r = _nf_accepts(all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this.currentNode = _lumen_make_element(all[i]);
            return this.currentNode;
        }
    }
    return null;
};

// ── NodeIterator (DOM LS §4.4) ───────────────────────────────────────────────
// Simplified: maintains a reference position as an index into the flat subtree.
function _NodeIterator(root, whatToShow, filter) {
    this.root        = root;
    this.whatToShow  = whatToShow;
    this.filter      = filter;
    this._all        = null; // lazily built
    this._pos        = -1;   // -1 = before root
    this.referenceNode = root;
    this.pointerBeforeReferenceNode = true;
}

_NodeIterator.prototype._ensure = function() {
    if (this._all === null) {
        var root_nid = this.root && this.root.__nid__ !== undefined ? this.root.__nid__ : null;
        this._all = root_nid !== null ? _tw_subtree(root_nid) : [];
    }
};

// Returns the next accepted node (forward traversal).
_NodeIterator.prototype.nextNode = function() {
    this._ensure();
    for (var i = this._pos + 1; i < this._all.length; i++) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_element(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = false;
            return el;
        }
    }
    return null;
};

// Returns the previous accepted node (backward traversal).
_NodeIterator.prototype.previousNode = function() {
    this._ensure();
    for (var i = this._pos - 1; i >= 0; i--) {
        var r = _nf_accepts(this._all[i], this.whatToShow, this.filter);
        if (r === NodeFilter.FILTER_ACCEPT) {
            this._pos = i;
            var el = _lumen_make_element(this._all[i]);
            this.referenceNode = el;
            this.pointerBeforeReferenceNode = true;
            return el;
        }
    }
    return null;
};

// No-op per DOM LS §4.4.6.
_NodeIterator.prototype.detach = function() {};

// ── CaretPosition (CSSOM View §5.1) ──────────────────────────────────────────
// Returned by document.caretPositionFromPoint(). Phase 0: no layout hit-testing;
// always points to body at offset 0. getClientRects() returns an empty list.
function _CaretPosition(offsetNode, offset) {
    this.offsetNode = offsetNode;
    this.offset     = offset;
}
_CaretPosition.prototype.getClientRects = function() { return []; };

// ── window.matchMedia / MediaQueryList (CSS Media Queries L4 §4.2) ───────────
// Pure-JS shim on top of the native binding `_lumen_match_media` (parses + matches
// a media query against an ad-hoc MediaContext). The registry keeps strong refs
// while the user-side MQL is reachable; shell pumps changes via
// `_lumen_deliver_media_changes(w, h, dark, reducedMotion)` after each relayout
// or preference flip.
var _mqlRegistry = [];

function MediaQueryListEvent(type, init) {
    Event.call(this, type, init || {});
    this.media   = (init && init.media)   || '';
    this.matches = !!(init && init.matches);
}
MediaQueryListEvent.prototype = Object.create(Event.prototype);
MediaQueryListEvent.prototype.constructor = MediaQueryListEvent;

function MediaQueryList(media) {
    var vp = (typeof _lumen_get_viewport_size === 'function')
        ? _lumen_get_viewport_size() : [800, 600];
    this.media       = String(media == null ? '' : media);
    this.matches     = !!_lumen_match_media(this.media, vp[0], vp[1], false, false);
    this.onchange    = null;
    this._listeners  = [];
}
MediaQueryList.prototype.addListener = function(fn) {
    if (typeof fn === 'function') this.addEventListener('change', fn);
};
MediaQueryList.prototype.removeListener = function(fn) {
    if (typeof fn === 'function') this.removeEventListener('change', fn);
};
MediaQueryList.prototype.addEventListener = function(type, fn) {
    if (type === 'change' && typeof fn === 'function') {
        // Spec: ignore duplicate registrations of the same callback.
        for (var i = 0; i < this._listeners.length; i++) {
            if (this._listeners[i] === fn) return;
        }
        this._listeners.push(fn);
    }
};
MediaQueryList.prototype.removeEventListener = function(type, fn) {
    if (type === 'change') {
        var idx = this._listeners.indexOf(fn);
        if (idx !== -1) this._listeners.splice(idx, 1);
    }
};
MediaQueryList.prototype.dispatchEvent = function(ev) {
    if (!ev || ev.type !== 'change') return true;
    for (var i = 0; i < this._listeners.length; i++) {
        try { this._listeners[i].call(this, ev); } catch(e) { _lumen_report_exception(e); }
    }
    if (typeof this.onchange === 'function') {
        try { this.onchange.call(this, ev); } catch(e) { _lumen_report_exception(e); }
    }
    return !ev.defaultPrevented;
};
MediaQueryList.prototype._fire = function(matches) {
    this.matches = matches;
    var ev = new MediaQueryListEvent('change', { media: this.media, matches: matches });
    ev.target = this;
    ev.currentTarget = this;
    this.dispatchEvent(ev);
};

// Shell entry point: re-evaluate every registered MediaQueryList against the
// new context. Fires `change` only when `matches` actually flipped (spec).
function _lumen_deliver_media_changes(w, h, dark, reducedMotion) {
    var darkB = !!dark;
    var rmB   = !!reducedMotion;
    for (var i = 0; i < _mqlRegistry.length; i++) {
        var mql = _mqlRegistry[i];
        if (!mql) continue;
        var newM = !!_lumen_match_media(mql.media, w, h, darkB, rmB);
        if (mql.matches !== newM) mql._fire(newM);
    }
}

// ── postMessage (HTML LS §7.7.4) ─────────────────────────────────────────────
var _message_listeners = [];

// ── Window load / DOMContentLoaded / visibilitychange / error listener arrays ──
var _load_listeners = [];
var _domcontentloaded_win_listeners = [];
var _visibilitychange_listeners = [];
var _error_listeners = [];
var _other_win_listeners = {};

var window = {
    history: history,
    onpopstate: null,
    onhashchange: null,
    onmessage: null,
    onpageshow: null,
    onpagehide: null,
    // BUG-834: declared for the same reason as `onscroll` below — `'onunload' in
    // window` / `'onbeforeunload' in window` is the feature test a page runs
    // before deciding whether it may hook the unload sequence. Assignment
    // already worked (`_lumen_bfcache_blocked` and the two dispatch loops in
    // `_lumen_unload_document`/`_lumen_fire_beforeunload` read the property
    // directly), a bare `in` check did not.
    onunload: null,
    onbeforeunload: null,
    onload: null,
    // BUG-702: present so `'onunhandledrejection' in window` is true, which is the
    // other half of the feature test libraries run for promise-rejection support.
    // Dispatched via `_lumen_dispatch_unhandled_rejection` — see BUG-716.
    onunhandledrejection: null,
    onrejectionhandled: null,
    // BUG-822: declared so `'onscroll' in window` / `'onscrollend' in window`
    // answer true — the feature test a page runs before deciding whether it may
    // wait for the end of a scroll. Assignment already worked without them
    // (`dispatchEvent`'s generic branch reads `window['on' + type]` at dispatch
    // time), but a bare `in` check did not; declaring the property is all that
    // branch needs, no dispatch-side change.
    onscroll: null,
    onscrollend: null,
    // `location` is deliberately absent here: it is defined directly on
    // `globalThis` as an unforgeable accessor (see `Location` above), and
    // `window` becomes `globalThis` at the end of this shim, so `window.location`
    // resolves to that accessor. Listing it here would make the window→globalThis
    // copy loop below re-ASSIGN it (`globalThis[k] = d.value`, the plain-value
    // branch), which now runs the navigating setter and would fire a spurious
    // full navigation to the current URL on every page load.
    navigator: navigator,
    alert: alert,
    confirm: confirm,
    prompt: prompt,
    print: print,
    setTimeout: setTimeout,
    setInterval: setInterval,
    clearTimeout: clearTimeout,
    clearInterval: clearInterval,
    requestAnimationFrame: requestAnimationFrame,
    cancelAnimationFrame: cancelAnimationFrame,
    _lumen_run_raf_callbacks: _lumen_run_raf_callbacks,
    EventSource: EventSource,
    WebSocket: WebSocket,
    CloseEvent: CloseEvent,
    MessageEvent: MessageEvent,
    _lumen_pump_websockets: _lumen_pump_websockets,
    _lumen_pump_sse: _lumen_pump_sse,
    caches: caches,
    document: document,
    console: console,
    fetch: fetch,
    Request: Request,
    Response: Response,
    Headers: Headers,
    AbortController: AbortController,
    AbortSignal: AbortSignal,
    ReadableStream: ReadableStream,
    WritableStream: WritableStream,
    TransformStream: TransformStream,
    ReadableStreamDefaultReader: ReadableStreamDefaultReader,
    WritableStreamDefaultWriter: WritableStreamDefaultWriter,
    TextDecoderStream: TextDecoderStream,
    TextEncoderStream: TextEncoderStream,
    CompressionStream: CompressionStream,
    DecompressionStream: DecompressionStream,
    ByteLengthQueuingStrategy: ByteLengthQueuingStrategy,
    CountQueuingStrategy: CountQueuingStrategy,
    FormData: FormData,
    TextEncoder: TextEncoder,
    TextDecoder: TextDecoder,
    localStorage: localStorage,
    sessionStorage: sessionStorage,
    _lumen_dispatch_composition: _lumen_dispatch_composition,
    _lumen_dispatch_mouse_event:        _lumen_dispatch_mouse_event,
    _lumen_dispatch_locked_mousemove:   _lumen_dispatch_locked_mousemove,
    _lumen_dispatch_pointer_event:      _lumen_dispatch_pointer_event,
    _lumen_dispatch_pointer_move_coalesced: _lumen_dispatch_pointer_move_coalesced,
    _lumen_dispatch_capture_event:      _lumen_dispatch_capture_event,
    _lumen_dispatch_key_event:     _lumen_dispatch_key_event,
    _lumen_set_field_value:        _lumen_set_field_value,
    _lumen_dispatch_rich:          _lumen_dispatch_rich,
    _lumen_set_ime_target: _lumen_set_ime_target,
    _lumen_fire_page_lifecycle: _lumen_fire_page_lifecycle,
    addEventListener: function(type, fn) {
        if (typeof fn !== 'function') return;
        if (type === 'popstate') {
            _popstate_listeners.push(fn);
        } else if (type === 'pageshow') {
            _pageshow_listeners.push(fn);
        } else if (type === 'pagehide') {
            _pagehide_listeners.push(fn);
        } else if (type === 'message') {
            _message_listeners.push(fn);
        } else if (type === 'load') {
            if (_doc_ready_state === 'complete') {
                // already loaded — fire async per spec
                queueMicrotask(function() {
                    try { fn(new Event('load', { bubbles: false })); } catch(e) { _lumen_report_exception(e); }
                });
            } else {
                _load_listeners.push(fn);
            }
        } else if (type === 'DOMContentLoaded') {
            if (_doc_ready_state !== 'loading') {
                queueMicrotask(function() {
                    try { fn(new Event('DOMContentLoaded', { bubbles: true })); } catch(e) { _lumen_report_exception(e); }
                });
            } else {
                _domcontentloaded_win_listeners.push(fn);
            }
        } else if (type === 'visibilitychange') {
            _visibilitychange_listeners.push(fn);
        } else if (type === 'error') {
            _error_listeners.push(fn);
        } else {
            if (!_other_win_listeners[type]) _other_win_listeners[type] = [];
            _other_win_listeners[type].push(fn);
        }
    },
    removeEventListener: function(type, fn) {
        var arr;
        if (type === 'popstate') arr = _popstate_listeners;
        else if (type === 'pageshow') arr = _pageshow_listeners;
        else if (type === 'pagehide') arr = _pagehide_listeners;
        else if (type === 'message') arr = _message_listeners;
        else if (type === 'load') arr = _load_listeners;
        else if (type === 'DOMContentLoaded') arr = _domcontentloaded_win_listeners;
        else if (type === 'visibilitychange') arr = _visibilitychange_listeners;
        else if (type === 'error') arr = _error_listeners;
        else arr = _other_win_listeners[type];
        if (!arr) return;
        var idx = arr.indexOf(fn);
        if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent: function(evt) {
        if (!evt || !evt.type) return true;
        var arr;
        if (evt.type === 'load') {
            arr = _load_listeners.slice();
            for (var i = 0; i < arr.length; i++) {
                try { arr[i].call(window, evt); } catch(e) { _lumen_report_exception(e); }
            }
            if (typeof window.onload === 'function') {
                try { window.onload.call(window, evt); } catch(e) { _lumen_report_exception(e); }
            }
        } else if (evt.type === 'error') {
            // Deliberately NOT routed through `_lumen_report_exception` here: this
            // branch runs *inside* that function's own dispatch (`_lumen_report_exception`
            // -> `window.dispatchEvent(new ErrorEvent(...))` -> here), so reporting
            // an exception thrown by an 'error' listener itself would recurse.
            arr = _error_listeners.slice();
            for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) {} }
            if (typeof window.onerror === 'function') {
                // BUG-591: `onerror`'s IDL type is OnErrorEventHandler, not the
                // plain EventHandler every other on<type> attribute uses -- its
                // "internal raw handler" is called with 5 positional arguments
                // (message, source, lineno, colno, error) instead of the Event
                // object, but only when the event genuinely is an ErrorEvent;
                // `window.dispatchEvent(new Event('error'))` still passes the
                // Event itself (single argument) to the same handler.
                var isErrorEvt = (evt instanceof ErrorEvent);
                var rv;
                try {
                    rv = isErrorEvt
                        ? window.onerror.call(window, evt.message, evt.filename, evt.lineno, evt.colno, evt.error)
                        : window.onerror.call(window, evt);
                } catch (e) { rv = undefined; }
                // Returning a truthy value from the ErrorEvent-flavoured call
                // cancels the event's default action (HTML LS "the event
                // handler processing algorithm", error-event special case).
                if (isErrorEvt && rv) { evt.preventDefault(); }
            }
        } else {
            arr = _other_win_listeners[evt.type];
            if (arr) {
                arr = arr.slice();
                for (var i = 0; i < arr.length; i++) { try { arr[i].call(window, evt); } catch(e) { _lumen_report_exception(e); } }
            }
            // BUG-392: the `on<type>` IDL attribute fires after the explicit
            // listeners, same ordering as the 'load'/'error' branches above and
            // as `_lumen_dispatch` does for elements. Generic by design: every
            // Window event handler attribute declared as a plain nullable
            // property (`onpopstate`, `ongamepadconnected`, …) is reached this
            // way, so a new one needs no dispatch-side change. No double-fire:
            // `load`/`error` are handled by the branches above, and the engine's
            // own delivery of `hashchange`/`popstate`/`message` calls the
            // handler directly instead of going through `dispatchEvent`.
            var onFn = window['on' + evt.type];
            if (typeof onFn === 'function') { try { onFn.call(window, evt); } catch(e) { _lumen_report_exception(e); } }
        }
        return !evt.defaultPrevented;
    },
    /// postMessage (HTML LS §7.7.4): dispatch a MessageEvent to this window.
    /// targetOrigin '*' → always deliver; '/' → same-origin only;
    /// any other string → must equal location.origin.
    postMessage: function(message, targetOrigin) {
        var origin = location.origin;
        if (targetOrigin !== '*') {
            var target = (targetOrigin === '/') ? origin : String(targetOrigin);
            if (target !== origin) return;
        }
        var ev = new MessageEvent(message);
        ev.origin = origin;
        ev.source = window;
        // Spec §7.7.4 step 5: dispatch as a task (asynchronously).
        setTimeout(function() {
            if (typeof window.onmessage === 'function') {
                try { window.onmessage(ev); } catch(e) { _lumen_report_exception(e); }
            }
            for (var i = 0; i < _message_listeners.length; i++) {
                try { _message_listeners[i](ev); } catch(e) { _lumen_report_exception(e); }
            }
        }, 0);
    },
};

// BUG-480 срез 4: доставка кросс-фреймового message в ЭТО окно из бриджа
// фреймов (frame_bridge::_lumen_frame_pump_messages). Данные уже разобраны,
// source — фасад окна отправителя или null. Тот же порядок, что у локального
// window.postMessage выше: сначала onmessage, затем addEventListener('message').
globalThis._lumen_deliver_frame_message = function(data, origin, source) {
    var ev = new MessageEvent(data);
    ev.origin = origin || '';
    if (source !== null && source !== undefined) ev.source = source;
    if (typeof window.onmessage === 'function') {
        try { window.onmessage(ev); } catch(e) {}
    }
    for (var i = 0; i < _message_listeners.length; i++) {
        try { _message_listeners[i](ev); } catch(e) {}
    }
};

// BUG-480 срез 6: синтетический click() из родительского фасада iframe
// (frame_bridge::_lumen_frame_pump_messages вызывает на тике ЭТОГО контекста).
// Исполняется в этом изоляте, поэтому событие достаётся слушателям этого
// документа; сама последовательность — та же бездоверительная семантика
// click(), что у HTMLElement.prototype.click (общая _lumen_perform_click,
// объявление поднимается хостингом в пределах одного скрипта шима).
globalThis._lumen_deliver_frame_click = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    _lumen_perform_click(nid);
};

// BUG-480 срез 7: focus() из чужого фасада iframe — семантика
// HTMLElement.prototype.focus, исполненная В ЭТОМ изоляте: focusability-гейт и
// _lumen_focus_update (blur/focusout на прежде сфокусированном, focus/focusin
// на новом). Два отклонения, оба задокументированы в BUG-480: (1) БЕЗ
// `_lumen_request_focus` — очередь фокус-запросов рантайма фрейма шеллом пока
// не дренируется (фреймы не рендерятся), запрос там только копился бы;
// `preventScroll` переносится конвертом, но игнорируется — layout у фреймов
// нулевой, скроллить нечего.
globalThis._lumen_deliver_frame_focus = function(nid, preventScroll) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (!_lumen_is_focusable(nid)) return;
    _lumen_focus_update(nid);
};
// Парный blur(): no-op для не сфокусированного элемента, как у
// HTMLElement.prototype.blur; тоже без `_lumen_request_blur`.
globalThis._lumen_deliver_frame_blur = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (_lumen_last_focused_nid !== _lumen_nearest_element_nid(nid)) return;
    _lumen_focus_update(-1);
};
// Срез 7: произвольное событие из чужого фасада dispatchEvent(). Точная копия
// последовательности собственного el.dispatchEvent этого шима (см. фабрику
// живых элементов): снимок Event строится заново в этом изоляте, диспатчится
// через _lumen_dispatch (слушатели цели + on<type>), а недоверенный 'click'
// без preventDefault запускает активационное поведение (BUG-439).
globalThis._lumen_deliver_frame_dom_event = function(nid, env) {
    if (typeof nid !== 'number' || nid < 0 || !env) return;
    var type = typeof env.type === 'string' ? env.type : '';
    if (!type) return;
    var init = { bubbles: !!env.bubbles, cancelable: !!env.cancelable };
    var ev = new Event(type, init);
    if (env.detail !== null && env.detail !== undefined && typeof CustomEvent === 'function') {
        ev = new CustomEvent(type, { bubbles: !!env.bubbles, cancelable: !!env.cancelable, detail: env.detail });
    }
    ev.target = _lumen_make_element(nid);
    ev.currentTarget = ev.target;
    var notCancelled = _lumen_dispatch(nid, ev);
    if (notCancelled && ev.isTrusted === false && type === 'click') {
        var at = _lumen_activation_target(nid);
        if (at !== -1) {
            _lumen_run_activation_behavior(at, (at === nid)
                ? _lumen_make_element(nid) : _lumen_make_element(at));
        }
    }
};

// BUG-480 срез 8: `<script>`, вставленный в под-документ из чужого фасада
// (appendChild/insertBefore через contentDocument). Мост ставит конверт
// RunScript, этот хук на тике ЭТОГО контекста исполняет элемент штатной
// `_lumen_script_prepare` — тем же путём, что скрипт, созданный самим
// ребёнком: гейт типа (data-блок не исполняется), пустой src → error,
// внешний src → fetch, инлайн-классика синхронно с document.currentScript.
//
// «Already started» — per element (HTML LS §4.12.1): повторная вставка
// исполненного скрипта не перезапускает его. Отсоединённый до доставки
// конверт теряется БЕЗ пометки — как у главного документа, где preparation
// ждёт первого connected-вставки.
//
// Срез 9: флаг ставится только когда подготовка РЕАЛЬНО началась по спеке —
// шаг «set el's already started to true» стоит после гейтов «дата-блок» и
// «нет src, тело пусто», поэтому оба эти исхода оставляют элемент
// непомеченным. Иначе поздний setAttribute('src', …) на вставленном пустым
// скрипте (каноничное `s.src = url` после appendChild) навсегда глотался бы
// первой доставкой. Предикат зеркалит ранние выходы `_lumen_script_prepare`.
function _lumen_frame_script_will_start(nid) {
    var type = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    var isModule = type !== null && String(type).trim().toLowerCase() === 'module';
    // Дата-блок никогда не становится скриптом.
    if (!isModule && !_lumen_is_classic_script_type(type)) return false;
    // ЛЮБОЙ src начинает элемент: непустой — загрузкой, пустой/пробельный —
    // error-таском (спека ставит already started до обеих веток).
    var src = _lumen_u2n(_lumen_get_attr(nid, 'src'));
    if (src !== null) return true;
    var body = _lumen_u2n(_lumen_get_text_content(nid));
    return body !== null && String(body).trim() !== '';
}
var _lumen_frame_scripts_started = {};
globalThis._lumen_deliver_frame_run_script = function(nid) {
    if (typeof nid !== 'number' || nid < 0) return;
    if (!_lumen_resource_is_connected(nid)) return;
    // Уже начавшийся — спековый ранний выход №1; не начинающийся вовсе
    // (дата-блок / пусто без src) — выход до пометки, чтобы поздний
    // setAttribute('src') получил свою доставку.
    if (_lumen_frame_scripts_started[nid] === 1) return;
    if (!_lumen_frame_script_will_start(nid)) return;
    _lumen_frame_scripts_started[nid] = 1;
    _lumen_script_prepare(nid);
};

// _lumen_dispatch_unhandled_rejection (BUG-716) — Rust→JS bridge for
// `v8::Isolate::set_promise_reject_callback` (`v8_runtime.rs`). Called
// directly with the *live* `promise`/`reason` values, never through
// `eval`/JSON — an `Error` reason must keep its class and `.stack`, and
// `PromiseRejectionEvent.promise` must be the actual settled promise per
// HTML LS §8.1.7.5. `type` is 'unhandledrejection' (cancelable — its default
// action is a console report, which the Rust side suppresses when this
// returns `true`) or 'rejectionhandled' (not cancelable, no default action).
function _lumen_dispatch_unhandled_rejection(type, promise, reason) {
    var evt = new PromiseRejectionEvent(type, {
        promise: promise,
        reason: reason,
        cancelable: type === 'unhandledrejection',
        bubbles: false,
    });
    window.dispatchEvent(evt);
    return !!evt.defaultPrevented;
}

// ── queueMicrotask (HTML LS §8.1.4.4) ────────────────────────────────────────
// Schedules `fn` as a microtask; implemented via a resolved Promise chain, which
// V8 drains between tasks (same semantics as spec §8.1.4.2 microtask queue).
//
// BUG-702: the resolve/then pair is captured HERE, at shim-install time, while
// `Promise` is still V8's own, and is never re-read from the global afterwards.
// A page is free to replace `window.Promise` with its own implementation — core-js
// does exactly that whenever its feature detection rejects the native one — and
// such a polyfill schedules its reaction jobs through the host `queueMicrotask`.
// Reading `Promise` from the global here would then close the loop: polyfill
// resolve -> queueMicrotask -> polyfill Promise.resolve().then() -> polyfill
// resolve -> ... an unbounded recursion that spins the engine at 100% CPU
// forever (the tbank.ru hang).
var queueMicrotask = (function() {
    var _nativeResolve = Promise.resolve.bind(Promise);
    var _nativeThen = Promise.prototype.then;
    return function queueMicrotask(fn) {
        if (typeof fn !== 'function') throw new TypeError('queueMicrotask: argument must be a function');
        // §8.1.4.4 step 3 reports an uncaught exception from `fn`, it does not
        // reject a promise -- BUG-591 (before this, an uncaught throw here
        // surfaced as an unhandledrejection on the untouched wrapper promise
        // below, the wrong event entirely: queue-microtask-exceptions.any.html
        // waits on 'error', never on 'unhandledrejection').
        _nativeThen.call(_nativeResolve(), function() {
            try { fn(); } catch (e) { _lumen_report_exception(e); }
        });
    };
})();

