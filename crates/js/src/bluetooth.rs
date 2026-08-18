//! Web Bluetooth API stub (W3C Web Bluetooth §3-4)
//! Phase 0: navigator.bluetooth.requestDevice() and all device operations reject (no BLE support)

/// V8 port of the former rquickjs `install_bluetooth_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_bluetooth_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BLUETOOTH_SHIM)?;
    Ok(())
}

/// JavaScript shim: Web Bluetooth stub (Phase 0 - all operations reject with NotSupportedError)
#[cfg(feature = "v8-backend")]
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

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_bluetooth_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
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
        install_bluetooth_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn bluetooth_shim_defined() {
        assert!(!BLUETOOTH_SHIM.is_empty());
    }

    #[test]
    fn bluetooth_navigator_bluetooth_exists() {
        with_bluetooth_api(|rt| {
            let ok = rt.eval("typeof navigator.bluetooth === 'object'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn bluetooth_get_availability_is_async() {
        with_bluetooth_api(|rt| {
            let ok = rt
                .eval("navigator.bluetooth.getAvailability() instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn bluetooth_device_class_exists() {
        with_bluetooth_api(|rt| {
            let ok = rt
                .eval("typeof window.BluetoothDevice === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn bluetooth_device_has_properties() {
        with_bluetooth_api(|rt| {
            let ok = rt
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
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn bluetooth_gatt_server_class_exists() {
        with_bluetooth_api(|rt| {
            let ok = rt
                .eval("typeof window.BluetoothRemoteGATTServer === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn bluetooth_request_device_is_async() {
        with_bluetooth_api(|rt| {
            let ok = rt
                .eval("navigator.bluetooth.requestDevice({ filters: [] }) instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
