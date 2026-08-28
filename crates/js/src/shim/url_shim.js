
// ── URLSearchParams (WHATWG URL §5) ──────────────────────────────────────────
function URLSearchParams(init) {
    // `_p` (the pair list) and `_url` (the URL this object is the `searchParams`
    // of, or null) are implementation slots, not web-visible properties — a
    // plain assignment would make them enumerable own properties and leak into
    // any `for…in` / `Object.keys` the page runs over the object (BUG-375 §5).
    Object.defineProperty(this, '_p',   { value: [],   writable: true, enumerable: false, configurable: true });
    Object.defineProperty(this, '_url', { value: null, writable: true, enumerable: false, configurable: true });
    if (init === undefined || init === null) return;
    if (typeof init === 'string') {
        this._p = _usp_parse(init);
    } else if (Array.isArray(init)) {
        for (var i = 0; i < init.length; i++) {
            var entry = init[i];
            if (!Array.isArray(entry) || entry.length < 2)
                throw new TypeError('URLSearchParams: each sequence entry must have 2 items');
            this._p.push([String(entry[0]), String(entry[1])]);
        }
    } else if (typeof init === 'object') {
        var keys = Object.keys(init);
        for (var i = 0; i < keys.length; i++) {
            this._p.push([String(keys[i]), String(init[keys[i]])]);
        }
    }
}
// Application/x-www-form-urlencoded parse (WHATWG URL §5.1), shared by the
// `URLSearchParams` constructor and by the URL -> searchParams resync that
// follows every mutation of a URL's query component.
function _usp_parse(init) {
    var s = String(init);
    if (s.length > 0 && s[0] === '?') s = s.slice(1);
    var out = [];
    if (!s) return out;
    var pairs = s.split('&');
    for (var i = 0; i < pairs.length; i++) {
        var pair = pairs[i];
        if (!pair) continue;
        var eq = pair.indexOf('=');
        var k = eq >= 0 ? pair.slice(0, eq) : pair;
        var v = eq >= 0 ? pair.slice(eq + 1) : '';
        out.push([_usp_decode(k), _usp_decode(v)]);
    }
    return out;
}
// Push a mutated pair list back into the owning URL (WHATWG URL §5.2 `update`
// steps). Without this the object returned by `url.searchParams` and `url.href`
// drift apart permanently after the first `set`/`append`/`delete`/`sort`.
function _usp_update(sp) {
    if (!sp._url) return;
    var q = sp.toString();
    sp._url._search = q ? '?' + q : '';
    _lumen_url_reserialize(sp._url, true);
}
function _usp_decode(s) {
    try { return decodeURIComponent(s.split('+').join(' ')); } catch(e) { return s; }
}
function _usp_encode(s) {
    // application/x-www-form-urlencoded percent-encode set (WHATWG URL §5.1 step 2)
    return encodeURIComponent(s).replace(/%20/g, '+');
}
URLSearchParams.prototype.append = function(name, value) {
    this._p.push([String(name), String(value)]);
    _usp_update(this);
};
URLSearchParams.prototype.delete = function(name) {
    var n = String(name);
    this._p = this._p.filter(function(e) { return e[0] !== n; });
    _usp_update(this);
};
URLSearchParams.prototype.get = function(name) {
    var n = String(name);
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) return this._p[i][1]; }
    return null;
};
URLSearchParams.prototype.getAll = function(name) {
    var n = String(name); var out = [];
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) out.push(this._p[i][1]); }
    return out;
};
URLSearchParams.prototype.has = function(name) {
    var n = String(name);
    for (var i = 0; i < this._p.length; i++) { if (this._p[i][0] === n) return true; }
    return false;
};
URLSearchParams.prototype.set = function(name, value) {
    var n = String(name), v = String(value), found = false;
    this._p = this._p.filter(function(e) {
        if (e[0] !== n) return true;
        if (!found) { found = true; e[1] = v; return true; }
        return false;
    });
    if (!found) this._p.push([n, v]);
    _usp_update(this);
};
URLSearchParams.prototype.sort = function() {
    this._p.sort(function(a, b) { return a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0; });
    _usp_update(this);
};
URLSearchParams.prototype.toString = function() {
    return this._p.map(function(e) { return _usp_encode(e[0]) + '=' + _usp_encode(e[1]); }).join('&');
};
URLSearchParams.prototype.forEach = function(cb, thisArg) {
    for (var i = 0; i < this._p.length; i++) cb.call(thisArg, this._p[i][1], this._p[i][0], this);
};
URLSearchParams.prototype.keys = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: p[i++][0], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.values = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: p[i++][1], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.entries = function() {
    var p = this._p, i = 0;
    return { next: function() { return i < p.length ? { value: [p[i][0], p[i++][1]], done: false } : { value: undefined, done: true }; },
             Symbol_iterator: function() { return this; } };
};
URLSearchParams.prototype.size = undefined; // defined as getter below
Object.defineProperty(URLSearchParams.prototype, 'size', {
    get: function() { return this._p.length; }
});

// ── URL (WHATWG URL §6.1) ─────────────────────────────────────────────────────
// Supports absolute URLs and resolution against a base URL.
// Full IDNA/percent-encoding spec requires platform support; this is a
// high-fidelity subset sufficient for the most common JS URL patterns.
function _url_resolve(href, base) {
    href = String(href || '');
    // Already absolute?
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(href)) return href;
    if (!base) return href;
    // Empty relative reference resolves to the base itself (RFC 3986 §4.2 same-document reference).
    if (href === '') return String(base);
    var bp = _lumen_parse_url(String(base));
    // Protocol-relative
    if (href.slice(0, 2) === '//') return bp.protocol + href;
    // Root-relative
    if (href[0] === '/') return bp.protocol + '//' + bp.host + href;
    // Fragment-only or query-only
    if (href[0] === '#') return bp.protocol + '//' + bp.host + bp.pathname + bp.search + href;
    if (href[0] === '?') return bp.protocol + '//' + bp.host + bp.pathname + href;
    // Relative path
    var dir = bp.pathname.slice(0, bp.pathname.lastIndexOf('/') + 1);
    var raw = dir + href;
    // Normalize dot segments (RFC 3986 §5.2.4)
    var parts = raw.split('/');
    var out = [];
    for (var i = 0; i < parts.length; i++) {
        if (parts[i] === '.') continue;
        if (parts[i] === '..') { if (out.length > 1) out.pop(); }
        else out.push(parts[i]);
    }
    return bp.protocol + '//' + bp.host + out.join('/');
}
// Implementation slots of a URL object. They are defined non-enumerable so a
// page walking the object (`for…in`, `Object.keys`) sees only the WebIDL
// attributes, which live on the prototype (BUG-375 §5).
var _LUMEN_URL_SLOTS = ['_href', '_protocol', '_username', '_password', '_hostname',
                        '_host', '_port', '_pathname', '_search', '_hash', '_origin',
                        '_authority', '_sp'];
function _lumen_url_define_slots(u) {
    for (var i = 0; i < _LUMEN_URL_SLOTS.length; i++) {
        Object.defineProperty(u, _LUMEN_URL_SLOTS[i],
            { value: null, writable: true, enumerable: false, configurable: true });
    }
}
// Copy a `_lumen_parse_url` result into the slots of `u`. Unless `keepSp` is
// set the lazily-created `searchParams` object is refilled from the new query,
// since per spec it is the *same* object for the lifetime of the URL and must
// track every change to `href`/`search`.
function _lumen_url_adopt(u, p, keepSp) {
    u._href      = p.href;
    u._protocol  = p.protocol;
    u._username  = p.username;
    u._password  = p.password;
    u._hostname  = p.hostname;
    u._host      = p.host;
    u._port      = p.port;
    u._pathname  = p.pathname;
    u._search    = p.search;
    u._hash      = p.hash;
    u._origin    = p.origin;
    u._authority = p.hasAuthority;
    if (!keepSp && u._sp) u._sp._p = _usp_parse(u._search);
}
// URL Standard §4.1 `URL serializer` — assemble an href out of the components.
function _lumen_url_serialize(u) {
    var out = u._protocol;
    if (u._authority) {
        out += '//';
        if (u._username || u._password) {
            out += u._username;
            if (u._password) out += ':' + u._password;
            out += '@';
        }
        out += u._host;
    }
    return out + u._pathname + u._search + u._hash;
}
// Rebuild `href` after a component was written, then re-parse it so every other
// component (notably `host`/`origin`) is re-derived from one source of truth
// instead of being patched by hand at each of the nine setters. Re-parsing
// through `_lumen_parse_url` also keeps a single URL parser in the shim.
function _lumen_url_reserialize(u, keepSp) {
    _lumen_url_adopt(u, _lumen_parse_url(_lumen_url_serialize(u)), keepSp);
}
// Characters a username/password may carry literally (URL Standard §1.3 userinfo
// percent-encode set); everything else — ':', '@', '/', '?', '#' included — is
// percent-encoded so it cannot break out of the userinfo when re-serialized.
var _LUMEN_USERINFO_SAFE = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~!$&'()*+,;=%";
function _lumen_url_userinfo_encode(v) {
    var s = String(v), out = '';
    for (var i = 0; i < s.length; i++) {
        var c = s[i];
        if (_LUMEN_USERINFO_SAFE.indexOf(c) >= 0) { out += c; continue; }
        try { out += encodeURIComponent(c); } catch (e) { out += c; }
    }
    return out;
}
// Cut a host-ish setter value at the first character that would end the host in
// a real URL parse, so `u.hostname = 'a/b'` cannot smuggle a path into the host.
function _lumen_url_cut_host(v) {
    var s = String(v);
    for (var i = 0; i < s.length; i++) {
        if (s[i] === '/' || s[i] === '?' || s[i] === '#') return s.slice(0, i);
    }
    return s;
}
function URL(href, base) {
    if (arguments.length === 0) throw new TypeError('URL constructor: at least 1 argument required');
    var resolved = _url_resolve(String(href), base ? String(base) : (typeof location !== 'undefined' ? location.href : ''));
    var p = _lumen_parse_url(resolved);
    if (!p.protocol) throw new TypeError('URL constructor: invalid URL: ' + href);
    _lumen_url_define_slots(this);
    _lumen_url_adopt(this, p);
    this._sp = null; // lazy URLSearchParams
}
(function() {
    // A component without a setter stays getter-only on purpose: an assignment
    // to it must throw a TypeError in strict mode. The former `set: setter ||
    // function() {}` swallowed such writes without a trace (BUG-375 §2).
    function prop(key, getter, setter) {
        var desc = { get: getter, enumerable: true, configurable: true };
        if (setter) desc.set = setter;
        Object.defineProperty(URL.prototype, key, desc);
    }
    prop('href', function() { return this._href; }, function(v) {
        _lumen_url_adopt(this, _lumen_parse_url(String(v)));
    });
    prop('protocol', function() { return this._protocol; }, function(v) {
        var m = /^([a-zA-Z][a-zA-Z0-9+.-]*):?$/.exec(String(v));
        if (!m) return; // not a scheme — URL Standard §4.5 says ignore
        this._protocol = m[1].toLowerCase() + ':';
        _lumen_url_reserialize(this);
    });
    prop('username', function() { return this._username; }, function(v) {
        if (!this._host) return; // a URL with no host cannot have credentials
        this._username = _lumen_url_userinfo_encode(v);
        _lumen_url_reserialize(this);
    });
    prop('password', function() { return this._password; }, function(v) {
        if (!this._host) return;
        this._password = _lumen_url_userinfo_encode(v);
        _lumen_url_reserialize(this);
    });
    prop('hostname', function() { return this._hostname; }, function(v) {
        if (!this._authority) return; // opaque path — no host to replace
        // The hostname setter stops at ':' without touching the port (§4.5).
        var h = _lumen_url_cut_host(v).split(':')[0];
        if (!h) return; // empty host is not a valid replacement
        this._hostname = h;
        this._host = this._port ? h + ':' + this._port : h;
        _lumen_url_reserialize(this);
    });
    prop('host', function() { return this._host; }, function(v) {
        if (!this._authority) return;
        var h = _lumen_url_cut_host(v);
        if (!h) return;
        this._host = h;
        _lumen_url_reserialize(this);
    });
    prop('port', function() { return this._port; }, function(v) {
        if (!this._authority || !this._host) return;
        var s = String(v);
        if (s === '') { this._port = ''; this._host = this._hostname; _lumen_url_reserialize(this); return; }
        var digits = '';
        for (var i = 0; i < s.length && s[i] >= '0' && s[i] <= '9'; i++) digits += s[i];
        if (!digits) return; // non-numeric port — ignore
        this._port = String(parseInt(digits, 10));
        this._host = this._hostname + ':' + this._port;
        _lumen_url_reserialize(this);
    });
    prop('pathname', function() { return this._pathname; }, function(v) {
        if (!this._authority) return; // opaque path is not settable (§4.5)
        var s = String(v);
        // '?' and '#' are percent-encoded rather than allowed to re-split the
        // URL, so writing a path can never silently move data into the query.
        s = s.split('?').join('%3F').split('#').join('%23');
        this._pathname = s.charAt(0) === '/' ? s : '/' + s;
        _lumen_url_reserialize(this);
    });
    prop('search', function() { return this._search; }, function(v) {
        var s = String(v);
        if (s === '') {
            this._search = '';
        } else {
            if (s.charAt(0) === '?') s = s.slice(1);
            this._search = '?' + s.split('#').join('%23');
        }
        _lumen_url_reserialize(this);
    });
    prop('hash', function() { return this._hash; }, function(v) {
        var s = String(v);
        if (s === '') {
            this._hash = '';
        } else {
            if (s.charAt(0) === '#') s = s.slice(1);
            this._hash = '#' + s;
        }
        _lumen_url_reserialize(this);
    });
    prop('origin', function() { return this._origin; }); // readonly per spec
    prop('searchParams', function() {                    // readonly per spec
        if (!this._sp) {
            this._sp = new URLSearchParams(this._search);
            this._sp._url = this; // mutations flow back into href
        }
        return this._sp;
    });
    URL.prototype.toString = function() { return this._href; };
    URL.prototype.toJSON   = function() { return this._href; };
    // URL.canParse(url, base?) — URL Living Standard §6.1 static method (2023)
    URL.canParse = function(url, base) {
        try { new URL(String(url), base !== undefined ? String(base) : undefined); return true; }
        catch (e) { return false; }
    };
    // URL.parse(url, base?) — returns URL or null (URL Living Standard §6.1)
    URL.parse = function(url, base) {
        try { return new URL(String(url), base !== undefined ? String(base) : undefined); }
        catch (e) { return null; }
    };
})();
