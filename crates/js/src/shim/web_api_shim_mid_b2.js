
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
// GAP-CSPENF срез 10: dispatch `securitypolicyviolation` for a `fetch()`/XHR
// request that `HttpClient` refused before any socket work because the
// document's `connect-src` (or `default-src`) does not allow `url`. Reused by
// both the async and synchronous/cancellable fetch paths below — `csp` is the
// `[blockedUri, originalPolicy]` pair the native side hands back (empty when
// the failure was not a CSP block).
function _lumen_fire_connect_src_violation(csp) {
    if (!csp || csp.length !== 2) return;
    if (typeof _lumen_dispatch_csp_violation === 'function') {
        _lumen_dispatch_csp_violation('connect-src', csp[0], csp[1], 'enforce');
    }
}

// GAP-CSPENF срез 13: same shape as `_lumen_fire_connect_src_violation` above,
// for `worker-src` — `new Worker(url)`/`new SharedWorker(url)` read their own
// `_lumen_worker_last_csp_block`/`_lumen_sw_last_csp_block` side channel and
// hand the `[blockedUri, originalPolicy]` pair (or `null`) here.
function _lumen_fire_worker_src_violation(csp) {
    if (!csp || csp.length !== 2) return;
    if (typeof _lumen_dispatch_csp_violation === 'function') {
        _lumen_dispatch_csp_violation('worker-src', csp[0], csp[1], 'enforce');
    }
}

// GAP-CSPENF срез 16: same shape as `_lumen_fire_worker_src_violation` above,
// for `object-src` — `<embed src>`/`<object data>` reads its own
// `_lumen_object_src_last_csp_block` side channel and hands the
// `[blockedUri, originalPolicy]` pair (or `null`) here.
function _lumen_fire_object_src_violation(csp) {
    if (!csp || csp.length !== 2) return;
    if (typeof _lumen_dispatch_csp_violation === 'function') {
        _lumen_dispatch_csp_violation('object-src', csp[0], csp[1], 'enforce');
    }
}

// GAP-CSPENF срез 17: same shape again, for `media-src` — the three media
// shims (`<video>`/`<audio>` in `video_bindings.rs`/`audio_element.rs`, and
// `<track>`'s `readTrackBody`) read the shared
// `_lumen_media_src_last_csp_block` side channel and hand the
// `[blockedUri, originalPolicy]` pair (or `null`) here. Lives in the common
// shim rather than in any one of those three JS strings precisely because all
// three call it.
function _lumen_fire_media_src_violation(csp) {
    if (!csp || csp.length !== 2) return;
    if (typeof _lumen_dispatch_csp_violation === 'function') {
        _lumen_dispatch_csp_violation('media-src', csp[0], csp[1], 'enforce');
    }
}

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
        //
        // `_lumenAsync` is the shim's own opt-in to that same worker path for
        // callers that have no signal to offer but must not park the JS thread
        // (BUG-1013: `FontFace.load()` blocked the whole load pipeline for as long
        // as the font host took to answer). It is deliberately keyed on an explicit
        // init flag rather than flipped on by default: every other `fetch()` caller
        // in the engine still relies on the response being in hand when the promise
        // is created, and the headless one-shot modes never pump timers at all, so
        // an async promise there would simply never settle.
        var useAsync = !(_timeoutMs > 0)
            && (!!(fetchSignal && !fetchSignal.aborted) || !!(init && init._lumenAsync));
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
                    // `_lumenAsync` callers reach this block with no signal at all,
                    // so the listener pair below is conditional rather than relying
                    // on a `catch` to swallow a TypeError on `undefined`.
                    if (fetchSignal) {
                        try { fetchSignal.removeEventListener('abort', onAbort); } catch (e) {}
                    }
                    fn();
                }
                function onAbort() { _lumen_fetch_async_abort(handle); }
                if (fetchSignal) {
                    try { fetchSignal.addEventListener('abort', onAbort); } catch (e) {}
                }
                function poll() {
                    if (settled) return;
                    var st = _lumen_fetch_async_poll(handle);
                    if (st === 0) { setTimeout(poll, 1); return; }
                    if (st === 3) {
                        finish(function() {
                            _lumen_fetch_async_free(handle);
                            reject((fetchSignal && fetchSignal.reason !== undefined) ? fetchSignal.reason : new DOMException('The operation was aborted', 'AbortError'));
                        });
                        return;
                    }
                    if (st === 4) {
                        finish(function() {
                            _lumen_fire_connect_src_violation(_lumen_fetch_async_csp_info(handle));
                            _lumen_fetch_async_free(handle);
                            reject(new TypeError('fetch: network error for ' + url));
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
                        var afinalUrl = _lumen_fetch_get_url() || url;
                        if (integrity && !_lumen_check_sri_integrity(integrity)) {
                            reject(new TypeError('fetch: SRI integrity check failed for ' + url));
                            return;
                        }
                        var ahdrs = [];
                        for (var i = 0; i + 1 < arawHeaders.length; i += 2) { ahdrs.push([arawHeaders[i], arawHeaders[i + 1]]); }
                        _perf_rt_record_fetch(url, _rtInitiator, _rtStart, astatus);
                        resolve(_lumen_response_from_fetch_cache(astatus, astatusText, ahdrs, afinalUrl, afinalUrl !== url));
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
            _lumen_fire_connect_src_violation(_lumen_fetch_last_csp_block());
            return Promise.reject(new TypeError('fetch: network error for ' + url));
        }
        var status = _lumen_fetch_get_status();
        var statusText = _lumen_fetch_get_status_text();
        var rawHeaders = _lumen_fetch_get_headers();
        var finalUrl = _lumen_fetch_get_url() || url;
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
        return Promise.resolve(_lumen_response_from_fetch_cache(status, statusText, hdrs, finalUrl, finalUrl !== url));
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
    // HTML LS §9.3.4 MessageEventInit.userActivation -- spec default is null,
    // not silently dropped (BUG-610).
    this.userActivation = (init && init.userActivation !== undefined) ? init.userActivation : null;
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
            } else if (ev.t === 'flushed') {
                // GAP-WSASYNC срез 2 (BUG-869): a queued send() finished
                // writing to the socket — take its bytes back out of
                // bufferedAmount, mirroring the increment in send() below.
                ws.bufferedAmount -= ev.bytes;
                if (ws.bufferedAmount < 0) { ws.bufferedAmount = 0; }
            } else if (ev.t === 'error') {
                // GAP-WSASYNC срез 1: connect() now resolves off-thread, so a
                // `connect-src` refusal or an ordinary handshake failure both
                // surface here instead of the old synchronous `!h` branch in
                // the constructor below — same violation-report side channel,
                // same synthesized `close(1006, '', wasClean=false)` (no real
                // close handshake ever happened, since no connection opened).
                var wsCspAsync = (typeof _lumen_ws_last_csp_block === 'function') ? _lumen_ws_last_csp_block() : null;
                _lumen_fire_connect_src_violation(wsCspAsync);
                var err = new Event('error', { isTrusted: true }); err.message = ev.msg;
                _lumen_ws_fire(ws, err);
                ws.readyState = 3; ws._handle = 0;
                _lumen_ws_fire(ws, new CloseEvent(1006, '', false, { isTrusted: true }));
                break;
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
        // GAP-WSASYNC срез 1: real connect failures (refused, timed out,
        // `connect-src` blocked, ...) no longer land here — `_lumen_ws_connect`
        // always hands back a handle and resolves the outcome asynchronously
        // through the `error`/`close` poll events above. `!h` now means only
        // "no WebSocket provider installed" (embedder didn't wire one up).
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
        // GAP-WSASYNC срез 2 (BUG-869): the native call only queues the
        // frame now — count it as buffered immediately, the 'flushed' poll
        // event above subtracts it back out once the writer thread actually
        // puts it on the wire.
        this.bufferedAmount += n;
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
    // GAP-SLOT (DOM LS §4.2.2.4): changing `slot` on a light-DOM child moves it
    // between named slots of its host's shadow tree — re-signal the host so
    // `_lumen_fire_slotchange` re-fires for the (possibly two) affected slots.
    if (String(name) === 'slot') {
        var _slot_host = _lumen_u2n(_lumen_get_parent(nid));
        if (_slot_host !== null) { _lumen_fire_slotchange(_slot_host); }
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
    if (_lumen_is_text_node(nid) || _lumen_is_comment_node(nid) || _lumen_is_processing_instruction_node(nid)) {
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

// GAP-CSSANIM срез 6 — `getAnimations()` registration for CSS-triggered
// transitions/animations. Срезы 1/2 below already dispatch real lifecycle
// events; срез 5 found that the Web Animations machinery itself (`Animation`/
// `KeyframeEffect`/`_wa_animations`, all in `web_api_shim_tail_b.js`) was
// already complete — the only gap is that `TransitionScheduler`/
// `AnimationScheduler` never register an entry there, so `getAnimations()`
// on an element with a live CSS transition/animation returns `[]`.
//
// Each registered entry is a real `Animation` wrapping an empty
// `KeyframeEffect(target, [], {})` — just enough for `.effect.target`/
// `.playState`/`.id` to answer without throwing. It is never `play()`ed and
// never ticks its own RAF: the visual value is driven natively by the Rust
// scheduler, and letting this shadow object's `_tick` run would overwrite
// `target.style` with its own (empty) keyframe computation on top of that.
// Keyed by `(kind prefix, node index, property/animation name)` so a second
// property transitioning on the same element gets its own entry, matching
// one `CSSTransition`/`CSSAnimation` per (target, property) per spec.
var _lumen_css_anim_registry = {};

function _lumen_css_anim_key(prefix, nid, name) { return prefix + nid + ':' + name; }

// CSS Transitions L1 §3 "creation" happens at the same time as `transitionrun`;
// CSS Animations L1 has no dedicated creation event, so `animationstart` (the
// earliest event this scheduler emits) is used as the approximation.
function _lumen_css_anim_register(prefix, nid, name) {
    var key = _lumen_css_anim_key(prefix, nid, name);
    var anim = _lumen_css_anim_registry[key];
    if (anim) return anim;
    var eff = new KeyframeEffect(_lumen_make_element(nid), [], {});
    anim = new Animation(eff, _wa_doc_timeline);
    anim.id = name;
    anim._state = 'running';
    _lumen_css_anim_registry[key] = anim;
    _wa_animations.push(anim);
    return anim;
}

// `finalState === 'idle'` drops the entry from `_wa_animations` entirely
// (CSS Transitions L1 §3: a completed/canceled transition is discarded);
// any other value keeps it there with that `playState` (CSS Animations L1
// §4.5.1: a finished CSS animation stays in `getAnimations()` until its
// `animation-name` is removed or it is replaced/canceled) and drops only the
// registry key, so a later restart under the same name creates a fresh entry.
function _lumen_css_anim_unregister(prefix, nid, name, finalState) {
    var key = _lumen_css_anim_key(prefix, nid, name);
    var anim = _lumen_css_anim_registry[key];
    if (!anim) return;
    delete _lumen_css_anim_registry[key];
    if (finalState === 'idle') {
        var idx = _wa_animations.indexOf(anim);
        if (idx >= 0) _wa_animations.splice(idx, 1);
    } else {
        anim._state = finalState;
    }
}

// CSS Transitions L1 §3 (GAP-CSSANIM срез 1) — deliver the shell's batch of
// transition lifecycle events. `events` is an array of `[node_index, kind,
// property_name, elapsed_time]` tuples, `kind` one of "run"/"start"/"end"/
// "cancel", computed by `TransitionScheduler::sync`/`tick` inside the shell's
// «update the rendering» step (Step 2, before rAF per spec §8.1.5.1) — same
// queued-task shape as `_lumen_deliver_cv_state_changes` above.
//
// `transitionrun`/`transitionstart`/`transitioncancel` are not cancelable;
// `transitionend` is (CSS Transitions L1 §3, "Firing Transition Events").
var _LUMEN_TRANSITION_EVENT_TYPES = {
    run: 'transitionrun', start: 'transitionstart',
    end: 'transitionend', cancel: 'transitioncancel'
};
function _lumen_deliver_transition_events(events) {
    if (!events || events.length === 0) return;
    for (var i = 0; i < events.length; i++) {
        var nid = events[i][0];
        var kind = events[i][1];
        var type = _LUMEN_TRANSITION_EVENT_TYPES[kind];
        if (!type) continue;
        var propertyName = events[i][2];
        if (kind === 'run') _lumen_css_anim_register('t:', nid, propertyName);
        var evt = new TransitionEvent(type, {
            bubbles: true, cancelable: type === 'transitionend', isTrusted: true,
            propertyName: propertyName, elapsedTime: events[i][3]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
        if (kind === 'end' || kind === 'cancel') _lumen_css_anim_unregister('t:', nid, propertyName, 'idle');
    }
}

// CSS Animations L1 §4.5.1 (GAP-CSSANIM срез 2) — deliver the shell's batch
// of CSS Animations lifecycle events. `events` is an array of `[node_index,
// kind, animation_name, elapsed_time]` tuples, `kind` one of
// "start"/"iteration"/"end"/"cancel", computed by
// `animation_scheduler::AnimationScheduler::tick` — same queued-task shape
// as `_lumen_deliver_transition_events` above.
//
// None of the four `AnimationEvent`s are cancelable (CSS Animations L1
// §4.5.1, "Event dispatch").
var _LUMEN_ANIMATION_EVENT_TYPES = {
    start: 'animationstart', iteration: 'animationiteration',
    end: 'animationend', cancel: 'animationcancel'
};
function _lumen_deliver_animation_events(events) {
    if (!events || events.length === 0) return;
    for (var i = 0; i < events.length; i++) {
        var nid = events[i][0];
        var kind = events[i][1];
        var type = _LUMEN_ANIMATION_EVENT_TYPES[kind];
        if (!type) continue;
        var animationName = events[i][2];
        if (kind === 'start') _lumen_css_anim_register('a:', nid, animationName);
        var evt = new AnimationEvent(type, {
            bubbles: true, cancelable: false, isTrusted: true,
            animationName: animationName, elapsedTime: events[i][3]
        });
        evt.target = _lumen_make_element(nid);
        _lumen_dispatch(nid, evt);
        if (kind === 'end') _lumen_css_anim_unregister('a:', nid, animationName, 'finished');
        else if (kind === 'cancel') _lumen_css_anim_unregister('a:', nid, animationName, 'idle');
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
    SHOW_PROCESSING_INSTRUCTION: 0x40,
    SHOW_COMMENT:        0x80,
    SHOW_DOCUMENT:       0x100,
    SHOW_DOCUMENT_TYPE:  0x200,
    SHOW_DOCUMENT_FRAGMENT: 0x400,
};

// Returns NodeFilter.FILTER_ACCEPT / SKIP / REJECT for a node nid given
// whatToShow bitmask and an optional filter callback or NodeFilter object.
function _nf_accepts(nid, whatToShow, filter) {
    // whatToShow bitmask check
    var nt = _lumen_is_text_node(nid) ? 3 : (_lumen_is_comment_node(nid) ? 8 : (_lumen_is_processing_instruction_node(nid) ? 7 : 1)); // 1=element, 3=text, 7=PI, 8=comment
    var bit = (nt === 3) ? NodeFilter.SHOW_TEXT : (nt === 8 ? NodeFilter.SHOW_COMMENT : (nt === 7 ? NodeFilter.SHOW_PROCESSING_INSTRUCTION : NodeFilter.SHOW_ELEMENT));
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
_CaretPosition.prototype.getClientRects = function() { return new DOMRectList([]); };

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
    var raw = String(media == null ? '' : media);
    // Media Queries L4 §Serializing a media query list — `.media` reports the
    // canonical serialization (whitespace collapsed, invalid clauses folded
    // into `not all`), not an echo of the constructor argument.
    this.media       = _lumen_serialize_media_query(raw);
    this.matches     = !!_lumen_match_media(raw, vp[0], vp[1], false, false);
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
// Window listeners registered with the capture flag. Kept apart from the
// buckets above because those are all bubble/at-target buckets and are read by
// `window.dispatchEvent`, whereas these run in the capture phase of a dispatch
// aimed at a node *below* the window — the first hop of `_lumen_event_path`
// walked backwards (BUG-873). Read from `_lumen_invoke_at_window`.
// Prototype-less: unlike `_lumen_listeners` this one is keyed by a bare event
// type, so a page listening for `constructor`/`toString` would otherwise read a
// `Object.prototype` member back as if it were a listener array.
var _win_capture_listeners = Object.create(null);
// The types `window.addEventListener` files in a dedicated bucket above rather
// than in `_other_win_listeners`. All of them are dispatched at the window
// itself, so a capture registration for one must stay in its own bucket.
var _LUMEN_WIN_TARGETED_EVENTS = {
    popstate: 1, pageshow: 1, pagehide: 1, message: 1,
    load: 1, DOMContentLoaded: 1, visibilitychange: 1, error: 1,
};

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
    addEventListener: function(type, fn, options) {
        if (typeof fn !== 'function') return;
        // A capture listener on the window sees an event on its way DOWN to a
        // node, which is a different bucket from everything below (BUG-873).
        // Only for the generic types: the specially-bucketed ones below are all
        // dispatched AT the window (`load`, `popstate`, …), and DOM §2.9 ignores
        // the capture flag at the target — so routing those away from their
        // bucket would silence them instead of re-ordering them.
        if (_lumen_capture_flag(options) && _LUMEN_WIN_TARGETED_EVENTS[type] !== 1) {
            if (!_win_capture_listeners[type]) _win_capture_listeners[type] = [];
            _win_capture_listeners[type].push(fn);
            return;
        }
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
    removeEventListener: function(type, fn, options) {
        var arr;
        if (_lumen_capture_flag(options) && _LUMEN_WIN_TARGETED_EVENTS[type] !== 1) arr = _win_capture_listeners[type];
        else if (type === 'popstate') arr = _popstate_listeners;
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
    /// Two call shapes: legacy `(message, targetOrigin, transfer)` and the
    /// current `(message, options)` where `options.targetOrigin` defaults to
    /// '/'. `targetOrigin` '*' → always deliver; '/' → same-origin only;
    /// any other string is parsed as an absolute URL and compared by origin
    /// (a parse failure throws `SyntaxError`, per spec — a mismatch just
    /// silently drops the message, it is not an error). `message` is
    /// structured-cloned, not passed by reference (BUG-717).
    postMessage: function(message, targetOrigin) {
        if (targetOrigin !== null && typeof targetOrigin === 'object') {
            targetOrigin = ('targetOrigin' in targetOrigin) ? targetOrigin.targetOrigin : '/';
        } else if (targetOrigin === undefined) {
            targetOrigin = '/';
        }
        var origin = location.origin;
        if (targetOrigin !== '*') {
            var target;
            if (targetOrigin === '/') {
                target = origin;
            } else {
                // `_lumen_parse_url` (url_parse_shim.js) splits an authority
                // on its first '/'/'?'/'#' without validating what is inside
                // it — no forbidden-host-code-point check like the URL
                // Standard's host parser has, so `new URL('http://foo bar')`
                // would build an origin instead of throwing. Reject those
                // code points here so the SyntaxError WPT expects still
                // fires; a real domain never contains them.
                var parsedTarget;
                try { parsedTarget = new URL(String(targetOrigin)); }
                catch (e) { parsedTarget = null; }
                if (parsedTarget === null || /[\x00-\x20#\/:<>?@\[\\\]^|]/.test(parsedTarget.hostname)) {
                    throw new DOMException(
                        "Failed to execute 'postMessage' on 'Window': Invalid target origin '" +
                        targetOrigin + "' in a call to 'postMessage'.", 'SyntaxError');
                }
                target = parsedTarget.origin;
            }
            if (target !== origin) return;
        }
        var ev = new MessageEvent(structuredClone(message));
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

// BUG-874 (second half): `Window includes GlobalEventHandlers` (HTML LS
// §8.1.7.1) — same gap as `document` above, on the same curated list. The
// generic branch of `window.dispatchEvent` (see the `onFn = window['on' +
// evt.type]` read a little above) already needs no dispatch-side change for a
// new entry here, per its own comment; only the bare `'onX' in window` idiom
// was missing a declared property to answer `true`. `hasOwnProperty` skips
// the handlers already declared on the literal above with bespoke dispatch
// (`onload`, `onscroll`, …) — same `null` value, so nothing behavioural
// changes for them.
for (var _wohi = 0; _wohi < _LUMEN_EVENT_HANDLER_ATTRS.length; _wohi++) {
    var _wohAttr = _LUMEN_EVENT_HANDLER_ATTRS[_wohi];
    if (!Object.prototype.hasOwnProperty.call(window, _wohAttr)) window[_wohAttr] = null;
}

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

