// fetch() + XMLHttpRequest for a dedicated/shared WorkerGlobalScope
// (BUG-778; WORKER-1 срез 5). Runs after `worker_exposed_shim`, so it builds on
// the page's own Headers/Response/Request (HEADERS_SHIM + FETCH_BODY_SHIM), not
// on a second copy of them. Natives, all from `crate::worker_net`:
//   _lumen_worker_net_fetch(url, method, headersFlat, bytes|null, contentType)
//       → JSON {status, statusText, headers:[[n,v]…], url, redirected} | undefined;
//         the body waits in the scope's "last body" slot, read back through
//         _lumen_stream_alloc (fetch) or _lumen_fetch_body_* (XHR).
(function() {
  function _base() {
    if (typeof _lumen_worker_base_url === 'string' && _lumen_worker_base_url) return _lumen_worker_base_url;
    return (typeof location !== 'undefined' && location) ? String(location.href) : '';
  }
  // The API base URL of a worker is its script URL (HTML LS §10.2.2); the
  // FETCH_BODY_SHIM slice asks for it by the page's name (`new Request(url)`,
  // `Response.redirect`).
  if (typeof globalThis._lumen_document_base_url !== 'function') {
    globalThis._lumen_document_base_url = _base;
  }

  function _report(e) {
    var r = globalThis._lumen_worker_exception_reporter;
    if (typeof r === 'function') { try { r(e); } catch (_e) {} }
  }

  // fetch(input, init) — Fetch §5.1: the arguments go through the Request
  // constructor (method/header/body validation, URL resolution), the network
  // step is the synchronous bridge, and the result is the page's network
  // Response (`_lumen_response_from_fetch_cache`) with the final URL.
  function fetch(input) {
    var init = arguments[1];
    var req;
    try { req = new Request(input, init); } catch (e) { return Promise.reject(e); }
    var signal = req.signal;
    if (signal && signal.aborted) {
      return Promise.reject(signal.reason !== undefined ? signal.reason
        : new DOMException('signal is aborted without reason', 'AbortError'));
    }
    var flat = [];
    req.headers.forEach(function(v, k) { flat.push(k); flat.push(v); });
    var ctype = req.headers.get('content-type') || '';
    var bodyP = req.body === null ? Promise.resolve(null) : req.arrayBuffer();
    return bodyP.then(function(buf) {
      var raw = _lumen_worker_net_fetch(req.url, req.method, flat,
        buf === null ? null : new Uint8Array(buf), ctype);
      if (!raw) throw new TypeError('Failed to fetch');
      var res = JSON.parse(raw);
      return _lumen_response_from_fetch_cache(res.status, res.statusText, res.headers, res.url, res.redirected);
    });
  }
  globalThis.fetch = fetch;

  // ── XMLHttpRequest (XHR Standard §4) ──────────────────────────────────────
  // The request itself is one synchronous bridge call; an async request makes
  // it in a task (setTimeout 0), so handlers assigned after send() — the usual
  // `send(); req.onload = …` order — still see its events.
  function ProgressEvent(type, init) {
    Event.call(this, type, init);
    this.lengthComputable = !!(init && init.lengthComputable);
    this.loaded = (init && typeof init.loaded === 'number') ? init.loaded : 0;
    this.total = (init && typeof init.total === 'number') ? init.total : 0;
  }
  ProgressEvent.prototype = Object.create(Event.prototype);
  ProgressEvent.prototype.constructor = ProgressEvent;
  if (typeof globalThis.ProgressEvent !== 'function') globalThis.ProgressEvent = ProgressEvent;

  function XMLHttpRequestEventTarget() { EventTarget.call(this); }
  XMLHttpRequestEventTarget.prototype = Object.create(EventTarget.prototype);
  XMLHttpRequestEventTarget.prototype.constructor = XMLHttpRequestEventTarget;
  function XMLHttpRequestUpload() { XMLHttpRequestEventTarget.call(this); }
  XMLHttpRequestUpload.prototype = Object.create(XMLHttpRequestEventTarget.prototype);
  XMLHttpRequestUpload.prototype.constructor = XMLHttpRequestUpload;

  var UNSENT = 0, OPENED = 1, HEADERS_RECEIVED = 2, LOADING = 3, DONE = 4;

  function invalidState(what) { return new DOMException(what, 'InvalidStateError'); }

  // A listener that throws must not stop the others (EventTarget already
  // guards each call); `dispatchEvent` itself is not expected to throw.
  function fire(xhr, ev) { try { xhr.dispatchEvent(ev); } catch (e) { _report(e); } }
  function progress(xhr, type, loaded, total) {
    fire(xhr, new ProgressEvent(type, { lengthComputable: total > 0, loaded: loaded, total: total }));
  }

  // Fetch §7.1 «extract a body», synchronously — a sync XHR has no turn to
  // wait on a promise.
  function extract(body) {
    if (typeof body === 'string') return { bytes: new TextEncoder().encode(body), type: 'text/plain;charset=UTF-8' };
    if (body instanceof URLSearchParams) {
      return { bytes: new TextEncoder().encode(body.toString()), type: 'application/x-www-form-urlencoded;charset=UTF-8' };
    }
    if (body instanceof FormData) {
      var boundary = '----LumenFormBoundary' + Math.random().toString(36).slice(2, 10).toUpperCase();
      return { bytes: body._toMultipart(boundary), type: 'multipart/form-data; boundary=' + boundary };
    }
    if (body instanceof Blob) return { bytes: new Uint8Array(body._bytes), type: body.type || null };
    if (body instanceof ArrayBuffer) return { bytes: new Uint8Array(body.slice(0)), type: null };
    if (ArrayBuffer.isView(body)) {
      return { bytes: new Uint8Array(body.buffer.slice(body.byteOffset, body.byteOffset + body.byteLength)), type: null };
    }
    return { bytes: new TextEncoder().encode(String(body)), type: 'text/plain;charset=UTF-8' };
  }

  // `charset` parameter of a MIME type string, quotes stripped; null if none.
  function charsetOf(mime) {
    if (!mime) return null;
    var params = String(mime).split(';');
    for (var i = 1; i < params.length; i++) {
      var p = params[i].trim();
      if (p.slice(0, 8).toLowerCase() !== 'charset=') continue;
      var v = p.slice(8).trim();
      if (v.length >= 2 && v[0] === '"' && v[v.length - 1] === '"') v = v.slice(1, -1);
      return v;
    }
    return null;
  }

  function XMLHttpRequest() {
    XMLHttpRequestEventTarget.call(this);
    Object.defineProperty(this, '_x', { value: {
      state: UNSENT, method: 'GET', url: '', async: true, headers: [], sent: false,
      status: 0, statusText: '', responseURL: '', respHeaders: [], bytes: null,
      override: null, responseType: '', aborted: false, error: false, uploadComplete: true,
    }, writable: true });
    this.timeout = 0;
    this.withCredentials = false;
    this.upload = new XMLHttpRequestUpload();
  }
  XMLHttpRequest.prototype = Object.create(XMLHttpRequestEventTarget.prototype);
  XMLHttpRequest.prototype.constructor = XMLHttpRequest;
  [['UNSENT', UNSENT], ['OPENED', OPENED], ['HEADERS_RECEIVED', HEADERS_RECEIVED],
   ['LOADING', LOADING], ['DONE', DONE]].forEach(function(c) {
    Object.defineProperty(XMLHttpRequest, c[0], { value: c[1], enumerable: true });
    Object.defineProperty(XMLHttpRequest.prototype, c[0], { value: c[1], enumerable: true });
  });

  function getter(name, fn) {
    Object.defineProperty(XMLHttpRequest.prototype, name, { get: fn, enumerable: true, configurable: true });
  }
  getter('readyState', function() { return this._x.state; });
  getter('status', function() { return this._x.status; });
  getter('statusText', function() { return this._x.statusText; });
  getter('responseURL', function() { return this._x.responseURL; });
  getter('responseXML', function() { return null; });
  Object.defineProperty(XMLHttpRequest.prototype, 'responseType', {
    get: function() { return this._x.responseType; },
    set: function(v) {
      var x = this._x;
      if (x.state === LOADING || x.state === DONE) throw invalidState('responseType cannot be set now');
      // XHR §4.6.2: a worker has no documents — 'document' is silently ignored.
      v = String(v);
      if (v === 'document') return;
      if (['', 'arraybuffer', 'blob', 'json', 'text'].indexOf(v) < 0) return;
      x.responseType = v;
    },
    enumerable: true, configurable: true,
  });

  // XHR §4.6.5: the override MIME type wins over the response's Content-Type.
  function finalMime(x) {
    if (x.override !== null) return x.override;
    for (var i = 0; i < x.respHeaders.length; i++) {
      if (String(x.respHeaders[i][0]).toLowerCase() === 'content-type') return String(x.respHeaders[i][1]);
    }
    return 'text/xml';
  }
  // Encoding Standard §4.2: labels of the "replacement" encoding. The shared
  // label table (`_lumen_text_encoding_for_label`) serves TextDecoder, which
  // must reject them, so it does not carry them.
  var REPLACEMENT_LABELS = ['csiso2022kr', 'hz-gb-2312', 'iso-2022-cn', 'iso-2022-cn-ext', 'iso-2022-kr', 'replacement'];
  // XHR §4.6.6 «text response»: the final charset, else UTF-8, then Encoding
  // §6 «decode», where a BOM overrides both.
  function textOf(x) {
    var bytes = x.bytes || new Uint8Array(0);
    var label = charsetOf(finalMime(x));
    var enc = 'utf-8';
    if (label !== null) {
      var l = label.trim().toLowerCase();
      if (REPLACEMENT_LABELS.indexOf(l) >= 0) enc = 'replacement';
      else {
        var c = _lumen_text_encoding_for_label(l);
        // UTF-32 is a Lumen extension of the TextDecoder table, not a WHATWG
        // encoding — for XHR its labels are unknown, i.e. UTF-8.
        if (c !== undefined && c.slice(0, 6) !== 'utf-32') enc = c;
      }
    }
    if (bytes.length >= 3 && bytes[0] === 0xEF && bytes[1] === 0xBB && bytes[2] === 0xBF) enc = 'utf-8';
    else if (bytes.length >= 2 && bytes[0] === 0xFE && bytes[1] === 0xFF) enc = 'utf-16be';
    else if (bytes.length >= 2 && bytes[0] === 0xFF && bytes[1] === 0xFE) enc = 'utf-16le';
    if (enc === 'replacement') return bytes.length ? String.fromCharCode(0xFFFD) : '';
    return _lumen_text_decode(enc, bytes, false, false);
  }
  getter('responseText', function() {
    var x = this._x;
    if (x.responseType !== '' && x.responseType !== 'text') throw invalidState('responseText needs responseType "" or "text"');
    if (x.state !== LOADING && x.state !== DONE) return '';
    return textOf(x);
  });
  getter('response', function() {
    var x = this._x;
    if (x.responseType === '' || x.responseType === 'text') {
      return (x.state === LOADING || x.state === DONE) ? textOf(x) : '';
    }
    if (x.state !== DONE || x.error) return null;
    var b = x.bytes || new Uint8Array(0);
    if (x.responseType === 'arraybuffer') return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
    if (x.responseType === 'blob') return new Blob([b], { type: finalMime(x) });
    try { return JSON.parse(new TextDecoder().decode(b)); } catch (e) { return null; }
  });

  XMLHttpRequest.prototype.open = function(method, url) {
    var x = this._x;
    var m = String(method);
    if (!/^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/.test(m)) throw new DOMException('Invalid method ' + m, 'SyntaxError');
    var up = m.toUpperCase();
    if (up === 'CONNECT' || up === 'TRACE' || up === 'TRACK') throw new DOMException('Forbidden method ' + m, 'SecurityError');
    if (['DELETE', 'GET', 'HEAD', 'OPTIONS', 'POST', 'PUT'].indexOf(up) >= 0) m = up;
    x.method = m;
    x.url = _url_resolve(String(url), _base()) || String(url);
    x.async = arguments.length < 3 ? true : !!arguments[2];
    x.headers = []; x.sent = false; x.aborted = false; x.error = false;
    x.status = 0; x.statusText = ''; x.responseURL = ''; x.respHeaders = []; x.bytes = null;
    if (x.state !== OPENED) { x.state = OPENED; fire(this, new Event('readystatechange')); }
  };

  XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
    var x = this._x;
    if (x.state !== OPENED || x.sent) throw invalidState('setRequestHeader before open() or after send()');
    var n = String(name).toLowerCase(), v = String(value);
    for (var i = 0; i < x.headers.length; i++) {
      if (x.headers[i][0] === n) { x.headers[i][1] += ', ' + v; return; }
    }
    x.headers.push([n, v]);
  };

  XMLHttpRequest.prototype.overrideMimeType = function(mime) {
    var x = this._x;
    if (x.state === LOADING || x.state === DONE) throw invalidState('overrideMimeType after the response arrived');
    x.override = String(mime);
  };

  XMLHttpRequest.prototype.getResponseHeader = function(name) {
    var x = this._x;
    if (x.state < HEADERS_RECEIVED || x.error) return null;
    var n = String(name).toLowerCase(), out = null;
    if (n === 'set-cookie' || n === 'set-cookie2') return null;
    for (var i = 0; i < x.respHeaders.length; i++) {
      if (String(x.respHeaders[i][0]).toLowerCase() !== n) continue;
      out = out === null ? String(x.respHeaders[i][1]) : out + ', ' + x.respHeaders[i][1];
    }
    return out;
  };

  XMLHttpRequest.prototype.getAllResponseHeaders = function() {
    var x = this._x;
    if (x.state < HEADERS_RECEIVED || x.error) return '';
    var map = {}, names = [];
    for (var i = 0; i < x.respHeaders.length; i++) {
      var n = String(x.respHeaders[i][0]).toLowerCase();
      if (n === 'set-cookie' || n === 'set-cookie2') continue;
      if (map[n] === undefined) { map[n] = String(x.respHeaders[i][1]); names.push(n); }
      else map[n] += ', ' + x.respHeaders[i][1];
    }
    names.sort();
    return names.map(function(k) { return k + ': ' + map[k] + '\r\n'; }).join('');
  };

  // Performs the request; true on a response, false on a network error.
  function perform(xhr, bodyInfo) {
    var x = xhr._x;
    var flat = [], ctype = '';
    for (var i = 0; i < x.headers.length; i++) {
      flat.push(x.headers[i][0]); flat.push(x.headers[i][1]);
      if (x.headers[i][0] === 'content-type') ctype = x.headers[i][1];
    }
    if (bodyInfo !== null && !ctype && bodyInfo.type) {
      ctype = bodyInfo.type; flat.push('content-type'); flat.push(ctype);
    }
    var raw;
    try {
      raw = _lumen_worker_net_fetch(x.url, x.method, flat, bodyInfo === null ? null : bodyInfo.bytes, ctype);
    } catch (e) { raw = undefined; }
    if (!raw) return false;
    var res = JSON.parse(raw);
    var len = _lumen_fetch_body_length();
    x.bytes = len > 0 ? new Uint8Array(_lumen_fetch_body_chunk(0, len)) : new Uint8Array(0);
    x.status = res.status; x.statusText = res.statusText;
    x.responseURL = res.url; x.respHeaders = res.headers || [];
    return true;
  }

  // XHR §4.5.7 «request error steps» for `type` ('error' or 'abort'): DONE,
  // then the upload's events while its body is still in flight, then the
  // request's own.
  function requestError(xhr, type) {
    var x = xhr._x;
    x.error = true; x.state = DONE; x.sent = false; x.bytes = null;
    x.status = 0; x.statusText = ''; x.respHeaders = [];
    fire(xhr, new Event('readystatechange'));
    if (!x.uploadComplete) {
      x.uploadComplete = true;
      progress(xhr.upload, type, 0, 0);
      progress(xhr.upload, 'loadend', 0, 0);
    }
    progress(xhr, type, 0, 0);
    progress(xhr, 'loadend', 0, 0);
  }

  XMLHttpRequest.prototype.send = function(body) {
    var x = this._x, self = this;
    if (x.state !== OPENED || x.sent) throw invalidState('send() needs an open(), unsent request');
    var bodyInfo = (body === undefined || body === null || x.method === 'GET' || x.method === 'HEAD')
      ? null : extract(body);
    x.sent = true; x.aborted = false; x.uploadComplete = bodyInfo === null;
    if (!x.async) {
      if (!perform(this, bodyInfo)) {
        x.error = true; x.state = DONE;
        throw new DOMException('Failed to load ' + x.url, 'NetworkError');
      }
      x.state = DONE;
      fire(this, new Event('readystatechange'));
      var n = x.bytes.length;
      progress(this, 'load', n, n);
      progress(this, 'loadend', n, n);
      return;
    }
    progress(this, 'loadstart', 0, 0);
    if (!x.uploadComplete && !x.aborted) progress(this.upload, 'loadstart', 0, bodyInfo.bytes.length);
    if (x.aborted) return;
    setTimeout(function() {
      if (x.aborted || !x.sent) return;
      if (!perform(self, bodyInfo)) { requestError(self, 'error'); return; }
      // The whole body went out with the request: XHR §4.5.6 step 11's
      // «process request end-of-body» fires the upload's final events here.
      if (!x.uploadComplete) {
        x.uploadComplete = true;
        var sent = bodyInfo.bytes.length;
        progress(self.upload, 'progress', sent, sent);
        progress(self.upload, 'load', sent, sent);
        progress(self.upload, 'loadend', sent, sent);
        if (x.aborted) return;
      }
      var total = x.bytes.length;
      x.state = HEADERS_RECEIVED; fire(self, new Event('readystatechange'));
      if (x.aborted) return;
      x.state = LOADING; fire(self, new Event('readystatechange'));
      if (x.aborted) return;
      if (total > 0) progress(self, 'progress', total, total);
      x.state = DONE; x.sent = false; fire(self, new Event('readystatechange'));
      progress(self, 'load', total, total);
      progress(self, 'loadend', total, total);
    }, 0);
  };

  XMLHttpRequest.prototype.abort = function() {
    var x = this._x;
    x.aborted = true;
    if ((x.state === OPENED && x.sent) || x.state === HEADERS_RECEIVED || x.state === LOADING) {
      requestError(this, 'abort');
    }
    if (x.state === DONE) { x.state = UNSENT; x.bytes = null; x.status = 0; x.statusText = ''; }
  };

  Object.defineProperty(XMLHttpRequest.prototype, Symbol.toStringTag, { value: 'XMLHttpRequest', configurable: true });
  globalThis.XMLHttpRequestEventTarget = XMLHttpRequestEventTarget;
  globalThis.XMLHttpRequestUpload = XMLHttpRequestUpload;
  globalThis.XMLHttpRequest = XMLHttpRequest;
})();
undefined;
