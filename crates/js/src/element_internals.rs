/// ElementInternals + CustomStateSet (WHATWG HTML §4.13.2)
/// Phase 0: JS-shim without real a11y integration.
/// `element.attachInternals()` returns an ElementInternals with a CustomStateSet,
/// validity API (setValidity/checkValidity/reportValidity), and ARIA reflection.
/// Native binding `_lumen_element_internals_get_states(nid)` exposes states to Rust.
/// `:state()` CSS selector handoff → P4 (css-parser).
use rquickjs::Ctx;

/// Install ElementInternals and CustomStateSet bindings into the JS context.
pub fn install_element_internals_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(ELEMENT_INTERNALS_SHIM)?;
    Ok(())
}

const ELEMENT_INTERNALS_SHIM: &str = r#"
(function() {
  'use strict';

  // CustomStateSet — set-like collection of custom element states (§4.13.2)
  // Implements iterable Set-like interface: add/has/delete/clear/values/forEach.
  class CustomStateSet {
    constructor() {
      this._states = new Set();
    }

    add(state) {
      if (typeof state !== 'string') throw new TypeError('State must be a string');
      this._states.add(state);
      return this;
    }

    has(state) {
      return this._states.has(state);
    }

    delete(state) {
      return this._states.delete(state);
    }

    clear() {
      this._states.clear();
    }

    get size() {
      return this._states.size;
    }

    values() {
      return this._states.values();
    }

    forEach(callback, thisArg) {
      this._states.forEach(callback, thisArg);
    }

    [Symbol.iterator]() {
      return this._states[Symbol.iterator]();
    }
  }
  window.CustomStateSet = CustomStateSet;

  // ElementInternals — internals object attached to custom elements (§4.13.2)
  class ElementInternals {
    constructor(element) {
      this._element = element;
      this._states = new CustomStateSet();
      // validity state (Phase 0: always valid until setValidity called)
      this._validityFlags = {};
      this._validationMessage = '';
      this._validityAnchor = null;
      // ARIA reflection (Phase 0: in-memory only, no DOM attribute sync)
      this.role = null;
      this.ariaLabel = null;
      this.ariaDisabled = null;
      this.ariaExpanded = null;
      this.ariaHidden = null;
      this.ariaChecked = null;
      this.ariaRequired = null;
      this.ariaSelected = null;
      this.ariaValueNow = null;
      this.ariaValueMin = null;
      this.ariaValueMax = null;
      this.ariaValueText = null;
    }

    get states() {
      return this._states;
    }

    // validity: read-only snapshot derived from _validityFlags (§4.13.2)
    get validity() {
      const f = this._validityFlags;
      const anyError = !!(f.valueMissing || f.typeMismatch || f.patternMismatch ||
        f.tooLong || f.tooShort || f.rangeUnderflow || f.rangeOverflow ||
        f.stepMismatch || f.badInput || f.customError);
      return {
        valueMissing:    !!f.valueMissing,
        typeMismatch:    !!f.typeMismatch,
        patternMismatch: !!f.patternMismatch,
        tooLong:         !!f.tooLong,
        tooShort:        !!f.tooShort,
        rangeUnderflow:  !!f.rangeUnderflow,
        rangeOverflow:   !!f.rangeOverflow,
        stepMismatch:    !!f.stepMismatch,
        badInput:        !!f.badInput,
        customError:     !!f.customError,
        valid:           !anyError,
      };
    }

    get validationMessage() {
      return this._validationMessage;
    }

    // setValidity: mark constraint validation state (§4.13.2)
    setValidity(flags, message, anchor) {
      if (flags === undefined) throw new TypeError('flags required');
      // Clearing all flags: setValidity({}) resets to valid
      this._validityFlags = {};
      if (flags && typeof flags === 'object') {
        for (const key of Object.keys(flags)) {
          if (flags[key]) this._validityFlags[key] = true;
        }
      }
      const anyError = Object.keys(this._validityFlags).some(k => this._validityFlags[k]);
      this._validationMessage = anyError ? (message || 'Invalid') : '';
      this._validityAnchor = anchor || null;
    }

    // checkValidity: fire 'invalid' event if element is invalid (§4.13.2)
    checkValidity() {
      if (this.validity.valid) return true;
      if (this._element && typeof this._element.dispatchEvent === 'function') {
        const ev = new Event('invalid', { bubbles: false, cancelable: true });
        this._element.dispatchEvent(ev);
      }
      return false;
    }

    // reportValidity: Phase 0 — same as checkValidity (no UI shown)
    reportValidity() {
      return this.checkValidity();
    }

    get form() {
      // Phase 0: returns null (form association via formAssociated not yet wired)
      return null;
    }

    get labels() {
      // Phase 0: empty NodeList
      return [];
    }

    get willValidate() {
      return true;
    }
  }
  window.ElementInternals = ElementInternals;

  // element.attachInternals(): returns cached ElementInternals per element (§4.13.2)
  // Phase 0: attaches to any element; Phase 1: restrict to custom elements only.
  if (typeof Element !== 'undefined') {
    Element.prototype.attachInternals = function attachInternals() {
      if (!this._elementInternals) {
        this._elementInternals = new ElementInternals(this);
      }
      return this._elementInternals;
    };
  }

  // Native binding: returns JSON array of active states for a given node id.
  // Shell/Rust calls this to check :state() matches during style computation.
  // CSS: :state() pseudo-class selector — P4 handoff (css-parser).
  globalThis._lumen_element_internals_get_states = function _lumen_element_internals_get_states(nid) {
    // Walk registered internals by nid — Phase 0: linear scan via __nid property.
    // Phase 1: replace with a WeakMap keyed on element objects.
    const el = typeof _lumen_get_element_by_nid === 'function'
      ? _lumen_get_element_by_nid(nid)
      : null;
    if (!el || !el._elementInternals) return '[]';
    return JSON.stringify([...el._elementInternals.states]);
  };
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

    /// Set up minimal DOM stubs + ElementInternals bindings.
    fn with_element_internals_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;

                function Event(type, init) {
                  this.type = type;
                  this.bubbles = !!(init && init.bubbles);
                  this.cancelable = !!(init && init.cancelable);
                  this._defaultPrevented = false;
                }
                Event.prototype.preventDefault = function() { this._defaultPrevented = true; };
                window.Event = Event;

                function Element() {}
                Element.prototype.dispatchEvent = function(ev) { return true; };
                window.Element = Element;

                // Factory: element with Element prototype
                window.makeEl = function() {
                  var el = Object.create(Element.prototype);
                  el._listeners = {};
                  el.dispatchEvent = function(ev) {
                    var hs = this._listeners[ev.type] || [];
                    hs.forEach(function(h) { h(ev); });
                    return !ev._defaultPrevented;
                  };
                  el.addEventListener = function(type, fn) {
                    if (!this._listeners[type]) this._listeners[type] = [];
                    this._listeners[type].push(fn);
                  };
                  return el;
                };
                "#,
            )
            .unwrap();
            install_element_internals_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn element_internals_class_exists() {
        with_element_internals_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.ElementInternals === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn custom_state_set_add_has_delete_clear() {
        with_element_internals_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeEl();
                    var internals = el.attachInternals();
                    internals.states.add('checked');
                    internals.states.add('loading');
                    var h1 = internals.states.has('checked');
                    var h2 = internals.states.has('loading');
                    var s1 = internals.states.size;
                    internals.states.delete('checked');
                    var h3 = internals.states.has('checked');
                    internals.states.clear();
                    var s2 = internals.states.size;
                    h1 && h2 && s1 === 2 && !h3 && s2 === 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_validity_marks_invalid_and_check_validity_fires_event() {
        with_element_internals_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeEl();
                    var internals = el.attachInternals();
                    var fired = false;
                    el.addEventListener('invalid', function() { fired = true; });
                    internals.setValidity({ valueMissing: true }, 'Required');
                    var isInvalid = !internals.validity.valid;
                    var result = internals.checkValidity();
                    isInvalid && !result && fired
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn set_validity_empty_resets_to_valid() {
        with_element_internals_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeEl();
                    var internals = el.attachInternals();
                    internals.setValidity({ customError: true }, 'oops');
                    internals.setValidity({});
                    internals.validity.valid && internals.validationMessage === ''
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn attach_internals_returns_same_instance() {
        with_element_internals_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var el = makeEl();
                    var a = el.attachInternals();
                    var b = el.attachInternals();
                    a === b
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
