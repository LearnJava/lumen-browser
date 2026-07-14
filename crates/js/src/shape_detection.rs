//! Shape Detection API (W3C Shape Detection API §3-4)
//!
//! Phase 0 stub: FaceDetector, BarcodeDetector, TextDetector all return empty arrays.
//! No actual detection is performed.

/// V8 port of the former rquickjs `install_shape_detection_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-7): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_shape_detection_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SHAPE_DETECTION_SHIM)?;
    Ok(())
}

/// JavaScript shim: Shape Detection API (Phase 0 - always returns empty arrays)
#[cfg(feature = "v8-backend")]
const SHAPE_DETECTION_SHIM: &str = r#"
(function() {
  // FaceDetector stub
  class FaceDetector {
    constructor(options) {
      this.options = options || {};
      this.maxDetectedFaces = options?.maxDetectedFaces || 10;
    }

    async detect(image) {
      if (!image) {
        throw new TypeError('detect: image argument is required');
      }
      // Phase 0: Always return empty array - no detection
      return [];
    }
  }

  // BarcodeDetector stub
  class BarcodeDetector {
    constructor(options) {
      this.options = options || {};
      // Phase 0: formats are ignored
      this.formats = options?.formats || [];
    }

    async detect(image) {
      if (!image) {
        throw new TypeError('detect: image argument is required');
      }
      // Phase 0: Always return empty array - no detection
      return [];
    }

    static async getSupportedFormats() {
      // Phase 0: Return empty array - no formats supported
      return [];
    }
  }

  // TextDetector stub
  class TextDetector {
    constructor(options) {
      this.options = options || {};
    }

    async detect(image) {
      if (!image) {
        throw new TypeError('detect: image argument is required');
      }
      // Phase 0: Always return empty array - no detection
      return [];
    }
  }

  // Export to global scope
  if (typeof window !== 'undefined') {
    window.FaceDetector = FaceDetector;
    window.BarcodeDetector = BarcodeDetector;
    window.TextDetector = TextDetector;
  }

  if (typeof globalThis !== 'undefined') {
    globalThis.FaceDetector = FaceDetector;
    globalThis.BarcodeDetector = BarcodeDetector;
    globalThis.TextDetector = TextDetector;
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_shape_detection(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis;").unwrap();
        install_shape_detection_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn face_detector_class_exists() {
        with_shape_detection(|rt| {
            let ok = rt.eval("typeof FaceDetector === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn barcode_detector_class_exists() {
        with_shape_detection(|rt| {
            let ok = rt.eval("typeof BarcodeDetector === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn text_detector_class_exists() {
        with_shape_detection(|rt| {
            let ok = rt.eval("typeof TextDetector === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn face_detector_has_detect_method() {
        with_shape_detection(|rt| {
            let ok = rt
                .eval("typeof new FaceDetector().detect === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn barcode_detector_has_detect_method() {
        with_shape_detection(|rt| {
            let ok = rt
                .eval("typeof new BarcodeDetector().detect === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn barcode_detector_has_get_supported_formats() {
        with_shape_detection(|rt| {
            let ok = rt
                .eval("typeof BarcodeDetector.getSupportedFormats === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn text_detector_has_detect_method() {
        with_shape_detection(|rt| {
            let ok = rt
                .eval("typeof new TextDetector().detect === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
