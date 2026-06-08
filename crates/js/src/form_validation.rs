/// HTML Form Constraint Validation API (WHATWG HTML §4.10.21)
/// ValidityState with all 11 flags, element.checkValidity/reportValidity/setCustomValidity,
/// element.validity/validationMessage/willValidate, form.checkValidity/reportValidity.
/// Phase 0: full ValidityState infrastructure; validation checks: valueMissing, customError,
/// typeMismatch (email/url), patternMismatch, tooLong, tooShort, rangeUnderflow, rangeOverflow.
use rquickjs::Ctx;

/// Install Form Constraint Validation API bindings into the JS context.
pub fn install_form_validation_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(FORM_VALIDATION_SHIM)?;
    Ok(())
}

const FORM_VALIDATION_SHIM: &str = r#"
(function() {
  'use strict';

  // ValidityState — read-only snapshot of constraint validation state (§4.10.21.1)
  class ValidityState {
    constructor(flags) {
      this.valueMissing    = !!flags.valueMissing;
      this.typeMismatch    = !!flags.typeMismatch;
      this.patternMismatch = !!flags.patternMismatch;
      this.tooLong         = !!flags.tooLong;
      this.tooShort        = !!flags.tooShort;
      this.rangeUnderflow  = !!flags.rangeUnderflow;
      this.rangeOverflow   = !!flags.rangeOverflow;
      this.stepMismatch    = !!flags.stepMismatch;
      this.badInput        = !!flags.badInput;
      this.customError     = !!flags.customError;
      this.valid = !this.valueMissing && !this.typeMismatch && !this.patternMismatch &&
                   !this.tooLong && !this.tooShort && !this.rangeUnderflow &&
                   !this.rangeOverflow && !this.stepMismatch && !this.badInput &&
                   !this.customError;
    }
  }
  window.ValidityState = ValidityState;

  // Compute ValidityState flags for an element.
  function computeValidity(el) {
    const flags = {};
    const type = (el.type || '').toLowerCase();
    const value = el.value || '';

    // valueMissing: required + empty (§4.10.18.5.1)
    if (el.required) {
      const empty = (type === 'checkbox' || type === 'radio')
        ? !el.checked
        : value.trim() === '';
      flags.valueMissing = empty;
    }

    // typeMismatch: built-in type format validation (§4.10.18.5.2)
    if (value !== '') {
      if (type === 'email') {
        flags.typeMismatch = !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value);
      } else if (type === 'url') {
        try { new URL(value); flags.typeMismatch = false; }
        catch (_) { flags.typeMismatch = true; }
      }
    }

    // patternMismatch: input.pattern attribute (§4.10.18.5.3)
    const pattern = el.getAttribute ? el.getAttribute('pattern') : null;
    if (pattern && value !== '') {
      try {
        flags.patternMismatch = !new RegExp('^(?:' + pattern + ')$').test(value);
      } catch (_) {}
    }

    // tooLong / tooShort (§4.10.18.5.4)
    const maxLen = el.maxLength >= 0 ? el.maxLength : -1;
    const minLen = el.minLength >= 0 ? el.minLength : -1;
    if (maxLen >= 0 && value.length > maxLen) flags.tooLong = true;
    if (minLen > 0 && value.length > 0 && value.length < minLen) flags.tooShort = true;

    // rangeUnderflow / rangeOverflow (§4.10.18.5.5)
    if (type === 'number' || type === 'range') {
      const num = parseFloat(value);
      if (!isNaN(num)) {
        const min = el.getAttribute ? el.getAttribute('min') : null;
        const max = el.getAttribute ? el.getAttribute('max') : null;
        if (min !== null && !isNaN(parseFloat(min)) && num < parseFloat(min)) flags.rangeUnderflow = true;
        if (max !== null && !isNaN(parseFloat(max)) && num > parseFloat(max)) flags.rangeOverflow = true;
      } else if (value !== '') {
        flags.badInput = true;
      }
    }

    // customError: set via setCustomValidity (§4.10.21.3)
    flags.customError = !!(el._customValidationMessage && el._customValidationMessage !== '');

    return new ValidityState(flags);
  }

  // Determine if an element is a submittable form-associated element (§4.10.18).
  function isSubmittable(el) {
    const tag = el.tagName ? el.tagName.toUpperCase() : '';
    if (tag !== 'INPUT' && tag !== 'TEXTAREA' && tag !== 'SELECT' && tag !== 'BUTTON') return false;
    if (el.disabled) return false;
    if (tag === 'INPUT' && (el.type || '').toLowerCase() === 'hidden') return false;
    return true;
  }

  // Mixin: add constraint validation API to a form-associated element prototype.
  function applyConstraintValidationMixin(proto) {
    // willValidate: true when the element is a candidate for constraint validation (§4.10.21.5)
    Object.defineProperty(proto, 'willValidate', {
      get() { return isSubmittable(this); },
      configurable: true,
    });

    // validity: return a ValidityState snapshot
    Object.defineProperty(proto, 'validity', {
      get() { return computeValidity(this); },
      configurable: true,
    });

    // validationMessage: human-readable message (§4.10.21.5)
    Object.defineProperty(proto, 'validationMessage', {
      get() {
        if (!this.willValidate) return '';
        const msg = this._customValidationMessage;
        if (msg) return msg;
        const v = computeValidity(this);
        if (v.valueMissing) return 'Please fill in this field.';
        if (v.typeMismatch) return 'Please enter a valid value.';
        if (v.patternMismatch) return 'Please match the requested format.';
        if (v.tooLong) return 'Please shorten this text.';
        if (v.tooShort) return 'Please lengthen this text.';
        if (v.rangeUnderflow) return 'Value must be greater than or equal to the minimum.';
        if (v.rangeOverflow) return 'Value must be less than or equal to the maximum.';
        if (v.badInput) return 'Please enter a number.';
        return '';
      },
      configurable: true,
    });

    // setCustomValidity: set or clear the custom validation message (§4.10.21.3)
    proto.setCustomValidity = function setCustomValidity(message) {
      this._customValidationMessage = message;
    };

    // checkValidity: fire 'invalid' event if not valid; return validity (§4.10.21.2)
    proto.checkValidity = function checkValidity() {
      if (!this.willValidate) return true;
      if (this.validity.valid) return true;
      // Dispatch non-bubbling, non-cancelable 'invalid' event
      const ev = new Event('invalid', { bubbles: false, cancelable: true });
      this.dispatchEvent(ev);
      return false;
    };

    // reportValidity: same as checkValidity but also shows browser validation UI (§4.10.21.2)
    // Phase 0: no UI shown; identical to checkValidity.
    proto.reportValidity = function reportValidity() {
      return this.checkValidity();
    };
  }

  // Apply mixin to all submittable element prototypes.
  // BUG-072: in the real install_dom environment these constructors are NOT
  // defined as globals, so a bare `HTMLInputElement` reference throws
  // ReferenceError and aborts the whole shim before it can install. Guard each
  // with `typeof` so a missing constructor yields a null proto (skipped below).
  const inputProto    = typeof HTMLInputElement    !== 'undefined' ? HTMLInputElement.prototype    : null;
  const textareaProto = typeof HTMLTextAreaElement !== 'undefined' ? HTMLTextAreaElement.prototype : null;
  const selectProto   = typeof HTMLSelectElement   !== 'undefined' ? HTMLSelectElement.prototype   : null;
  const buttonProto   = typeof HTMLButtonElement   !== 'undefined' ? HTMLButtonElement.prototype   : null;

  for (const proto of [inputProto, textareaProto, selectProto, buttonProto]) {
    if (proto) applyConstraintValidationMixin(proto);
  }

  // HTMLFormElement: checkValidity / reportValidity iterate all form controls (§4.10.22.3)
  if (typeof HTMLFormElement !== 'undefined') {
    HTMLFormElement.prototype.checkValidity = function checkValidity() {
      let allValid = true;
      const controls = this.elements || [];
      for (let i = 0; i < controls.length; i++) {
        const el = controls[i];
        if (typeof el.checkValidity === 'function' && !el.checkValidity()) {
          allValid = false;
        }
      }
      return allValid;
    };

    HTMLFormElement.prototype.reportValidity = function reportValidity() {
      return this.checkValidity();
    };
  }
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

    /// Set up a minimal DOM environment + form validation API for testing.
    fn with_form_validation_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;

                // Minimal Event stub
                function Event(type, init) {
                  this.type = type;
                  this.bubbles = !!(init && init.bubbles);
                  this.cancelable = !!(init && init.cancelable);
                  this._defaultPrevented = false;
                }
                Event.prototype.preventDefault = function() { this._defaultPrevented = true; };
                window.Event = Event;

                // DOMException stub
                function DOMException(msg, name) {
                  this.message = msg;
                  this.name = name || 'Error';
                }
                window.DOMException = DOMException;

                // Minimal element factory
                function makeElement(tag, attrs) {
                  var el = {
                    tagName: tag.toUpperCase(),
                    type: '',
                    value: '',
                    required: false,
                    disabled: false,
                    checked: false,
                    maxLength: -1,
                    minLength: -1,
                    _customValidationMessage: '',
                    _listeners: {},
                    getAttribute: function(name) { return attrs && attrs[name] !== undefined ? attrs[name] : null; },
                    dispatchEvent: function(ev) {
                      var handlers = this._listeners[ev.type] || [];
                      handlers.forEach(function(h) { h(ev); });
                      return !ev._defaultPrevented;
                    },
                    addEventListener: function(type, fn) {
                      if (!this._listeners[type]) this._listeners[type] = [];
                      this._listeners[type].push(fn);
                    },
                  };
                  return el;
                }

                // Element prototypes — the mixin targets
                function HTMLInputElement() {}
                function HTMLTextAreaElement() {}
                function HTMLSelectElement() {}
                function HTMLButtonElement() {}
                function HTMLFormElement() {}
                window.HTMLInputElement    = HTMLInputElement;
                window.HTMLTextAreaElement = HTMLTextAreaElement;
                window.HTMLSelectElement   = HTMLSelectElement;
                window.HTMLButtonElement   = HTMLButtonElement;
                window.HTMLFormElement     = HTMLFormElement;

                // Assign prototype to element instances
                window.makeInput = function(attrs) {
                  var el = makeElement('INPUT', attrs || {});
                  Object.setPrototypeOf(el, HTMLInputElement.prototype);
                  return el;
                };
                window.makeTextarea = function() {
                  var el = makeElement('TEXTAREA', {});
                  Object.setPrototypeOf(el, HTMLTextAreaElement.prototype);
                  return el;
                };
                window.makeForm = function() {
                  var el = {
                    tagName: 'FORM',
                    elements: [],
                  };
                  Object.setPrototypeOf(el, HTMLFormElement.prototype);
                  return el;
                };
                "#,
            )
            .unwrap();
            install_form_validation_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    /// BUG-072: the real `install_dom` environment does NOT define
    /// `HTMLInputElement`/`HTMLTextAreaElement`/`HTMLSelectElement`/`HTMLButtonElement`
    /// as global constructors. Before the fix, the shim referenced them by bare
    /// name and threw `ReferenceError: HTMLInputElement is not defined`, aborting
    /// the whole install. The `typeof` guards must let it install cleanly.
    #[test]
    fn installs_without_form_element_constructors() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal environment: window only, no HTML*Element constructors —
            // mirrors the real install_dom globals.
            ctx.eval::<(), _>("var window = globalThis;").unwrap();
            install_form_validation_bindings(&ctx)
                .expect("shim must install without throwing when element constructors are absent");
            // ValidityState is still exported (the part that survives regardless),
            // and HTMLFormElement.prototype methods are guarded by typeof.
            let ok: bool = ctx
                .eval("typeof window.ValidityState === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn validity_state_class_exists() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.ValidityState === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn validity_state_valid_when_no_flags() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var vs = new ValidityState({});
                    vs.valid === true &&
                    vs.valueMissing === false &&
                    vs.typeMismatch === false &&
                    vs.customError === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn check_validity_fires_invalid_event_when_required_empty() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeInput();
                    el.required = true;
                    el.value = '';
                    var fired = false;
                    el.addEventListener('invalid', function() { fired = true; });
                    var result = el.checkValidity();
                    result === false && fired === true
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_custom_validity_sets_custom_error_flag() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeInput();
                    el.setCustomValidity('bad value');
                    el.validity.customError === true && el.validity.valid === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_custom_validity_empty_clears_error() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeInput();
                    el.setCustomValidity('error');
                    el.setCustomValidity('');
                    el.validity.customError === false && el.validity.valid === true
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn form_check_validity_iterates_controls() {
        with_form_validation_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var form = makeForm();
                    var el1 = makeInput();
                    el1.required = true;
                    el1.value = '';
                    var el2 = makeTextarea();
                    el2.value = 'ok';
                    form.elements = [el1, el2];
                    var result = form.checkValidity();
                    result === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
