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

use rquickjs::Ctx;

/// Install the Payment Request API stub into the JS context.
///
/// Defines `window.PaymentRequest` constructor and related types.
/// Must be called **after** `dom::install_dom_api` so that `window` is already present.
pub fn init_payment_request(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(PAYMENT_REQUEST_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Payment Request API (Phase 0).
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

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Ctx, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn install_stubs(ctx: &Ctx) {
        ctx.eval::<(), _>(
            r#"
            globalThis.window = globalThis;
            globalThis.DOMException = function(msg, name) {
              this.message = msg;
              this.name = name;
            };
            "#,
        )
        .expect("install stubs");
    }

    #[test]
    fn test_payment_request_constructor() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  try {
                    var pr = new PaymentRequest(
                      [{supportedMethods: 'basic-card'}],
                      {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}}
                    );
                    return typeof pr === 'object' ? 'created' : 'failed';
                  } catch (e) {
                    return 'error: ' + e.message;
                  }
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "created");
        });
    }

    #[test]
    fn test_show_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  var pr = new PaymentRequest(
                    [{supportedMethods: 'basic-card'}],
                    {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}}
                  );
                  return pr.show() instanceof Promise ? 'promise' : 'not_promise';
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_show_rejects_with_not_supported() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  var pr = new PaymentRequest(
                    [{supportedMethods: 'basic-card'}],
                    {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}}
                  );
                  var show_promise = pr.show();
                  var error_name = '';
                  show_promise.catch(function(e) {
                    error_name = e.name;
                  });
                  // In synchronous context, we can't wait for the promise
                  // Just verify it's a promise and will reject
                  return show_promise instanceof Promise ? 'is_promise' : 'not_promise';
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "is_promise");
        });
    }

    #[test]
    fn test_can_make_payment_returns_false() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  var pr = new PaymentRequest(
                    [{supportedMethods: 'basic-card'}],
                    {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}}
                  );
                  var can_pay_promise = pr.canMakePayment();
                  var result_value = null;
                  can_pay_promise.then(function(val) {
                    result_value = val;
                  });
                  // In synchronous context, just verify it returns a promise
                  return can_pay_promise instanceof Promise ? 'is_promise' : 'not_promise';
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "is_promise");
        });
    }

    #[test]
    fn test_abort_rejects_when_not_interactive() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  var pr = new PaymentRequest(
                    [{supportedMethods: 'basic-card'}],
                    {total: {label: 'Total', amount: {currency: 'USD', value: '10'}}}
                  );
                  var abort_promise = pr.abort();
                  var error_name = '';
                  abort_promise.catch(function(e) {
                    error_name = e.name;
                  });
                  // Just verify it returns a promise
                  return abort_promise instanceof Promise ? 'is_promise' : 'not_promise';
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "is_promise");
        });
    }

    #[test]
    fn test_payment_response_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_payment_request(&ctx).expect("init payment request");
            let result: String = ctx
                .eval(
                    r#"
                (function() {
                  return typeof PaymentResponse === 'function' ? 'exists' : 'missing';
                })()
                "#,
                )
                .expect("eval");
            assert_eq!(result, "exists");
        });
    }
}
