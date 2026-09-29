//! WebCodecs API Phase 0
//!
//! W3C Web Codecs (https://www.w3.org/TR/webcodecs/)
//!
//! Phase 0 — API stubs without real encoding/decoding:
//! - VideoEncoder / VideoDecoder classes
//! - AudioEncoder / AudioDecoder classes
//! - EncodedVideoChunk / EncodedAudioChunk buffer types
//! - VideoFrame / AudioData types
//! - Error handling: NotSupportedError, OperationError
//! - Full DOM structure; Phase 1 (future): actual codec bindings via FFmpeg or libav1

/// V8 port of the former rquickjs `install_webcodecs_bindings` (Ph3 V8
/// migration S5-S7, rquickjs side removed in S12b-B15): identical JS shims
/// (error constructors + WebCodecs class stubs), evaluated via
/// [`lumen_core::ext::JsRuntime::eval`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_webcodecs_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;

    // Install error constructors
    let error_shim = r#"
        class NotSupportedError extends DOMException {
            constructor(message = '') {
                super(message, 'NotSupportedError');
                this.name = 'NotSupportedError';
            }
        }
        class OperationError extends DOMException {
            constructor(message = '') {
                super(message, 'OperationError');
                this.name = 'OperationError';
            }
        }
        // Referenced by encode()/decode() when the codec is not configured.
        // Defined here so a not-configured call throws a real InvalidStateError
        // (per spec) rather than a ReferenceError.
        class InvalidStateError extends DOMException {
            constructor(message = '') {
                super(message, 'InvalidStateError');
                this.name = 'InvalidStateError';
            }
        }
        globalThis.NotSupportedError = NotSupportedError;
        globalThis.OperationError = OperationError;
        if (typeof globalThis.InvalidStateError === 'undefined') {
            globalThis.InvalidStateError = InvalidStateError;
        }
    "#;
    rt.eval(error_shim)?;

    // Install WebCodecs classes
    let webcodecs_shim = r#"
        class VideoEncoder {
            constructor(output, error) {
                this._output = output;
                this._error = error;
                this._state = 'unconfigured';
            }
            configure(config) {
                // Phase 0 has no codec backend. Per the WebCodecs spec, an
                // unsupported configuration is reported asynchronously through
                // the error callback — NOT a synchronous throw (which crashes
                // SPAs that don't wrap configure() in try/catch).
                this._state = 'configured';
                var err = this._error;
                if (typeof err === 'function') {
                    Promise.resolve().then(function() {
                        err(new NotSupportedError('VideoEncoder: codec not supported'));
                    });
                }
            }
            encode(frame, options) {
                if (this._state === 'unconfigured') {
                    throw new InvalidStateError('VideoEncoder not configured');
                }
            }
            async flush() {
                // Phase 0: no-op
            }
            reset() {
                this._state = 'unconfigured';
            }
            close() {
                this._state = 'closed';
            }
            static isConfigSupported(config) {
                return Promise.resolve(false);
            }
        }

        class VideoDecoder {
            constructor(output, error) {
                this._output = output;
                this._error = error;
                this._state = 'unconfigured';
            }
            configure(config) {
                // See VideoEncoder.configure — report unsupported async, no throw.
                this._state = 'configured';
                var err = this._error;
                if (typeof err === 'function') {
                    Promise.resolve().then(function() {
                        err(new NotSupportedError('VideoDecoder: codec not supported'));
                    });
                }
            }
            decode(chunk) {
                if (this._state === 'unconfigured') {
                    throw new InvalidStateError('VideoDecoder not configured');
                }
            }
            async flush() {
                // Phase 0: no-op
            }
            reset() {
                this._state = 'unconfigured';
            }
            close() {
                this._state = 'closed';
            }
            static isConfigSupported(config) {
                return Promise.resolve(false);
            }
        }

        class AudioEncoder {
            constructor(output, error) {
                this._output = output;
                this._error = error;
                this._state = 'unconfigured';
            }
            configure(config) {
                // See VideoEncoder.configure — report unsupported async, no throw.
                this._state = 'configured';
                var err = this._error;
                if (typeof err === 'function') {
                    Promise.resolve().then(function() {
                        err(new NotSupportedError('AudioEncoder: codec not supported'));
                    });
                }
            }
            encode(data) {
                if (this._state === 'unconfigured') {
                    throw new InvalidStateError('AudioEncoder not configured');
                }
            }
            async flush() {
                // Phase 0: no-op
            }
            reset() {
                this._state = 'unconfigured';
            }
            close() {
                this._state = 'closed';
            }
            static isConfigSupported(config) {
                return Promise.resolve(false);
            }
        }

        class AudioDecoder {
            constructor(output, error) {
                this._output = output;
                this._error = error;
                this._state = 'unconfigured';
            }
            configure(config) {
                // See VideoEncoder.configure — report unsupported async, no throw.
                this._state = 'configured';
                var err = this._error;
                if (typeof err === 'function') {
                    Promise.resolve().then(function() {
                        err(new NotSupportedError('AudioDecoder: codec not supported'));
                    });
                }
            }
            decode(chunk) {
                if (this._state === 'unconfigured') {
                    throw new InvalidStateError('AudioDecoder not configured');
                }
            }
            async flush() {
                // Phase 0: no-op
            }
            reset() {
                this._state = 'unconfigured';
            }
            close() {
                this._state = 'closed';
            }
            static isConfigSupported(config) {
                return Promise.resolve(false);
            }
        }

        // Copies a BufferSource (ArrayBuffer / view) into a fresh Uint8Array.
        function _wcBytes(src) {
            if (src instanceof ArrayBuffer) {
                return new Uint8Array(src.slice(0));
            }
            if (ArrayBuffer.isView(src)) {
                return new Uint8Array(src.buffer.slice(src.byteOffset, src.byteOffset + src.byteLength));
            }
            throw new TypeError('data must be a BufferSource');
        }
        function _wcDest(destination, needed) {
            if (!(destination instanceof ArrayBuffer) && !ArrayBuffer.isView(destination)) {
                throw new TypeError('destination must be a BufferSource');
            }
            if (destination.byteLength < needed) {
                throw new TypeError('destination is not large enough');
            }
            return destination instanceof ArrayBuffer
                ? new Uint8Array(destination)
                : new Uint8Array(destination.buffer, destination.byteOffset, destination.byteLength);
        }

        class EncodedVideoChunk {
            constructor(init) {
                if (init === null || typeof init !== 'object') {
                    throw new TypeError('EncodedVideoChunk: init dictionary required');
                }
                if (init.type !== 'key' && init.type !== 'delta') {
                    throw new TypeError('EncodedVideoChunk: invalid type');
                }
                this.type = init.type;
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration === undefined ? null : init.duration;
                this._data = _wcBytes(init.data === undefined ? new Uint8Array(0) : init.data);
            }
            get byteLength() {
                return this._data.byteLength;
            }
            copyTo(destination) {
                _wcDest(destination, this._data.byteLength).set(this._data);
            }
        }

        class EncodedAudioChunk {
            constructor(init) {
                if (init === null || typeof init !== 'object') {
                    throw new TypeError('EncodedAudioChunk: init dictionary required');
                }
                if (init.type !== 'key' && init.type !== 'delta') {
                    throw new TypeError('EncodedAudioChunk: invalid type');
                }
                this.type = init.type;
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration === undefined ? null : init.duration;
                this._data = _wcBytes(init.data === undefined ? new Uint8Array(0) : init.data);
            }
            get byteLength() {
                return this._data.byteLength;
            }
            copyTo(destination) {
                _wcDest(destination, this._data.byteLength).set(this._data);
            }
        }

        class VideoColorSpace {
            constructor(init) {
                init = init || {};
                this.primaries = init.primaries === undefined ? null : init.primaries;
                this.transfer = init.transfer === undefined ? null : init.transfer;
                this.matrix = init.matrix === undefined ? null : init.matrix;
                this.fullRange = init.fullRange === undefined ? null : init.fullRange;
            }
            toJSON() {
                return {
                    primaries: this.primaries,
                    transfer: this.transfer,
                    matrix: this.matrix,
                    fullRange: this.fullRange
                };
            }
        }

        // Plane layouts: [[subsampleX, subsampleY, bytesPerPixel], ...]
        var _wcVideoFormats = {
            I420: [[1, 1, 1], [2, 2, 1], [2, 2, 1]],
            I420A: [[1, 1, 1], [2, 2, 1], [2, 2, 1], [1, 1, 1]],
            I422: [[1, 1, 1], [2, 1, 1], [2, 1, 1]],
            I444: [[1, 1, 1], [1, 1, 1], [1, 1, 1]],
            NV12: [[1, 1, 1], [2, 2, 2]],
            RGBA: [[1, 1, 4]], RGBX: [[1, 1, 4]], BGRA: [[1, 1, 4]], BGRX: [[1, 1, 4]]
        };

        function _wcPlanes(format, w, h) {
            return _wcVideoFormats[format].map(function(p) {
                var pw = Math.ceil(w / p[0]);
                var ph = Math.ceil(h / p[1]);
                return { width: pw, height: ph, stride: pw * p[2], size: pw * p[2] * ph };
            });
        }

        class VideoFrame {
            constructor(data, init) {
                init = init || {};
                var fmt, cw, ch, bytes;
                if (data instanceof VideoFrame) {
                    if (data._closed) {
                        throw new InvalidStateError('VideoFrame: source frame is closed');
                    }
                    fmt = data.format;
                    cw = data.codedWidth;
                    ch = data.codedHeight;
                    bytes = data._data.slice();
                    this._visible = { x: data.visibleRect.x, y: data.visibleRect.y,
                        width: data.visibleRect.width, height: data.visibleRect.height };
                    this.colorSpace = new VideoColorSpace(data.colorSpace);
                    this.rotation = data.rotation;
                    this.flip = data.flip;
                    this.timestamp = init.timestamp === undefined ? data.timestamp : init.timestamp;
                    this.duration = init.duration === undefined ? data.duration : init.duration;
                } else if (ArrayBuffer.isView(data) || data instanceof ArrayBuffer) {
                    fmt = init.format;
                    if (!_wcVideoFormats[fmt]) {
                        throw new TypeError('VideoFrame: invalid format');
                    }
                    cw = init.codedWidth;
                    ch = init.codedHeight;
                    if (!(cw > 0) || !(ch > 0)) {
                        throw new TypeError('VideoFrame: codedWidth/codedHeight must be positive');
                    }
                    if (init.timestamp === undefined) {
                        throw new TypeError('VideoFrame: timestamp is required');
                    }
                    var need = _wcPlanes(fmt, cw, ch).reduce(function(a, p) { return a + p.size; }, 0);
                    bytes = _wcBytes(data);
                    if (bytes.byteLength < need) {
                        throw new TypeError('VideoFrame: data is too small for format and size');
                    }
                    this._visible = { x: 0, y: 0, width: cw, height: ch };
                    this.colorSpace = new VideoColorSpace(init.colorSpace);
                    this.rotation = init.rotation || 0;
                    this.flip = !!init.flip;
                    this.timestamp = init.timestamp;
                    this.duration = init.duration === undefined ? null : init.duration;
                } else if (data !== null && typeof data === 'object' &&
                           typeof (data.width !== undefined ? data.width : data.displayWidth) === 'number') {
                    // Canvas / ImageBitmap / OffscreenCanvas source: RGBA snapshot.
                    cw = data.width !== undefined ? data.width : data.displayWidth;
                    ch = data.height !== undefined ? data.height : data.displayHeight;
                    if (!(cw > 0) || !(ch > 0)) {
                        throw new InvalidStateError('VideoFrame: source has no pixels');
                    }
                    if (init.timestamp === undefined) {
                        throw new TypeError('VideoFrame: timestamp is required');
                    }
                    fmt = 'RGBA';
                    bytes = new Uint8Array(cw * ch * 4);
                    try {
                        var ctx = typeof data.getContext === 'function' ? data.getContext('2d') : null;
                        if (ctx && typeof ctx.getImageData === 'function') {
                            bytes = new Uint8Array(ctx.getImageData(0, 0, cw, ch).data);
                        }
                    } catch (e) { /* keep the zero-filled snapshot */ }
                    this._visible = { x: 0, y: 0, width: cw, height: ch };
                    this.colorSpace = new VideoColorSpace(init.colorSpace || { primaries: 'bt709',
                        transfer: 'iec61966-2-1', matrix: 'rgb', fullRange: true });
                    this.rotation = init.rotation || 0;
                    this.flip = !!init.flip;
                    this.timestamp = init.timestamp;
                    this.duration = init.duration === undefined ? null : init.duration;
                } else {
                    throw new TypeError('VideoFrame: unsupported source');
                }
                if (init.visibleRect) {
                    var r = init.visibleRect;
                    if (r.x < 0 || r.y < 0 || !(r.width > 0) || !(r.height > 0) ||
                        r.x + r.width > cw || r.y + r.height > ch) {
                        throw new RangeError('VideoFrame: visibleRect outside coded size');
                    }
                    this._visible = { x: r.x, y: r.y, width: r.width, height: r.height };
                }
                this.format = fmt;
                this.codedWidth = cw;
                this.codedHeight = ch;
                this.displayWidth = init.displayWidth || (this.rotation % 180 ? this._visible.height : this._visible.width);
                this.displayHeight = init.displayHeight || (this.rotation % 180 ? this._visible.width : this._visible.height);
                this._data = bytes;
                this._closed = false;
            }
            get codedRect() {
                return this._closed ? null : { x: 0, y: 0, width: this.codedWidth, height: this.codedHeight };
            }
            get visibleRect() {
                return this._closed ? null : {
                    x: this._visible.x, y: this._visible.y,
                    width: this._visible.width, height: this._visible.height
                };
            }
            metadata() {
                return {};
            }
            allocationSize(options) {
                if (this._closed) {
                    throw new InvalidStateError('VideoFrame is closed');
                }
                return this._layout(options).total;
            }
            _layout(options) {
                var rect = (options && options.rect) || this._visible;
                if (rect.x < 0 || rect.y < 0 || !(rect.width > 0) || !(rect.height > 0) ||
                    rect.x + rect.width > this.codedWidth || rect.y + rect.height > this.codedHeight) {
                    throw new TypeError('VideoFrame: rect outside coded size');
                }
                var desc = _wcVideoFormats[this.format];
                var planes = _wcPlanes(this.format, rect.width, rect.height);
                if (options && options.layout !== undefined && options.layout.length !== planes.length) {
                    throw new TypeError('VideoFrame: layout must have one entry per plane');
                }
                var offset = 0;
                var out = [];
                for (var i = 0; i < planes.length; i++) {
                    var l = options && options.layout ? options.layout[i] : null;
                    var stride = l ? l.stride : planes[i].stride;
                    var off = l ? l.offset : offset;
                    if (stride < planes[i].stride) {
                        throw new TypeError('VideoFrame: layout stride too small');
                    }
                    out.push({ offset: off, stride: stride, rows: planes[i].height,
                        rowBytes: planes[i].stride, sx: desc[i][0], sy: desc[i][1], bpp: desc[i][2] });
                    offset = off + stride * planes[i].height;
                }
                var total = 0;
                out.forEach(function(p) { total = Math.max(total, p.offset + p.stride * p.rows); });
                return { rect: rect, planes: out, total: total };
            }
            async copyTo(destination, options) {
                if (this._closed) {
                    throw new InvalidStateError('VideoFrame is closed');
                }
                var lay = this._layout(options);
                var dst = _wcDest(destination, lay.total);
                var coded = _wcPlanes(this.format, this.codedWidth, this.codedHeight);
                var srcOffset = 0;
                var result = [];
                for (var i = 0; i < lay.planes.length; i++) {
                    var p = lay.planes[i];
                    var cx = Math.floor(lay.rect.x / p.sx) * p.bpp;
                    var cy = Math.floor(lay.rect.y / p.sy);
                    for (var row = 0; row < p.rows; row++) {
                        var s = srcOffset + (cy + row) * coded[i].stride + cx;
                        dst.set(this._data.subarray(s, s + p.rowBytes), p.offset + row * p.stride);
                    }
                    srcOffset += coded[i].size;
                    result.push({ offset: p.offset, stride: p.stride });
                }
                return result;
            }
            close() {
                this._closed = true;
                this._data = new Uint8Array(0);
                this.format = null;
                this.codedWidth = 0;
                this.codedHeight = 0;
                this.displayWidth = 0;
                this.displayHeight = 0;
            }
            clone() {
                if (this._closed) {
                    throw new InvalidStateError('VideoFrame is closed');
                }
                return new VideoFrame(this, {});
            }
        }

        var _wcAudioBytes = { u8: 1, s16: 2, s32: 4, f32: 4,
            'u8-planar': 1, 's16-planar': 2, 's32-planar': 4, 'f32-planar': 4 };

        class AudioData {
            constructor(init) {
                if (init === null || typeof init !== 'object') {
                    throw new TypeError('AudioData: init dictionary required');
                }
                if (!_wcAudioBytes[init.format]) {
                    throw new TypeError('AudioData: invalid format');
                }
                if (!(init.sampleRate > 0)) {
                    throw new TypeError('AudioData: sampleRate must be positive');
                }
                if (!(init.numberOfFrames > 0) || !(init.numberOfChannels > 0)) {
                    throw new TypeError('AudioData: numberOfFrames/numberOfChannels must be positive');
                }
                if (init.timestamp === undefined) {
                    throw new TypeError('AudioData: timestamp is required');
                }
                var bytes = _wcBytes(init.data);
                var need = _wcAudioBytes[init.format] * init.numberOfFrames * init.numberOfChannels;
                if (bytes.byteLength < need) {
                    throw new TypeError('AudioData: data is too small');
                }
                this.format = init.format;
                this.sampleRate = init.sampleRate;
                this.numberOfFrames = init.numberOfFrames;
                this.numberOfChannels = init.numberOfChannels;
                this.timestamp = init.timestamp;
                this.duration = Math.round(init.numberOfFrames / init.sampleRate * 1e6);
                this._data = bytes.subarray(0, need);
                this._closed = false;
            }
            _copyPlan(options) {
                if (this._closed) {
                    throw new InvalidStateError('AudioData is closed');
                }
                options = options || {};
                var planar = /-planar$/.test(this.format);
                var idx = options.planeIndex;
                if (idx === undefined) {
                    throw new TypeError('AudioData: planeIndex is required');
                }
                if (idx >= (planar ? this.numberOfChannels : 1)) {
                    throw new RangeError('AudioData: planeIndex out of range');
                }
                var off = options.frameOffset || 0;
                if (off >= this.numberOfFrames) {
                    throw new RangeError('AudioData: frameOffset out of range');
                }
                var count = options.frameCount === undefined ? this.numberOfFrames - off : options.frameCount;
                if (off + count > this.numberOfFrames) {
                    throw new RangeError('AudioData: frameCount out of range');
                }
                var bps = _wcAudioBytes[this.format];
                var perFrame = planar ? 1 : this.numberOfChannels;
                return { bytes: count * perFrame * bps,
                    start: planar ? (idx * this.numberOfFrames + off) * bps : off * perFrame * bps };
            }
            allocationSize(options) {
                return this._copyPlan(options).bytes;
            }
            copyTo(destination, options) {
                var plan = this._copyPlan(options);
                _wcDest(destination, plan.bytes).set(this._data.subarray(plan.start, plan.start + plan.bytes));
            }
            close() {
                this._closed = true;
                this._data = new Uint8Array(0);
                this.format = null;
                this.sampleRate = 0;
                this.numberOfFrames = 0;
                this.numberOfChannels = 0;
                this.duration = 0;
            }
            clone() {
                if (this._closed) {
                    throw new InvalidStateError('AudioData is closed');
                }
                return new AudioData({
                    format: this.format,
                    sampleRate: this.sampleRate,
                    numberOfFrames: this.numberOfFrames,
                    numberOfChannels: this.numberOfChannels,
                    timestamp: this.timestamp,
                    data: this._data
                });
            }
        }

        globalThis.VideoEncoder = VideoEncoder;
        globalThis.VideoDecoder = VideoDecoder;
        globalThis.AudioEncoder = AudioEncoder;
        globalThis.AudioDecoder = AudioDecoder;
        globalThis.EncodedVideoChunk = EncodedVideoChunk;
        globalThis.EncodedAudioChunk = EncodedAudioChunk;
        globalThis.VideoFrame = VideoFrame;
        globalThis.AudioData = AudioData;
        globalThis.VideoColorSpace = VideoColorSpace;
    "#;
    rt.eval(webcodecs_shim)?;

    Ok(())

}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal `DOMException` stub — the WebCodecs shim's error classes
    /// (`NotSupportedError`/`OperationError`/`InvalidStateError`) extend it.
    fn with_webcodecs_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            r#"
            function DOMException(message, name) {
              Error.call(this, message);
              this.message = message;
              this.name = name || 'Error';
            }
            DOMException.prototype = Object.create(Error.prototype);
            DOMException.prototype.constructor = DOMException;
            globalThis.DOMException = DOMException;
            "#,
        )
        .unwrap();
        install_webcodecs_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn webcodecs_api_installs() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof VideoEncoder === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn video_decoder_exists() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof VideoDecoder === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn encoded_video_chunk_exists() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof EncodedVideoChunk === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn video_frame_exists() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof VideoFrame === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn audio_data_exists() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof AudioData === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn not_supported_error_exists() {
        with_webcodecs_api(|rt| {
            let result = rt.eval("typeof NotSupportedError === 'function'").unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn video_encoder_configure_does_not_throw() {
        // Graceful degradation (U-4 stage 2): configure() must NOT throw
        // synchronously — unsupported codecs are reported via the async error
        // callback so SPAs don't white-screen. Feature detection still works
        // through isConfigSupported() → false.
        with_webcodecs_api(|rt| {
            let state = rt
                .eval(
                    r#"
                const enc = new VideoEncoder(function(){}, function(){});
                enc.configure({codec: 'vp9'});
                enc._state
            "#,
                )
                .unwrap();
            assert_eq!(state, JsValue::String("configured".to_owned()));
        });
    }

    #[test]
    fn is_config_supported_resolves_false() {
        with_webcodecs_api(|rt| {
            // The promise should resolve (not reject); feature detection path.
            let is_promise = rt
                .eval("VideoEncoder.isConfigSupported({codec:'vp9'}) instanceof Promise")
                .unwrap();
            assert_eq!(is_promise, JsValue::Bool(true));
        });
    }

    fn eval_bool(js: &str) -> bool {
        let mut out = false;
        with_webcodecs_api(|rt| {
            out = rt.eval(js).unwrap() == JsValue::Bool(true);
        });
        out
    }

    #[test]
    fn encoded_chunk_copy_to_copies_and_checks_size() {
        assert!(eval_bool(
            "var c = new EncodedVideoChunk({type:'key', timestamp:0, data:new Uint8Array([10,20,30,40])});
             var d = new Uint8Array(4); c.copyTo(d);
             var threw = false; try { c.copyTo(new Uint8Array(2)); } catch (e) { threw = e instanceof TypeError; }
             d[0] === 10 && d[3] === 40 && threw"
        ));
    }

    #[test]
    fn video_frame_from_buffer_has_geometry_and_copies() {
        assert!(eval_bool(
            "var data = new Uint8Array(4*2*4).map(function(_, i) { return i; });
             var f = new VideoFrame(data, {format:'RGBA', codedWidth:4, codedHeight:2, timestamp:0});
             var out = new Uint8Array(f.allocationSize());
             f.copyTo(out);
             f.allocationSize() === 32 && f.displayWidth === 4 && f.visibleRect.height === 2 &&
             typeof VideoColorSpace === 'function' && f.colorSpace instanceof VideoColorSpace"
        ));
    }

    #[test]
    fn video_frame_i420_allocation_size_odd() {
        assert!(eval_bool(
            "var f = new VideoFrame(new Uint8Array(100), {format:'I420', codedWidth:5, codedHeight:3, timestamp:0});
             f.allocationSize() === 15 + 2 * 6"
        ));
    }

    #[test]
    fn video_frame_from_canvas_like_reads_size() {
        assert!(eval_bool(
            "var f = new VideoFrame({width:64, height:32}, {timestamp:0});
             f.codedWidth === 64 && f.codedHeight === 32 && f.format === 'RGBA'"
        ));
    }

    #[test]
    fn audio_data_copy_to_returns_stored_samples() {
        assert!(eval_bool(
            "var a = new AudioData({format:'f32-planar', sampleRate:8000, numberOfFrames:2, numberOfChannels:2,
                timestamp:0, data:new Float32Array([1,2,3,4])});
             var d = new Float32Array(2); a.copyTo(d, {planeIndex:1});
             var bad = false; try { a.copyTo(d, {planeIndex:2}); } catch (e) { bad = e instanceof RangeError; }
             d[0] === 3 && d[1] === 4 && a.allocationSize({planeIndex:0}) === 8 && bad"
        ));
    }
}
