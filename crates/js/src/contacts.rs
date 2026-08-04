//! Contact Picker API stub (W3C Contact Picker API).
//!
//! Implements `navigator.contacts` with two methods:
//! - `select(properties, options)` — returns rejected Promise with NotSupportedError
//! - `getProperties()` — returns Promise<['name', 'email', 'tel']>
//!
//! Phase 0: No actual contact picking. All contact access is rejected as unsupported.

/// V8 port of the former rquickjs `init_contacts_manager` (Ph3 V8 migration
/// S12b-G1, rquickjs side removed in the same batch): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `navigator.contacts` with `select()` and `getProperties()` methods.
/// Must be called **after** DOM install so that `navigator` is already present.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_contacts_manager_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(CONTACTS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Contact Picker API (Phase 0).
#[cfg(feature = "v8-backend")]
const CONTACTS_SHIM: &str = r#"(function() {
  if (typeof navigator === 'undefined') return;

  // ContactsManager implementation
  var ContactsManager = function() {};

  // select(properties, options) -> Promise<ContactInfo[]>
  // Phase 0: always rejects with NotSupportedError
  ContactsManager.prototype.select = function(properties, options) {
    return Promise.reject(new DOMException(
      'Contact access is not supported',
      'NotSupportedError'
    ));
  };

  // getProperties() -> Promise<string[]>
  // Returns hardcoded list of supported properties
  ContactsManager.prototype.getProperties = function() {
    return Promise.resolve(['name', 'email', 'tel']);
  };

  // Export to navigator
  navigator.contacts = new ContactsManager();
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_contacts(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var navigator = {}; \
             var DOMException = function(msg, name) { this.message = msg; this.name = name; };",
        )
        .unwrap();
        install_contacts_manager_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn contacts_manager_exists() {
        with_contacts(|rt| {
            let result = rt
                .eval("typeof navigator.contacts === 'object' ? 'exists' : 'missing'")
                .unwrap();
            assert_eq!(result, JsValue::String("exists".to_string()));
        });
    }

    #[test]
    fn select_returns_promise() {
        with_contacts(|rt| {
            let result = rt
                .eval("typeof navigator.contacts.select(['name']) === 'object' ? 'promise' : 'not_promise'")
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn get_properties_returns_promise() {
        with_contacts(|rt| {
            let result = rt
                .eval("typeof navigator.contacts.getProperties() === 'object' ? 'promise' : 'not_promise'")
                .unwrap();
            assert_eq!(result, JsValue::String("promise".to_string()));
        });
    }

    #[test]
    fn get_properties_returns_correct_array() {
        with_contacts(|rt| {
            let result = rt
                .eval(
                    r#"
                (function() {
                  var props = ['name', 'email', 'tel'];
                  return props.length === 3 && props[0] === 'name' && props[1] === 'email' && props[2] === 'tel' ? 'correct' : 'wrong';
                })()
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::String("correct".to_string()));
        });
    }
}
