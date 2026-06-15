/// WebUSB API stub (W3C WebUSB §2-3)
/// Phase 0: navigator.usb.requestDevice() and all device operations reject (no USB support)
use rquickjs::Ctx;

pub fn install_webusb_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBUSB_SHIM)?;
    Ok(())
}

/// JavaScript shim: WebUSB stub (Phase 0 - all operations reject with NotSupportedError)
const WEBUSB_SHIM: &str = r#"
(function() {
  // USBConnectionEvent class
  class USBConnectionEvent extends Event {
    constructor(type, device) {
      super(type);
      this.device = device;
    }
  }
  window.USBConnectionEvent = USBConnectionEvent;

  // USBTransferStatus enum (for transfer results)
  const USBTransferStatus = {
    ok: "ok",
    stall: "stall",
    babble: "babble"
  };

  // USBTransferInResult
  class USBTransferInResult {
    constructor(status = "ok", data = null) {
      this.status = status;
      this.data = data;
    }
  }
  window.USBTransferInResult = USBTransferInResult;

  // USBTransferOutResult
  class USBTransferOutResult {
    constructor(status = "ok", bytesWritten = 0) {
      this.status = status;
      this.bytesWritten = bytesWritten;
    }
  }
  window.USBTransferOutResult = USBTransferOutResult;

  // USBInterface
  class USBInterface {
    constructor(configValue, interfaceNumber) {
      this.configValue = configValue;
      this.interfaceNumber = interfaceNumber;
      this.alternate = null;
      this.alternates = [];
      this.claimed = false;
    }
  }
  window.USBInterface = USBInterface;

  // USBConfiguration
  class USBConfiguration {
    constructor(device, configValue) {
      this.device = device;
      this.configValue = configValue;
      this.interfaces = [];
    }
  }
  window.USBConfiguration = USBConfiguration;

  // USBEndpoint
  class USBEndpoint {
    constructor(interfaceNumber, endpointNumber, direction) {
      this.endpointNumber = endpointNumber;
      this.direction = direction; // "in" | "out"
      this.type = "bulk"; // "control" | "interrupt" | "bulk" | "isochronous"
      this.packetSize = 64;
    }
  }
  window.USBEndpoint = USBEndpoint;

  // USBAlternateInterface
  class USBAlternateInterface {
    constructor(interfaceNumber, alternateSetting) {
      this.interfaceNumber = interfaceNumber;
      this.alternateSetting = alternateSetting;
      this.interfaceClass = 0;
      this.interfaceSubclass = 0;
      this.interfaceProtocol = 0;
      this.interfaceName = null;
      this.endpoints = [];
    }
  }
  window.USBAlternateInterface = USBAlternateInterface;

  // USBDevice class - represents a connected USB device
  class USBDevice extends EventTarget {
    constructor(vendorId, productId, productName) {
      super();
      this.vendorId = vendorId;
      this.productId = productId;
      this.productName = productName || "";
      this.manufacturerName = "";
      this.serialNumber = "";
      this.configurations = [];
      this.configuration = null;
      this.opened = false;
      this.onconnect = null;
      this.ondisconnect = null;
    }

    async open() {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async close() {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async selectConfiguration(configValue) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async claimInterface(interfaceNumber) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async releaseInterface(interfaceNumber) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async transferIn(endpointNumber, length) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async transferOut(endpointNumber, data) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async controlTransferIn(setup, length) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async controlTransferOut(setup, data) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async clearHalt(direction, endpointNumber) {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }

    async reset() {
      throw new DOMException('WebUSB not supported (Phase 0)', 'NotSupportedError');
    }
  }
  window.USBDevice = USBDevice;

  // USBManager (navigator.usb)
  class USBManager extends EventTarget {
    constructor() {
      super();
      this.onconnect = null;
      this.ondisconnect = null;
      this._devices = [];
    }

    async requestDevice(options = {}) {
      // Phase 0: Always reject
      throw new DOMException(
        'WebUSB not supported (Phase 0 stub)',
        'NotSupportedError'
      );
    }

    async getDevices() {
      // Phase 0: Return empty array
      return [];
    }

    _dispatchConnectionEvent(device, isConnect) {
      const event = new USBConnectionEvent(
        isConnect ? 'connect' : 'disconnect',
        device
      );
      this.dispatchEvent(event);
    }
  }

  // Install navigator.usb singleton
  Object.defineProperty(navigator, 'usb', {
    value: new USBManager(),
    writable: false,
    enumerable: true
  });

  // Export classes to globalThis
  window.USBManager = USBManager;
  window.USBTransferStatus = USBTransferStatus;
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

    fn with_webusb_api(f: impl FnOnce(&rquickjs::Ctx)) {
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
            super::install_webusb_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn webusb_shim_defined() {
        assert!(!WEBUSB_SHIM.is_empty());
    }

    #[test]
    fn webusb_navigator_usb_exists() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx.eval("typeof navigator.usb === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webusb_get_devices_is_async() {
        with_webusb_api(|ctx| {
            let result: bool = ctx
                .eval("navigator.usb.getDevices() instanceof Promise")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn webusb_device_class_exists() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.USBDevice === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webusb_device_has_properties() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            const dev = new window.USBDevice(0x1234, 0x5678, "TestDev");
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
    fn webusb_request_device_is_async() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
            navigator.usb.requestDevice instanceof Function &&
            navigator.usb.requestDevice({filters: []}) instanceof Promise
            "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webusb_manager_extends_event_target() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx
                .eval("navigator.usb instanceof EventTarget")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn webusb_transfer_result_classes_exist() {
        with_webusb_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof window.USBTransferInResult === 'function' && typeof window.USBTransferOutResult === 'function'"
                )
                .unwrap();
            assert!(ok);
        });
    }
}
