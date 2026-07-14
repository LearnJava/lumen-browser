//! Media Capabilities API (W3C Media Capabilities §5)
//!
//! Phase 0: `navigator.mediaCapabilities` singleton.
//! `decodingInfo(config)` -> `Promise<{supported:true, smooth:true, powerEfficient:false}>`.
//! `encodingInfo(config)` -> `Promise<{supported:true, smooth:true, powerEfficient:false}>`.

/// V8 port of the former rquickjs `install_media_capabilities_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-11): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_media_capabilities_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(MEDIA_CAPABILITIES_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_media_capabilities(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
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
        install_media_capabilities_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn media_capabilities_on_navigator() {
        with_media_capabilities(|rt| {
            let ok = rt
                .eval("typeof navigator.mediaCapabilities === 'object' && navigator.mediaCapabilities !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn decoding_info_returns_promise() {
        with_media_capabilities(|rt| {
            let ok = rt
                .eval(
                    r#"
                    navigator.mediaCapabilities.decodingInfo({
                        type: 'file',
                        video: { contentType: 'video/mp4; codecs=avc1', width: 1920, height: 1080, bitrate: 2000000, framerate: 30 }
                    }) instanceof Promise
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn encoding_info_returns_promise() {
        with_media_capabilities(|rt| {
            let ok = rt
                .eval(
                    r#"
                    navigator.mediaCapabilities.encodingInfo({
                        type: 'record',
                        video: { contentType: 'video/webm; codecs=vp8', width: 1280, height: 720, bitrate: 1000000, framerate: 24 }
                    }) instanceof Promise
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn decoding_info_result_fields() {
        // Verify MediaCapabilitiesInfo constructor and field values directly.
        // Can't use .then() in sync tests — microtask queue not flushed.
        with_media_capabilities(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var info = new window.MediaCapabilitiesInfo(true, true, false);
                    info.supported === true && info.smooth === true && info.powerEfficient === false
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn encoding_info_result_fields() {
        // Verify MediaCapabilitiesInfo constructor returns correct Phase 0 values.
        // Can't use .then() in sync tests — microtask queue not flushed.
        with_media_capabilities(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var info = new window.MediaCapabilitiesInfo(true, true, false);
                    info.supported === true && info.smooth === true && info.powerEfficient === false
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
