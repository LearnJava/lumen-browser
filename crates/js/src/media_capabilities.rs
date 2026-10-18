//! Media Capabilities API (W3C Media Capabilities §5)
//!
//! `navigator.mediaCapabilities` singleton. Ответы согласованы с реальными
//! декодерами (BUG-1619, ADR-034 п.2): `decodingInfo` — `supported:false` для
//! `media-source` (MSE нет), `keySystemConfiguration` (EME нет) и контейнеров без
//! декодера; `encodingInfo` — всегда `supported:false` (кодировщиков нет).

/// V8 port of the former rquickjs `install_media_capabilities_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-11): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_media_capabilities_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.register_native(
        "__lumen_media_audio_can_play",
        crate::v8_compat::into_v8_fn1(|mime: String| -> String {
            lumen_core::ext::audio_can_play_type(&mime).to_owned()
        }),
    )?;
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

  // Нет ни MSE, ни EME, ни кодировщика: честные ответы вместо Phase 0 «всё supported».
  // Аудио — таблица декодеров Rust (__lumen_media_audio_can_play), видео — реальный
  // декодер за __lumen_video_can_play_type; нет привязки — нет декодера.
  function canDecode(kind, contentType) {
    if (typeof contentType !== 'string') return false;
    if (kind === 'audio') {
      return typeof __lumen_media_audio_can_play === 'function' &&
             __lumen_media_audio_can_play(contentType) !== '';
    }
    return typeof __lumen_video_can_play_type === 'function' &&
           __lumen_video_can_play_type(contentType) !== '';
  }

  function checkConfig(method, config, types) {
    if (!config || typeof config !== 'object') {
      throw new TypeError(method + ': config must be an object');
    }
    if (types.indexOf(config.type) < 0) {
      throw new TypeError(method + ': config.type must be one of ' + types.join(', '));
    }
    if (!config.audio && !config.video) {
      throw new TypeError(method + ': config needs an audio or video configuration');
    }
  }

  // MediaCapabilities singleton — navigator.mediaCapabilities
  class MediaCapabilities {
    // W3C Media Capabilities §5.2: decodingInfo
    decodingInfo(config) {
      try {
        checkConfig('decodingInfo', config, ['file', 'media-source', 'webrtc']);
      } catch (e) {
        return Promise.reject(e);
      }
      var supported = config.type !== 'media-source' && !config.keySystemConfiguration;
      if (supported && config.audio) supported = canDecode('audio', config.audio.contentType);
      if (supported && config.video) supported = canDecode('video', config.video.contentType);
      return Promise.resolve(new MediaCapabilitiesInfo(supported, supported, false));
    }

    // W3C Media Capabilities §5.3: encodingInfo — кодировщиков нет
    // (MediaRecorder.isTypeSupported и WebCodecs isConfigSupported тоже false).
    encodingInfo(config) {
      try {
        checkConfig('encodingInfo', config, ['record', 'webrtc']);
      } catch (e) {
        return Promise.reject(e);
      }
      return Promise.resolve(new MediaCapabilitiesInfo(false, false, false));
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
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
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

    /// Результат `decodingInfo`/`encodingInfo` как строка "supported" или "TypeError: …".
    /// Промис разрешается микротаском — читаем второй `eval`.
    fn settle(rt: &V8JsRuntime, call: &str) -> String {
        rt.eval(&format!(
            "globalThis.__r = 'pending'; {call}.then(function(i) {{ globalThis.__r = 'supported=' + i.supported + ',smooth=' + i.smooth; }}, function(e) {{ globalThis.__r = e.name + ': rejected'; }}); 0"
        ))
        .unwrap();
        match rt.eval("globalThis.__r").unwrap() {
            JsValue::String(s) => s,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn decoding_info_media_source_unsupported() {
        with_media_capabilities(|rt| {
            let r = settle(
                rt,
                "navigator.mediaCapabilities.decodingInfo({type:'media-source', audio:{contentType:'audio/mpeg'}})",
            );
            assert_eq!(r, "supported=false,smooth=false");
        });
    }

    #[test]
    fn decoding_info_follows_audio_table() {
        with_media_capabilities(|rt| {
            let mp3 = settle(
                rt,
                "navigator.mediaCapabilities.decodingInfo({type:'file', audio:{contentType:'audio/mpeg'}})",
            );
            assert_eq!(mp3, "supported=true,smooth=true");
            let opus = settle(
                rt,
                "navigator.mediaCapabilities.decodingInfo({type:'file', audio:{contentType:'audio/ogg; codecs=opus'}})",
            );
            assert_eq!(opus, "supported=false,smooth=false");
        });
    }

    #[test]
    fn decoding_info_drm_and_missing_video_decoder_unsupported() {
        with_media_capabilities(|rt| {
            let drm = settle(
                rt,
                "navigator.mediaCapabilities.decodingInfo({type:'file', audio:{contentType:'audio/mpeg'}, keySystemConfiguration:{keySystem:'com.widevine.alpha'}})",
            );
            assert_eq!(drm, "supported=false,smooth=false");
            // Нативного видео-декодера в тестовом рантайме нет.
            let video = settle(
                rt,
                "navigator.mediaCapabilities.decodingInfo({type:'file', video:{contentType:'video/mp4; codecs=avc1.42E01E'}})",
            );
            assert_eq!(video, "supported=false,smooth=false");
        });
    }

    #[test]
    fn info_without_audio_or_video_rejects() {
        with_media_capabilities(|rt| {
            assert_eq!(
                settle(rt, "navigator.mediaCapabilities.decodingInfo({type:'file'})"),
                "TypeError: rejected"
            );
            assert_eq!(
                settle(rt, "navigator.mediaCapabilities.encodingInfo({type:'bogus', audio:{}})"),
                "TypeError: rejected"
            );
        });
    }

    #[test]
    fn encoding_info_unsupported_without_encoder() {
        with_media_capabilities(|rt| {
            let r = settle(
                rt,
                "navigator.mediaCapabilities.encodingInfo({type:'record', video:{contentType:'video/webm; codecs=vp8'}})",
            );
            assert_eq!(r, "supported=false,smooth=false");
        });
    }
}
