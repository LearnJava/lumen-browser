//! Shape Detection API (W3C Shape Detection API §3-4)
//!
//! Phase 0 stub: FaceDetector, BarcodeDetector, TextDetector all return empty arrays.
//! No actual detection is performed.

use rquickjs::Ctx;

pub fn install_shape_detection_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SHAPE_DETECTION_SHIM)?;
    Ok(())
}

/// JavaScript shim: Shape Detection API (Phase 0 - always returns empty arrays)
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

#[cfg(test)]
mod tests {
    use lumen_core::JsRuntime as _;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn make_rt() -> crate::QuickJsRuntime {
        let rt = crate::QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, false)
            .unwrap();
        rt
    }

    #[test]
    fn face_detector_class_exists() {
        let rt = make_rt();
        match rt.eval("typeof FaceDetector === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("FaceDetector class check failed: {other:?}"),
        }
    }

    #[test]
    fn barcode_detector_class_exists() {
        let rt = make_rt();
        match rt.eval("typeof BarcodeDetector === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("BarcodeDetector class check failed: {other:?}"),
        }
    }

    #[test]
    fn text_detector_class_exists() {
        let rt = make_rt();
        match rt.eval("typeof TextDetector === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("TextDetector class check failed: {other:?}"),
        }
    }

    #[test]
    fn face_detector_has_detect_method() {
        let rt = make_rt();
        match rt.eval("typeof new FaceDetector().detect === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("FaceDetector detect method check failed: {other:?}"),
        }
    }

    #[test]
    fn barcode_detector_has_detect_method() {
        let rt = make_rt();
        match rt.eval("typeof new BarcodeDetector().detect === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("BarcodeDetector detect method check failed: {other:?}"),
        }
    }

    #[test]
    fn barcode_detector_has_get_supported_formats() {
        let rt = make_rt();
        match rt.eval("typeof BarcodeDetector.getSupportedFormats === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("BarcodeDetector getSupportedFormats check failed: {other:?}"),
        }
    }

    #[test]
    fn text_detector_has_detect_method() {
        let rt = make_rt();
        match rt.eval("typeof new TextDetector().detect === 'function'") {
            Ok(lumen_core::JsValue::Bool(true)) => (),
            other => panic!("TextDetector detect method check failed: {other:?}"),
        }
    }
}
