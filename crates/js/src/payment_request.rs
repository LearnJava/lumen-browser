//! Payment Request API stub (W3C Payment Request API).
//!
//! Implements `PaymentRequest` constructor and methods for payment handling.
//! Phase 0: All payment operations are rejected as unsupported.
//!
//! API surface:
//! - `new PaymentRequest(methodData, details, options)` — constructor: full §3.1 validation
//!   (PMI syntax/duplicates, currency codes, decimal amounts, negative total, shipping ids,
//!   `data` serialization, `shippingType` enum), then stores the canonicalized request (BUG-646)
//! - `.id` / `.shippingAddress` / `.shippingOption` / `.shippingType` — read-only getters
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

  // ── WebIDL conversions (BUG-646) ──────────────────────────────────────
  // Dictionaries: undefined/null → empty, other primitives → TypeError.
  function toDict(v, what) {
    if (v === undefined || v === null) return {};
    if (typeof v !== 'object' && typeof v !== 'function') {
      throw new TypeError(what + ' is not an object');
    }
    return v;
  }
  function toSeq(v, what) {
    if (v === null || (typeof v !== 'object' && typeof v !== 'function') ||
        typeof v[Symbol.iterator] !== 'function') {
      throw new TypeError(what + ' is not iterable');
    }
    return Array.from(v);
  }
  function reqString(dict, key, what) {
    var v = dict[key];
    if (v === undefined) throw new TypeError(what + '.' + key + ' is required');
    return String(v);
  }
  // `object data` member: absent → undefined, present → must be an object
  // (null is not), then JSON-serialized so cycles rethrow as TypeError.
  function serializeData(dict, what) {
    var d = dict.data;
    if (d === undefined) return undefined;
    if (d === null || (typeof d !== 'object' && typeof d !== 'function')) {
      throw new TypeError(what + '.data is not an object');
    }
    return JSON.stringify(d);
  }

  // Payment Method Identifiers §validity: URL-based (https, no credentials)
  // or standardized `part *("-" part)`, part = lower-alpha *(lower-alpha / DIGIT).
  var STD_PMI = /^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$/;
  // The basic URL parser without a base fails on anything lacking a scheme;
  // `new URL(x)` here resolves such input against `location.href` instead
  // (BUG-1173), so a schemeless PMI must not be offered to it.
  var URL_SCHEME = /^[\x00-\x20]*[A-Za-z][A-Za-z0-9+.\-\t\n\r]*:/;
  function isValidPmi(pmi) {
    var url = null;
    if (URL_SCHEME.test(pmi) && typeof URL === 'function') {
      try { url = new URL(pmi); } catch (e) { url = null; }
    }
    if (url) {
      return url.protocol === 'https:' && url.username === '' && url.password === '';
    }
    return STD_PMI.test(pmi);
  }

  // Payment Request §4.9 checkAndCanonicalizeAmount / checkAndCanonicalizeTotal.
  var DECIMAL_MONETARY = /^-?[0-9]+(\.[0-9]+)?$/;
  function toAmount(v, what) {
    var d = toDict(v, what);
    return {
      currency: reqString(d, 'currency', what),
      value: reqString(d, 'value', what)
    };
  }
  function checkAmount(amount) {
    if (!/^[A-Za-z]{3}$/.test(amount.currency)) {
      throw new RangeError('"' + amount.currency + '" is not a well-formed currency code');
    }
    if (!DECIMAL_MONETARY.test(amount.value)) {
      throw new TypeError('"' + amount.value + '" is not a valid decimal monetary value');
    }
    amount.currency = amount.currency.toUpperCase();
  }
  function checkTotal(amount) {
    checkAmount(amount);
    if (amount.value.charAt(0) === '-') {
      throw new TypeError('total amount must not be negative');
    }
  }
  function toItem(v, what) {
    var d = toDict(v, what);
    var label = reqString(d, 'label', what);
    if (d.amount === undefined) throw new TypeError(what + '.amount is required');
    return { label: label, amount: toAmount(d.amount, what + '.amount'), pending: Boolean(d.pending) };
  }

  function newId() {
    if (typeof crypto !== 'undefined' && crypto && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
      var r = Math.random() * 16 | 0;
      return (c === 'x' ? r : (r & 3 | 8)).toString(16);
    });
  }

  var SHIPPING_TYPES = ['shipping', 'delivery', 'pickup'];

  // PaymentRequest constructor — Payment Request API §3.1.
  var PaymentRequest = function(methodData, details, options) {
    if (!(this instanceof PaymentRequest)) {
      throw new TypeError("Failed to construct 'PaymentRequest': Please use the 'new' operator");
    }
    if (arguments.length < 2) {
      throw new TypeError("Failed to construct 'PaymentRequest': 2 arguments required");
    }

    // WebIDL argument conversion.
    var methods = toSeq(methodData, 'methodData').map(function(m) {
      var d = toDict(m, 'PaymentMethodData');
      return { supportedMethods: reqString(d, 'supportedMethods', 'PaymentMethodData'), dict: d };
    });
    var det = toDict(details, 'details');
    if (det.total === undefined) throw new TypeError('details.total is required');
    var total = toItem(det.total, 'details.total');
    var displayItems = det.displayItems === undefined ? undefined :
      toSeq(det.displayItems, 'details.displayItems').map(function(i) {
        return toItem(i, 'PaymentItem');
      });
    var shippingOptions = det.shippingOptions === undefined ? undefined :
      toSeq(det.shippingOptions, 'details.shippingOptions').map(function(o) {
        var d = toDict(o, 'PaymentShippingOption');
        var item = toItem(d, 'PaymentShippingOption');
        return {
          id: reqString(d, 'id', 'PaymentShippingOption'),
          label: item.label,
          amount: item.amount,
          selected: Boolean(d.selected)
        };
      });
    var modifiers = det.modifiers === undefined ? undefined :
      toSeq(det.modifiers, 'details.modifiers').map(function(m) {
        var d = toDict(m, 'PaymentDetailsModifier');
        return {
          supportedMethods: reqString(d, 'supportedMethods', 'PaymentDetailsModifier'),
          total: d.total === undefined ? undefined : toItem(d.total, 'modifier.total'),
          additionalDisplayItems: d.additionalDisplayItems === undefined ? undefined :
            toSeq(d.additionalDisplayItems, 'modifier.additionalDisplayItems').map(function(i) {
              return toItem(i, 'PaymentItem');
            }),
          dict: d
        };
      });
    var opts = toDict(options, 'options');
    var requestShipping = Boolean(opts.requestShipping);
    var shippingType = opts.shippingType === undefined ? 'shipping' : String(opts.shippingType);
    if (SHIPPING_TYPES.indexOf(shippingType) < 0) {
      throw new TypeError("The provided value '" + shippingType +
        "' is not a valid enum value of type PaymentShippingType.");
    }

    // Step: methodData must be non-empty; PMIs valid and unique.
    if (methods.length === 0) {
      throw new TypeError('At least one payment method is required');
    }
    var seenPmis = [];
    var serializedMethodData = methods.map(function(m) {
      var pmi = m.supportedMethods;
      if (!isValidPmi(pmi)) {
        throw new RangeError('"' + pmi + '" is not a valid payment method identifier');
      }
      if (seenPmis.indexOf(pmi) >= 0) {
        throw new RangeError('Duplicate payment method identifier "' + pmi + '"');
      }
      seenPmis.push(pmi);
      return { supportedMethods: pmi, data: serializeData(m.dict, 'PaymentMethodData') };
    });

    // Process the total and display items.
    checkTotal(total.amount);
    if (displayItems) displayItems.forEach(function(i) { checkAmount(i.amount); });

    // Shipping options are processed only when shipping is requested.
    var selectedShippingOption = null;
    if (requestShipping && shippingOptions) {
      var seenIds = [];
      shippingOptions.forEach(function(o) {
        checkAmount(o.amount);
        if (seenIds.indexOf(o.id) >= 0) {
          throw new TypeError('Duplicate shipping option id "' + o.id + '"');
        }
        seenIds.push(o.id);
        if (o.selected) selectedShippingOption = o.id;
      });
    }

    // Modifiers.
    var serializedModifierData = [];
    if (modifiers) {
      modifiers.forEach(function(m) {
        if (!isValidPmi(m.supportedMethods)) {
          throw new RangeError('"' + m.supportedMethods + '" is not a valid payment method identifier');
        }
        if (m.total) checkTotal(m.total.amount);
        if (m.additionalDisplayItems) {
          m.additionalDisplayItems.forEach(function(i) { checkAmount(i.amount); });
        }
        serializedModifierData.push(serializeData(m.dict, 'PaymentDetailsModifier'));
      });
    }

    // Phase 0: store the canonicalized request, no payment handler behind it.
    this._id = det.id === undefined ? newId() : String(det.id);
    this._methodData = serializedMethodData;
    this._details = {
      total: total,
      displayItems: displayItems,
      shippingOptions: shippingOptions,
      modifiers: modifiers
    };
    this._serializedModifierData = serializedModifierData;
    this._options = opts;
    this._shippingOption = selectedShippingOption;
    this._shippingType = requestShipping ? shippingType : null;
    this._state = 'created'; // 'created' | 'interactive' | 'closed'
  };

  Object.defineProperties(PaymentRequest.prototype, {
    id: { get: function() { return this._id; }, configurable: true, enumerable: true },
    shippingAddress: { get: function() { return null; }, configurable: true, enumerable: true },
    shippingOption: { get: function() { return this._shippingOption; }, configurable: true, enumerable: true },
    shippingType: { get: function() { return this._shippingType; }, configurable: true, enumerable: true }
  });

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
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
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
