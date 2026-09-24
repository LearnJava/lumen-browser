
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
//   _lumen_response_from_fetch_cache(status, statusText, headers, url, redirected)
//        — the network-path factory: the body stays in the Rust FetchCache and
//          is pulled lazily, so large bodies are never copied into JS eagerly.
//          `url` must be the FINAL URL after redirects (BUG-984) — read it
//          from `_lumen_fetch_get_url()`, not the pre-fetch request URL;
//
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
    _lumen_response_from_fetch_cache = function(status, statusText, headers, url, redirected) {
        var st = responseSlots(_lumen_headers_set_guard(new Headers(headers), 'response'));
        st.status = status;
        st.statusText = statusText;
        st.url = url;
        st.redirected = !!redirected;
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

