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

