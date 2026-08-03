//! WebXR Device API stub (W3C WebXR Device API §5)
//! Phase 0: navigator.xr.isSessionSupported() → Promise<false>,
//! requestSession() → reject NotSupportedError. XRSession/XRFrame/XRReferenceSpace/XRView stubs.

/// V8 port of the former rquickjs `install_webxr_bindings` (Ph3 V8 migration S5-S7):
/// identical JS shim, evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of
/// `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_webxr_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(WEBXR_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const WEBXR_SHIM: &str = r#"
(function() {
  // XRView stub — represents a single view (eye) within an XR frame
  class XRView {
    constructor(eye, transform, projectionMatrix) {
      this.eye = eye || 'none';
      this.transform = transform || null;
      this.projectionMatrix = projectionMatrix || new Float32Array(16);
      this.recommendedViewportScale = null;
    }
    requestViewportScale(scale) {}
  }
  window.XRView = XRView;

  // XRReferenceSpace stub — coordinate system for XR sessions
  class XRReferenceSpace extends EventTarget {
    constructor(type) {
      super();
      this.type = type || 'local';
      this.onreset = null;
    }
    getOffsetReferenceSpace(originOffset) {
      return new XRReferenceSpace(this.type);
    }
  }
  window.XRReferenceSpace = XRReferenceSpace;

  // XRFrame stub — snapshot of XR state for a single animation frame
  class XRFrame {
    constructor(session) {
      this.session = session;
      this.predictedDisplayTime = 0;
    }
    getViewerPose(referenceSpace) { return null; }
    getPose(space, baseSpace) { return null; }
    getHitTestResults(hitTestSource) { return []; }
  }
  window.XRFrame = XRFrame;

  // XRSession stub — an active XR session
  class XRSession extends EventTarget {
    constructor(mode) {
      super();
      this.visibilityState = 'hidden';
      this.frameRate = null;
      this.supportedFrameRates = null;
      this.renderState = { baseLayer: null, depthFar: 1000, depthNear: 0.1, inlineVerticalFieldOfView: null };
      this.inputSources = [];
      this.environmentBlendMode = 'opaque';
      this.interactionMode = 'screen-space';
      this.onend = null;
      this.oninputsourceschange = null;
      this.onselect = null;
      this.onselectstart = null;
      this.onselectend = null;
      this.onsqueeze = null;
      this.onsqueezestart = null;
      this.onsqueezeend = null;
      this.onvisibilitychange = null;
      this.onframeratechange = null;
      this._mode = mode;
    }

    requestAnimationFrame(callback) { return 0; }
    cancelAnimationFrame(handle) {}
    async requestReferenceSpace(type) {
      return new XRReferenceSpace(type);
    }
    async updateRenderState(state) {}
    async end() {
      this.dispatchEvent(new Event('end'));
    }
    updateTargetFrameRate(rate) { return Promise.resolve(); }
  }
  window.XRSession = XRSession;

  // XRSystem — navigator.xr singleton
  class XRSystem extends EventTarget {
    constructor() {
      super();
      this.ondevicechange = null;
    }

    isSessionSupported(mode) {
      return Promise.resolve(false);
    }

    requestSession(mode, options) {
      return Promise.reject(
        new DOMException('WebXR is not supported (Phase 0)', 'NotSupportedError')
      );
    }
  }

  Object.defineProperty(navigator, 'xr', {
    value: new XRSystem(),
    writable: false,
    enumerable: true,
    configurable: false
  });

  window.XRSystem = XRSystem;
})();
"#;
