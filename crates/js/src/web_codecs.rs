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
        globalThis.NotSupportedError = NotSupportedError;
        globalThis.OperationError = OperationError;
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
                throw new NotSupportedError('VideoEncoder: no codec support in Phase 0');
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
                throw new NotSupportedError('VideoDecoder: no codec support in Phase 0');
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
                throw new NotSupportedError('AudioEncoder: no codec support in Phase 0');
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
                throw new NotSupportedError('AudioDecoder: no codec support in Phase 0');
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
    fn video_encoder_configure_throws() {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            super::install_webcodecs_bindings(&ctx).unwrap();
            let result: Result<(), _> = ctx.eval(r#"
                const enc = new VideoEncoder(null, null);
                enc.configure({codec: 'vp9'});
            "#);
            assert!(result.is_err());
        });
    }
}
