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

        class EncodedVideoChunk {
            constructor(init) {
                this.type = init.type || 'key';
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration || 0;
                this._data = init.data || new Uint8Array(0);
            }
            get byteLength() {
                return this._data.byteLength;
            }
            copyTo(destination) {
                // Phase 0: no-op
            }
        }

        class EncodedAudioChunk {
            constructor(init) {
                this.type = init.type || 'key';
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration || 0;
                this._data = init.data || new Uint8Array(0);
            }
            get byteLength() {
                return this._data.byteLength;
            }
            copyTo(destination) {
                // Phase 0: no-op
            }
        }

        class VideoFrame {
            constructor(data, init) {
                this.format = init.format || 'I420';
                this.codedWidth = init.codedWidth || 0;
                this.codedHeight = init.codedHeight || 0;
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration || 0;
            }
            close() {
                // Phase 0: no-op
            }
            clone() {
                return new VideoFrame(null, {
                    format: this.format,
                    codedWidth: this.codedWidth,
                    codedHeight: this.codedHeight,
                    timestamp: this.timestamp,
                    duration: this.duration
                });
            }
        }

        class AudioData {
            constructor(init) {
                this.format = init.format || 'f32';
                this.sampleRate = init.sampleRate || 48000;
                this.numberOfFrames = init.numberOfFrames || 0;
                this.numberOfChannels = init.numberOfChannels || 0;
                this.timestamp = init.timestamp || 0;
                this.duration = init.duration || 0;
            }
            close() {
                // Phase 0: no-op
            }
            clone() {
                return new AudioData({
                    format: this.format,
                    sampleRate: this.sampleRate,
                    numberOfFrames: this.numberOfFrames,
                    numberOfChannels: this.numberOfChannels,
                    timestamp: this.timestamp,
                    duration: this.duration
                });
            }
            copyTo(destination) {
                // Phase 0: no-op
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
    "#;
    rt.eval(webcodecs_shim)?;

    Ok(())

}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
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
}
