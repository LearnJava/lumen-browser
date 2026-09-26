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

// PERF-14: in-flight `fetch()` requests. Each entry is the request's `poll`
// closure — it returns `true` once the request has settled (promise resolved
// or rejected) and `false` while the worker thread still has it.
//
// One registry instead of the per-request `setTimeout(poll, 1)` chain the async
// path used before: that chain only advanced when the shell ran timers, and
// the headless one-shot modes run none — so async could not be the default.
// `_lumen_fetch_pump` is pumped by both: by `_lumen_tick_timers` on every live
// tick, and by `V8JsRuntime::settle_pending_fetches` in headless.
var _lumen_fetch_inflight = [];

// Ask the shell to wake up soon while anything is in flight: the parked event
// loop would otherwise only notice a finished request on an unrelated wakeup.
// 2 ms is the cadence the old `setTimeout(poll, 1)` chain settled at.
function _lumen_fetch_arm_wakeup() {
    if (_lumen_fetch_inflight.length > 0) _lumen_request_wakeup(_lumen_now_ms() + 2);
}

function _lumen_fetch_track(poll) {
    _lumen_fetch_inflight.push(poll);
    _lumen_fetch_arm_wakeup();
}

// Settle every request whose response has arrived. Returns how many are still
// in flight — including ones a settled request's callbacks started, which is
// how a `fetch().then(() => fetch())` chain keeps the headless settle loop
// going. The callbacks themselves run as microtasks when the calling `eval`
// returns, not inside this loop.
function _lumen_fetch_pump() {
    if (_lumen_fetch_inflight.length === 0) return 0;
    var list = _lumen_fetch_inflight;
    _lumen_fetch_inflight = [];
    var keep = [];
    for (var i = 0; i < list.length; i++) {
        var done = true;
        try { done = list[i](); } catch (e) { _lumen_report_exception(e); }
        if (!done) keep.push(list[i]);
    }
    _lumen_fetch_inflight = keep.concat(_lumen_fetch_inflight);
    _lumen_fetch_arm_wakeup();
    return _lumen_fetch_inflight.length;
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

        // Fetch §4.2 «scheme fetch», `blob`: the bytes live in the page's blob
        // URL store, not behind a socket — before BUG-1126 the URL went to the
        // network layer and died there as «unsupported scheme». Only GET is
        // allowed; a revoked URL is a network error.
        if (url.slice(0, 5) === 'blob:') {
            var blobEntry = _lumen_blob_url_entry(url);
            if (!blobEntry || method !== 'GET') {
                return Promise.reject(new TypeError('fetch: network error for ' + url));
            }
            return Promise.resolve(_lumen_response_from_blob(blobEntry, url));
        }

        // Fetch §5.4 keepalive flag: request survives page unload (Beacon semantics).
        // Phase 0: accepted syntactically; detachment from page lifecycle is Phase 2.
        // network: keepalive — Phase 2: spawn detached thread, skip response body
        var keepalive = !!(init && init.keepalive);

        // Fetch Priority Hints (WHATWG Fetch §2.2.6): 'high'|'low'|'auto'.
        // network: priority queue — lumen-network Phase 2 (still a single FIFO
        // connection pool); an explicit hint maps to the RFC 9218 `Priority`
        // request header (urgency `u`, 0 most urgent .. 7 least, default u=3)
        // below, same on-the-wire signal srez 4 sends for the HTML
        // `fetchpriority` attribute's preload hints — 'auto' sends nothing and
        // leaves urgency to server/UA heuristics.
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
        // RFC 9218 Priority header for an explicit init.priority — an author
        // header of the same name (set via init.headers/Request.headers)
        // wins, mirroring the Content-Type override rule below.
        if (_fetchPriority !== 'auto') {
            var hasPriorityHeader = false;
            for (var pfi = 0; pfi + 1 < authorHeaders.length; pfi += 2) {
                if (authorHeaders[pfi] === 'priority') { hasPriorityHeader = true; break; }
            }
            if (!hasPriorityHeader) {
                authorHeaders.push('priority', _fetchPriority === 'high' ? 'u=1' : 'u=5');
            }
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

        // Async path — the default (PERF-14). The request runs on a worker thread
        // (the _lumen_fetch_async_* bridges) and the promise settles from
        // `_lumen_fetch_pump`, so the JS thread is free for the whole round trip:
        // whatever the script does after `fetch()` — `new Image().src`, the next
        // `fetch()`, DOM work, the rest of the page's scripts — no longer waits
        // for the response. A live signal additionally gets in-flight
        // cancellation: `abort()` flips the token and the socket is torn down.
        //
        // Who pumps: the live loop through `_lumen_tick_timers` (every tick, for
        // the page and for every frame), the headless one-shot modes through
        // `V8JsRuntime::settle_pending_fetches` right after the page's scripts —
        // before that change of PERF-14 they pumped nothing, which is why the
        // default used to be synchronous (BUG-1013 had to opt `FontFace.load()`
        // out by hand with `_lumenAsync`, now a no-op kept for readability).
        //
        // Still synchronous: an `AbortSignal.timeout(ms)` signal, whose deadline
        // is enforced by a native deadline thread on the cancellable bridge — the
        // headless settle loop does not run timers, so the JS-side timeout could
        // never fire there.
        var useAsync = !(_timeoutMs > 0);
        if (useAsync) {
            return new Promise(function(resolve, reject) {
                // BUG-1116: an element loading itself (`<script src>`, `<link
                // rel=stylesheet>`, `@import` — the `_lumenInitiatorType`
                // callers) may take bytes a `<link rel=preload>` hint has
                // already fetched this navigation instead of a second GET. A
                // plain page `fetch()` never does: its request mode/credentials
                // can differ from the hint's.
                //
                // BUG-1021: the request's Fetch mode and destination ride in
                // one `mode|destination` string (the native side reads the
                // preload decision off it too: a non-empty destination is an
                // element load). An element names its own — `<link
                // rel=stylesheet>`/`@import` are `no-cors`/`style`, a classic
                // `<script src>` `no-cors`/`script`, a module script
                // `cors`/`script` — a page `fetch()` forwards `init.mode`
                // (empty = Fetch's default `cors`) with the empty destination.
                var _fetchLoad;
                if (_rtInitiator === 'css' || _rtInitiator === 'link') {
                    _fetchLoad = 'no-cors|style';
                } else if (_rtInitiator === 'script') {
                    _fetchLoad = (init && init._lumenModule) ? 'cors|script' : 'no-cors|script';
                } else {
                    _fetchLoad = ((init && typeof init.mode === 'string') ? init.mode : '') + '|';
                }
                var handle = _lumen_fetch_async_start(url, method, contentType || '', bodyBytes || [], !!hasBody, authorHeaders, _fetchLoad);
                if (!handle) {
                    reject(new TypeError('fetch: network error for ' + url));
                    return;
                }
                var settled = false;
                function finish(fn) {
                    if (settled) return;
                    settled = true;
                    // Most callers reach this block with no signal at all,
                    // so the listener pair below is conditional rather than relying
                    // on a `catch` to swallow a TypeError on `undefined`.
                    if (fetchSignal) {
                        try { fetchSignal.removeEventListener('abort', onAbort); } catch (e) {}
                    }
                    // Building the Response runs page-observable code (a
                    // `ReadableStream` reads its underlying-source dictionary,
                    // which a page can poison through `Object.prototype`). On
                    // the synchronous path such a throw landed in `_lumen_fetch`'s
                    // own `catch` and rejected the promise; here it would reach
                    // the pump instead and leave the promise pending forever.
                    try { fn(); } catch (e) { reject(e); }
                }
                function onAbort() { _lumen_fetch_async_abort(handle); }
                if (fetchSignal) {
                    try { fetchSignal.addEventListener('abort', onAbort); } catch (e) {}
                }
                // Returns true once the request has settled (the pump then
                // drops it), false while the worker thread is still on it.
                function poll() {
                    if (settled) return true;
                    var st = _lumen_fetch_async_poll(handle);
                    if (st === 0) return false;
                    if (st === 3) {
                        finish(function() {
                            _lumen_fetch_async_free(handle);
                            reject((fetchSignal && fetchSignal.reason !== undefined) ? fetchSignal.reason : new DOMException('The operation was aborted', 'AbortError'));
                        });
                        return true;
                    }
                    if (st === 4) {
                        finish(function() {
                            _lumen_fire_connect_src_violation(_lumen_fetch_async_csp_info(handle));
                            _lumen_fetch_async_free(handle);
                            reject(new TypeError('fetch: network error for ' + url));
                        });
                        return true;
                    }
                    if (st === 2) {
                        finish(function() {
                            _lumen_fetch_async_free(handle);
                            reject(new TypeError('fetch: network error for ' + url));
                        });
                        return true;
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
                    return true;
                }
                _lumen_fetch_track(poll);
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

