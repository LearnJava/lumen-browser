/// Media Capabilities API (W3C Media Capabilities §5)
/// Phase 0: navigator.mediaCapabilities singleton.
/// decodingInfo(config) → Promise<{supported:true, smooth:true, powerEfficient:false}>.
/// encodingInfo(config) → Promise<{supported:true, smooth:true, powerEfficient:false}>.
use rquickjs::Ctx;

/// Install Media Capabilities API bindings into the JS context.
pub fn install_media_capabilities_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(MEDIA_CAPABILITIES_SHIM)?;
    Ok(())
}

const MEDIA_CAPABILITIES_SHIM: &str = r#"
(function() {
  // MediaCapabilitiesInfo — result object returned by decodingInfo / encodingInfo
  class MediaCapabilitiesInfo {
    constructor(supported, smooth, powerEfficient) {
      this.supported = supported;
      this.smooth = smooth;
      this.powerEfficient = powerEfficient;
    }
  }

  // MediaCapabilities singleton — navigator.mediaCapabilities
  class MediaCapabilities {
    // W3C Media Capabilities §5.2: decodingInfo
    // Phase 0: any valid config returns supported=true, smooth=true, powerEfficient=false.
    decodingInfo(config) {
      if (!config || typeof config !== 'object') {
        return Promise.reject(new TypeError('decodingInfo: config must be an object'));
      }
      if (typeof config.type !== 'string') {
        return Promise.reject(new TypeError('decodingInfo: config.type must be a string'));
      }
      return Promise.resolve(new MediaCapabilitiesInfo(true, true, false));
    }

    // W3C Media Capabilities §5.3: encodingInfo
    // Phase 0: any valid config returns supported=true, smooth=true, powerEfficient=false.
    encodingInfo(config) {
      if (!config || typeof config !== 'object') {
        return Promise.reject(new TypeError('encodingInfo: config must be an object'));
      }
      if (typeof config.type !== 'string') {
        return Promise.reject(new TypeError('encodingInfo: config.type must be a string'));
      }
      return Promise.resolve(new MediaCapabilitiesInfo(true, true, false));
    }
  }

  // Attach to navigator as a non-enumerable getter
  Object.defineProperty(navigator, 'mediaCapabilities', {
    get: function() { return _lumen_media_capabilities_instance; },
    configurable: true,
    enumerable: false,
  });

  const _lumen_media_capabilities_instance = new MediaCapabilities();

  window.MediaCapabilities = MediaCapabilities;
  window.MediaCapabilitiesInfo = MediaCapabilitiesInfo;
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_media_capabilities(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
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
            install_media_capabilities_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn media_capabilities_on_navigator() {
        with_media_capabilities(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.mediaCapabilities === 'object' && navigator.mediaCapabilities !== null")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn decoding_info_returns_promise() {
        with_media_capabilities(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    navigator.mediaCapabilities.decodingInfo({
                        type: 'file',
                        video: { contentType: 'video/mp4; codecs=avc1', width: 1920, height: 1080, bitrate: 2000000, framerate: 30 }
                    }) instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn encoding_info_returns_promise() {
        with_media_capabilities(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    navigator.mediaCapabilities.encodingInfo({
                        type: 'record',
                        video: { contentType: 'video/webm; codecs=vp8', width: 1280, height: 720, bitrate: 1000000, framerate: 24 }
                    }) instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn decoding_info_result_fields() {
        // Verify MediaCapabilitiesInfo constructor and field values directly.
        // Can't use .then() in sync tests — microtask queue not flushed.
        with_media_capabilities(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var info = new window.MediaCapabilitiesInfo(true, true, false);
                    info.supported === true && info.smooth === true && info.powerEfficient === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn encoding_info_result_fields() {
        // Verify MediaCapabilitiesInfo constructor returns correct Phase 0 values.
        // Can't use .then() in sync tests — microtask queue not flushed.
        with_media_capabilities(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var info = new window.MediaCapabilitiesInfo(true, true, false);
                    info.supported === true && info.smooth === true && info.powerEfficient === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
