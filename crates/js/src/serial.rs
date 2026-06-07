/// WebSerial API stub (W3C Serial API L1)
/// Phase 0: navigator.serial.requestPort() → reject NotSupportedError,
/// getPorts() → Promise<[]>, SerialPort operations reject.
use rquickjs::Ctx;

/// Install WebSerial API bindings into the JS context.
pub fn install_serial_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SERIAL_SHIM)?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn with_serial_api(f: impl FnOnce(&rquickjs::Ctx)) {
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
                EventTarget.prototype.dispatchEvent = function() {};
                globalThis.EventTarget = EventTarget;
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
            install_serial_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn serial_navigator_serial_exists() {
        with_serial_api(|ctx| {
            let ok: bool = ctx.eval("typeof navigator.serial === 'object'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn serial_get_ports_returns_promise() {
        with_serial_api(|ctx| {
            let ok: bool = ctx
                .eval("navigator.serial.getPorts() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn serial_request_port_returns_promise() {
        with_serial_api(|ctx| {
            let ok: bool = ctx
                .eval("navigator.serial.requestPort({filters:[]}) instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn serial_port_class_exists() {
        with_serial_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.SerialPort === 'function'")
                .unwrap();
            assert!(ok);
        });
    }
}
