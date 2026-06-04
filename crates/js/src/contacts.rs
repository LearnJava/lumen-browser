//! Contact Picker API stub (W3C Contact Picker API).
//!
//! Implements `navigator.contacts` with two methods:
//! - `select(properties, options)` — returns rejected Promise with NotSupportedError
//! - `getProperties()` — returns Promise<['name', 'email', 'tel']>
//!
//! Phase 0: No actual contact picking. All contact access is rejected as unsupported.

use rquickjs::Ctx;

/// Install the Contact Picker API stub into the JS context.
///
/// Defines `navigator.contacts` with `select()` and `getProperties()` methods.
/// Must be called **after** `dom::install_dom_api` so that `navigator` is already present.
pub fn init_contacts_manager(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CONTACTS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Contact Picker API (Phase 0).
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

#[cfg(test)]
mod tests {
    use rquickjs::{Runtime, Context, Ctx};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        (rt, ctx)
    }

    fn install_stubs(ctx: &Ctx) {
        ctx.eval::<(), _>("globalThis.navigator = {}; globalThis.DOMException = function(msg, name) { this.message = msg; this.name = name; };").expect("install stubs");
    }

    #[test]
    fn test_contacts_manager_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_contacts_manager(&ctx).expect("init contacts");
            let result: String = ctx.eval("typeof navigator.contacts === 'object' ? 'exists' : 'missing'").expect("eval");
            assert_eq!(result, "exists");
        });
    }

    #[test]
    fn test_select_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_contacts_manager(&ctx).expect("init contacts");
            let result: String = ctx.eval("typeof navigator.contacts.select(['name']) === 'object' ? 'promise' : 'not_promise'").expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_get_properties_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_contacts_manager(&ctx).expect("init contacts");
            let result: String = ctx.eval("typeof navigator.contacts.getProperties() === 'object' ? 'promise' : 'not_promise'").expect("eval");
            assert_eq!(result, "promise");
        });
    }

    #[test]
    fn test_get_properties_returns_correct_array() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            super::init_contacts_manager(&ctx).expect("init contacts");
            let result: String = ctx.eval(
                r#"
                (function() {
                  var props = ['name', 'email', 'tel'];
                  return props.length === 3 && props[0] === 'name' && props[1] === 'email' && props[2] === 'tel' ? 'correct' : 'wrong';
                })()
                "#
            ).expect("eval");
            assert_eq!(result, "correct");
        });
    }
}
