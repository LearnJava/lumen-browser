/// Web Bluetooth API stub (W3C Web Bluetooth §3-4)
/// Phase 0: navigator.bluetooth.requestDevice() and all device operations reject (no BLE support)
use rquickjs::Ctx;

pub fn install_bluetooth_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(BLUETOOTH_SHIM)?;
    Ok(())
}

/// JavaScript shim: Web Bluetooth stub (Phase 0 - all operations reject with NotSupportedError)
const BLUETOOTH_SHIM: &str = r#"
(function() {
  // BluetoothRemoteGATTServer class
  class BluetoothRemoteGATTServer {
    constructor(device) {
      this.device = device;
      this.connected = false;
    }

    async connect() {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }

    async disconnect() {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }

    async getPrimaryService(serviceUUID) {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }

    async getPrimaryServices(serviceUUID) {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }
  }
  window.BluetoothRemoteGATTServer = BluetoothRemoteGATTServer;

  // BluetoothDevice class
  class BluetoothDevice extends EventTarget {
    constructor(id, name, uuids = []) {
      super();
      this.id = id;
      this.name = name;
      this.uuids = uuids;
      this.gatt = new BluetoothRemoteGATTServer(this);
    }

    async watchAdvertisements() {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }

    unwatchAdvertisements() {
      // Phase 0: no-op
    }

    async forget() {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }
  }
  window.BluetoothDevice = BluetoothDevice;

  // BluetoothManager (navigator.bluetooth)
  class BluetoothManager {
    async requestDevice(options) {
      throw new DOMException('Web Bluetooth not supported (Phase 0)', 'NotSupportedError');
    }

    async getAvailability() {
      return false;
    }

    addEventListener(type, listener, options) {
      // Phase 0: availability change events not supported
    }

    removeEventListener(type, listener, options) {
      // Phase 0: availability change events not supported
    }
  }

  // Install navigator.bluetooth
  Object.defineProperty(navigator, 'bluetooth', {
    value: new BluetoothManager(),
    writable: false,
    configurable: true
  });
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

    fn with_bluetooth_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
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
            super::install_bluetooth_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn bluetooth_shim_defined() {
        assert!(!BLUETOOTH_SHIM.is_empty());
    }

    #[test]
    fn bluetooth_navigator_bluetooth_exists() {
        with_bluetooth_api(|ctx| {
            let ok: bool = ctx.eval("typeof navigator.bluetooth === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn bluetooth_get_availability_is_async() {
        with_bluetooth_api(|ctx| {
            let result: bool = ctx
                .eval("navigator.bluetooth.getAvailability() instanceof Promise")
                .unwrap();
            assert!(result);
        });
    }

    #[test]
    fn bluetooth_device_class_exists() {
        with_bluetooth_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.BluetoothDevice === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn bluetooth_device_has_properties() {
        with_bluetooth_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    const device = new BluetoothDevice('id123', 'Test Device', ['180a']);
                    device.id === 'id123' &&
                    device.name === 'Test Device' &&
                    Array.isArray(device.uuids) &&
                    device.gatt instanceof BluetoothRemoteGATTServer
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn bluetooth_gatt_server_class_exists() {
        with_bluetooth_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.BluetoothRemoteGATTServer === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn bluetooth_request_device_is_async() {
        with_bluetooth_api(|ctx| {
            let result: bool = ctx
                .eval("navigator.bluetooth.requestDevice({ filters: [] }) instanceof Promise")
                .unwrap();
            assert!(result);
        });
    }
}
