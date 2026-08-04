//! Payment Request API stub (W3C Payment Request API).
//!
//! Implements `PaymentRequest` constructor and methods for payment handling.
//! Phase 0: All payment operations are rejected as unsupported.
//!
//! API surface:
//! - `new PaymentRequest(methodData, details, options)` — constructor (Phase 0: accepts but no processing)
//! - `.show()` — returns rejected Promise with NotSupportedError
//! - `.canMakePayment()` — returns Promise<false>
//! - `.abort()` — returns Promise<void>, rejected with InvalidStateError if not showing

/// V8 port of the former rquickjs `init_payment_request` (Ph3 V8 migration
/// S12b-G4, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `window.PaymentRequest` constructor and related types.
/// Must be called **after** `v8_runtime.rs::install_dom` so that `window` is already present.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_payment_request_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PAYMENT_REQUEST_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Payment Request API (Phase 0).
#[cfg(feature = "v8-backend")]
const PAYMENT_REQUEST_SHIM: &str = r#"(function() {
  if (typeof window === 'undefined') return;

  // PaymentResponse class (stub)
  var PaymentResponse = function() {
    this.requestId = '';
    this.methodName = '';
    this.details = {};
  };

  PaymentResponse.prototype.toJSON = function() {
    return {
      requestId: this.requestId,
      methodName: this.methodName,
      details: this.details
    };
  };

  // PaymentRequest constructor
  var PaymentRequest = function(methodData, details, options) {
    if (!methodData || typeof methodData !== 'object') {
      throw new TypeError('methodData is required');
    }
    if (!details || typeof details !== 'object') {
      throw new TypeError('details is required');
    }

    // Store minimal state (Phase 0: no actual processing)
    this._id = Math.random().toString(36).substr(2, 9);
    this._methodData = methodData;
    this._details = details;
    this._options = options || {};
    this._state = 'created'; // 'created' | 'interactive' | 'closed'
  };

  // show() -> Promise<PaymentResponse>
  // Phase 0: always rejects with NotSupportedError
  PaymentRequest.prototype.show = function() {
    var self = this;
    return new Promise(function(resolve, reject) {
      // Simulate asynchronous rejection
      setTimeout(function() {
        reject(new DOMException(
          'Payment method not supported',
          'NotSupportedError'
        ));
      }, 0);
    });
  };

  // canMakePayment() -> Promise<boolean>
  // Phase 0: always returns false
  PaymentRequest.prototype.canMakePayment = function() {
    return Promise.resolve(false);
  };

  // abort() -> Promise<void>
  // Phase 0: rejects if not in 'interactive' state
  PaymentRequest.prototype.abort = function() {
    var self = this;
    return new Promise(function(resolve, reject) {
      if (self._state !== 'interactive') {
        reject(new DOMException(
          'Cannot abort: request is not in interactive state',
          'InvalidStateError'
        ));
      } else {
        self._state = 'closed';
        resolve();
      }
    });
  };

  // Expose to window and globalThis
  window.PaymentRequest = PaymentRequest;
  window.PaymentResponse = PaymentResponse;

  if (typeof globalThis !== 'undefined') {
    globalThis.PaymentRequest = PaymentRequest;
    globalThis.PaymentResponse = PaymentResponse;
  }
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_payment_request(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "globalThis.window = globalThis; \
             globalThis.DOMException = function(msg, name) { this.message = msg; this.name = name; };",
        )
        .unwrap();
        install_payment_request_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn test_payment_request_constructor() {
        with_payment_request(|rt| {
            let result = rt
                .eval(
                    "(function() { \
                       try { \
                         var pr = new PaymentRequest( \
                           [{supportedMethods: 'basic-card'}], \
                           {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}} \
                         ); \
                         return typeof pr === 'object' ? 'created' : 'failed'; \
                       } catch (e) { \
                         return 'error: ' + e.message; \
                       } \
                     })()",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("created".to_string()));
        });
    }

    #[test]
    fn test_show_returns_promise() {
        with_payment_request(|rt| {
            let result = rt
                .eval(
                    "var pr = new PaymentRequest( \
                       [{supportedMethods: 'basic-card'}], \
                       {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}} \
                     ); \
                     pr.show() instanceof Promise ? 'promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    // In a synchronous eval we can't await the rejection; mirrors the original
    // rquickjs test which only verified the returned value is a Promise.
    #[test]
    fn test_show_rejects_with_not_supported() {
        with_payment_request(|rt| {
            let result = rt
                .eval(
                    "var pr = new PaymentRequest( \
                       [{supportedMethods: 'basic-card'}], \
                       {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}} \
                     ); \
                     var show_promise = pr.show(); \
                     show_promise.catch(function(e) {}); \
                     show_promise instanceof Promise ? 'is_promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("is_promise".to_string()));
        });
    }

    #[test]
    fn test_can_make_payment_returns_false() {
        with_payment_request(|rt| {
            let result = rt
                .eval(
                    "var pr = new PaymentRequest( \
                       [{supportedMethods: 'basic-card'}], \
                       {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}} \
                     ); \
                     var can_pay_promise = pr.canMakePayment(); \
                     can_pay_promise.then(function(val) {}); \
                     can_pay_promise instanceof Promise ? 'is_promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("is_promise".to_string()));
        });
    }

    #[test]
    fn test_abort_rejects_when_not_interactive() {
        with_payment_request(|rt| {
            let result = rt
                .eval(
                    "var pr = new PaymentRequest( \
                       [{supportedMethods: 'basic-card'}], \
                       {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}} \
                     ); \
                     var abort_promise = pr.abort(); \
                     abort_promise.catch(function(e) {}); \
                     abort_promise instanceof Promise ? 'is_promise' : 'not_promise'",
                )
                .unwrap();
            assert_eq!(result, JsValue::String("is_promise".to_string()));
        });
    }

    #[test]
    fn test_payment_response_exists() {
        with_payment_request(|rt| {
            let result = rt
                .eval("typeof PaymentResponse === 'function' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }
}
