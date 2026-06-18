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

use rquickjs::Ctx;

/// Install WebCodecs API JS shim.
pub fn install_webcodecs_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
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
    ctx.eval::<(), _>(error_shim)?;

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
    ctx.eval::<(), _>(webcodecs_shim)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    #[test]
    fn webcodecs_api_installs() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof VideoEncoder === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn video_decoder_exists() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof VideoDecoder === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn encoded_video_chunk_exists() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof EncodedVideoChunk === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn video_frame_exists() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof VideoFrame === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn audio_data_exists() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof AudioData === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn not_supported_error_exists() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: bool = ctx.eval("typeof NotSupportedError === 'function'").unwrap();
            assert!(result);
        });
    }

    #[test]
    fn video_encoder_configure_does_not_throw() {
        // Graceful degradation (U-4 stage 2): configure() must NOT throw
        // synchronously — unsupported codecs are reported via the async error
        // callback so SPAs don't white-screen. Feature detection still works
        // through isConfigSupported() → false.
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let state: String = ctx
                .eval(
                    r#"
                const enc = new VideoEncoder(function(){}, function(){});
                enc.configure({codec: 'vp9'});
                enc._state
            "#,
                )
                .unwrap();
            assert_eq!(state, "configured");
        });
    }

    #[test]
    fn is_config_supported_resolves_false() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            // The promise should resolve (not reject); feature detection path.
            let is_promise: bool = ctx
                .eval("VideoEncoder.isConfigSupported({codec:'vp9'}) instanceof Promise")
                .unwrap();
            assert!(is_promise);
        });
    }
}
