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
  // SerialPort stub — all I/O operations reject (Phase 0)
  class SerialPort extends EventTarget {
    constructor() {
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
    constructor() {
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
    value: new Serial(),
    writable: false,
    enumerable: true
  });

  window.Serial = Serial;
})();
"#;
