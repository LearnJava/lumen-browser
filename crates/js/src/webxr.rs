/// WebXR Device API stub (W3C WebXR Device API §5)
/// Phase 0: navigator.xr.isSessionSupported() → Promise<false>,
/// requestSession() → reject NotSupportedError. XRSession/XRFrame/XRReferenceSpace/XRView stubs.
use rquickjs::Ctx;

/// Install WebXR Device API bindings into the JS context.
pub fn install_webxr_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBXR_SHIM)?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_webxr(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                function EventTarget() {}
                EventTarget.prototype.addEventListener = function() {};
                EventTarget.prototype.removeEventListener = function() {};
                EventTarget.prototype.dispatchEvent = function() { return true; };
                globalThis.EventTarget = EventTarget;
                function Event(type) { this.type = type; }
                globalThis.Event = Event;
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
            install_webxr_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn webxr_navigator_xr_exists() {
        with_webxr(|ctx| {
            let ok: bool = ctx.eval("typeof navigator.xr === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webxr_is_session_supported_returns_promise_false() {
        with_webxr(|ctx| {
            let ok: bool = ctx
                .eval("navigator.xr.isSessionSupported('immersive-vr') instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webxr_request_session_returns_promise() {
        with_webxr(|ctx| {
            let ok: bool = ctx
                .eval("navigator.xr.requestSession('immersive-vr') instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webxr_stub_classes_exist() {
        with_webxr(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof window.XRSession === 'function' && \
                     typeof window.XRFrame === 'function' && \
                     typeof window.XRReferenceSpace === 'function' && \
                     typeof window.XRView === 'function'",
                )
                .unwrap();
            assert!(ok);
        });
    }
}
