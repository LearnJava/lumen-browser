//! WebSerial API stub (W3C Serial API L1)
//! Phase 0: navigator.serial.requestPort() → reject NotSupportedError,
//! getPorts() → Promise<[]>, SerialPort operations reject.

/// V8 port of the former rquickjs `install_serial_bindings` (Ph3 V8 migration S5-S7):
/// identical JS shim, evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of
/// `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_serial_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SERIAL_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SERIAL_SHIM: &str = r#"
(function() {
  // WebIDL: neither `Serial` nor `SerialPort` declares a constructor
  // operation — both are handed out only by the engine (`navigator.serial`,
  // `requestPort()`/`getPorts()`), so `new X()` from a page is an illegal
  // constructor (BUG-672). The engine passes this private key instead.
  var ENGINE_KEY = {};

  // SerialPort stub — all I/O operations reject (Phase 0)
  class SerialPort extends EventTarget {
    constructor(key = undefined) {
      if (key !== ENGINE_KEY) throw new TypeError('Illegal constructor');
      super();
      this.readable = null;
      this.writable = null;
      this.onconnect = null;
      this.ondisconnect = null;
    }

    async open(options) {
      throw new DOMException('WebSerial not supported (Phase 0)', 'NotSupportedError');
    }

    async close() {
      throw new DOMException('WebSerial not supported (Phase 0)', 'NotSupportedError');
    }

    getInfo() {
      return { usbVendorId: undefined, usbProductId: undefined };
    }
  }
  window.SerialPort = SerialPort;

  // Serial (navigator.serial)
  class Serial extends EventTarget {
    constructor(key = undefined) {
      if (key !== ENGINE_KEY) throw new TypeError('Illegal constructor');
      super();
      this.onconnect = null;
      this.ondisconnect = null;
    }

    async requestPort(options) {
      throw new DOMException('WebSerial not supported (Phase 0)', 'NotSupportedError');
    }

    async getPorts() {
      return [];
    }
  }

  Object.defineProperty(navigator, 'serial', {
    value: new Serial(ENGINE_KEY),
    writable: false,
    enumerable: true,
    configurable: true
  });

  window.Serial = Serial;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn with_serial(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, None, false, None)
            .unwrap();
        f(&rt);
    }

    fn ctor_result(rt: &V8JsRuntime, name: &str) -> JsValue {
        rt.eval(&format!(
            "(function() {{ try {{ new {name}(); return 'constructed'; }} \
             catch (e) {{ return e instanceof TypeError ? 'TypeError' : String(e); }} }})()"
        ))
        .unwrap()
    }

    /// BUG-672: `new Serial()`/`new SerialPort()` from a page must throw
    /// `TypeError` — WebIDL defines no constructor for either interface.
    #[test]
    fn serial_interfaces_are_not_constructible() {
        with_serial(|rt| {
            assert_eq!(ctor_result(rt, "Serial"), JsValue::String("TypeError".into()));
            assert_eq!(ctor_result(rt, "SerialPort"), JsValue::String("TypeError".into()));
        });
    }

    /// The engine-created `navigator.serial` singleton still exists and is a
    /// genuine `Serial : EventTarget`.
    #[test]
    fn navigator_serial_is_engine_instance() {
        with_serial(|rt| {
            let ok = rt
                .eval(
                    "navigator.serial instanceof Serial && navigator.serial instanceof EventTarget \
                     && typeof navigator.serial.getPorts === 'function'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
