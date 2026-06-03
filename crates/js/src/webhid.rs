/// WebHID API stub (W3C WebHID §3-5)
/// Phase 0: navigator.hid.requestDevice() and all device operations reject (no USB/HID support)
use rquickjs::Ctx;

pub fn install_webhid_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBHID_SHIM)?;
    Ok(())
}

/// JavaScript shim: WebHID stub (Phase 0 - all operations reject with NotSupportedError)
const WEBHID_SHIM: &str = r#"
(function() {
  // HIDConnectionEvent class
  class HIDConnectionEvent extends Event {
    constructor(type, device) {
      super(type);
      this.device = device;
    }
  }
  window.HIDConnectionEvent = HIDConnectionEvent;

  // HIDDevice class - represents a connected HID device
  class HIDDevice extends EventTarget {
    constructor(vendorId, productId, productName, collections = []) {
      super();
      this.vendorId = vendorId;
      this.productId = productId;
      this.productName = productName;
      this.collections = collections;
      this.opened = false;
      this.oninputreport = null;
    }

    async open() {
      throw new DOMException('WebHID not supported (Phase 0)', 'NotSupportedError');
    }

    async close() {
      throw new DOMException('WebHID not supported (Phase 0)', 'NotSupportedError');
    }

    async sendReport(reportId, data) {
      throw new DOMException('WebHID not supported (Phase 0)', 'NotSupportedError');
    }

    async sendFeatureReport(reportId, data) {
      throw new DOMException('WebHID not supported (Phase 0)', 'NotSupportedError');
    }

    async receiveFeatureReport(reportId) {
      throw new DOMException('WebHID not supported (Phase 0)', 'NotSupportedError');
    }
  }
  window.HIDDevice = HIDDevice;

  // HIDInput/Output/Feature Report classes
  class HIDReportItem {
    constructor(type, id, size) {
      this.reportType = type; // "input" | "output" | "feature"
      this.reportId = id;
      this.reportSize = size; // bytes
    }
  }

  // HIDCollectionInfo - describes a HID collection
  class HIDCollectionInfo {
    constructor(type, usage, children = []) {
      this.type = type; // "application" | "logical" | "report"
      this.usage = {
        usagePage: 0,
        usage: usage
      };
      this.children = children;
      this.inputReports = [];
      this.outputReports = [];
      this.featureReports = [];
    }
  }

  // HIDManager (navigator.hid)
  class HIDManager extends EventTarget {
    constructor() {
      super();
      this.onconnect = null;
      this.ondisconnect = null;
      this._devices = [];
    }

    async requestDevice(options = {}) {
      // Phase 0: Always reject
      throw new DOMException(
        'WebHID not supported (Phase 0 stub)',
        'NotSupportedError'
      );
    }

    async getDevices() {
      // Phase 0: Return empty array
      return [];
    }

    _dispatchConnectionEvent(device, isConnect) {
      const event = new HIDConnectionEvent(
        isConnect ? 'connect' : 'disconnect',
        device
      );
      this.dispatchEvent(event);
    }
  }

  // Install navigator.hid singleton
  Object.defineProperty(navigator, 'hid', {
    value: new HIDManager(),
    writable: false,
    enumerable: true
  });

  // Export classes to globalThis
  window.HIDManager = HIDManager;
  window.HIDCollectionInfo = HIDCollectionInfo;
  window.HIDReportItem = HIDReportItem;
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

    fn with_webhid_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal stubs so the shim doesn't fail.
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                // Minimal EventTarget stub
                function EventTarget() {}
                EventTarget.prototype.addEventListener = function() {};
                EventTarget.prototype.removeEventListener = function() {};
                EventTarget.prototype.dispatchEvent = function() {};
                globalThis.EventTarget = EventTarget;
                // Minimal Event stub
                function Event(type, init) {
                  this.type = type;
                  this.bubbles = (init && init.bubbles) || false;
                  this.cancelable = (init && init.cancelable) || false;
                }
                Event.prototype.constructor = Event;
                globalThis.Event = Event;
                // Minimal DOMException
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
            super::install_webhid_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn webhid_shim_defined() {
        assert!(!WEBHID_SHIM.is_empty());
    }

    #[test]
    fn webhid_navigator_hid_exists() {
        with_webhid_api(|ctx| {
            let ok: bool = ctx.eval("typeof navigator.hid === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webhid_get_devices_is_async() {
        with_webhid_api(|ctx| {
            let result: bool = ctx
                .eval("navigator.hid.getDevices() instanceof Promise")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn webhid_device_class_exists() {
        with_webhid_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.HIDDevice === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webhid_device_has_properties() {
        with_webhid_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            const dev = new window.HIDDevice(0x1234, 0x5678, "TestDev");
            dev.vendorId === 0x1234 &&
            dev.productId === 0x5678 &&
            dev.productName === "TestDev" &&
            dev.opened === false
            "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webhid_request_device_is_async() {
        with_webhid_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            navigator.hid.requestDevice instanceof Function &&
            navigator.hid.requestDevice({filters: []}) instanceof Promise
            "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webhid_manager_extends_event_target() {
        with_webhid_api(|ctx| {
            let ok: bool = ctx
                .eval("navigator.hid instanceof EventTarget")
                .unwrap();
            assert!(ok);
        });
    }
}
