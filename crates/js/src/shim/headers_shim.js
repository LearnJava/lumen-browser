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
