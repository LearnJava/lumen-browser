//! Local Font Access API — WICG Local Font Access (<https://wicg.github.io/local-font-access/>).
//!
//! Installs:
//! - `window.queryLocalFonts(options)` → `Promise<sequence<FontData>>` — the
//!   spec's only entry point (§2). Phase 0: passes the gates, then resolves
//!   with an empty list, because nothing enumerates OS fonts yet.
//! - `FontData` — the descriptor interface (§3): four readonly attributes plus
//!   `blob()`. Not constructible from page script.
//!
//! Deliberately *not* installed: `navigator.fonts` and `FontAccessManager`.
//! Those come from the 2020 WICG draft, were dropped before the API shipped in
//! Chrome 103, and no browser or test exposes them today — so an own enumerable
//! `fonts` property on `navigator` is a fingerprint and nothing else (BUG-385,
//! same class as BUG-379).
//!
//! Phase 1: the `_lumen_local_fonts_query()` / `_lumen_local_font_blob()`
//! natives will enumerate the fonts installed on the OS and hand back their
//! bytes. The shim already calls them where they exist, behind the
//! transient-activation and `local-fonts` permission gates — an installed-font
//! list is the strongest fingerprinting vector this engine could hand out, and
//! BUG-386 still answers `granted` to every permission name it does not
//! explicitly deny.

/// V8 port of the former rquickjs `install_local_font_access_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_local_font_access_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(LOCAL_FONT_ACCESS_SHIM)?;
    Ok(())
}

/// The Local Font Access shim.
///
/// BUG-385 rewrote it. It used to be two ES5 constructor functions: `FontData`
/// assigned its four fields onto `this`, so they were writable enumerable own
/// data properties (a descriptor could be edited after it had been handed out)
/// and calling `window.FontData({family: 'LEAK'})` — a plain call, no `new` —
/// wrote those four very common names onto `window` instead of throwing;
/// `FontAccessManager` was published as `navigator.fonts`; and
/// `queryLocalFonts`, the entry point every upstream test and every real page
/// starts from, did not exist at all.
#[cfg(feature = "v8-backend")]
const LOCAL_FONT_ACCESS_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // Phase 1 natives, captured in closure scope rather than read off the global
  // object at call time: the sealing pass hides internal `_lumen*` names from
  // page script (BUG-378), and a name looked up at call time is a name page
  // script could shadow to feed the shim a font list of its own.
  function nat(name) {
    return (typeof globalThis[name] === 'function') ? globalThis[name] : null;
  }
  var NAT_QUERY = nat('_lumen_local_fonts_query');
  var NAT_BLOB  = nat('_lumen_local_font_blob');

  // -- WebIDL plumbing --------------------------------------------------------

  // A `readonly attribute` is an accessor on the interface prototype
  // (enumerable, configurable, getter only), never an own property of the
  // instance — which is why `Object.keys(fontData)` must come back empty.
  function defineAttribute(proto, name, getter) {
    Object.defineProperty(proto, name, { get: getter, enumerable: true, configurable: true });
  }
  function defineMethod(proto, name, fn) {
    Object.defineProperty(proto, name, { value: fn, writable: true, enumerable: false, configurable: true });
  }
  function defineToStringTag(ctor, name) {
    Object.defineProperty(ctor.prototype, Symbol.toStringTag,
      { value: name, writable: false, enumerable: false, configurable: true });
  }
  // An interface object, and an operation on the global, are writable and
  // configurable but NOT enumerable (WebIDL): neither shows up in
  // `for (k in window)` in a real browser, whereas the old
  // `globalThis.FontData = FontData` made it enumerable.
  function defineGlobal(name, value) {
    var desc = { value: value, writable: true, enumerable: false, configurable: true };
    Object.defineProperty(globalThis, name, desc);
    // On a page `window === globalThis` by the time this runs (dom.rs repoints
    // it); the branch is for hosts where it does not, such as the unit tests.
    if (typeof window !== 'undefined' && window !== null && window !== globalThis) {
      Object.defineProperty(window, name, desc);
    }
  }

  // -- FontData (WICG §3) -----------------------------------------------------

  // fontData -> {postscriptName, fullName, family, style}
  //
  // The values are a private slot, not own properties: as own data properties
  // they were writable, so `fd.postscriptName = 'x'` was accepted and the
  // descriptor then lied about which font it described.
  var STATE = new WeakMap();

  // The interface declares no constructor, so page script must not be able to
  // build one — `new FontData({...})` and `FontData({...})` both throw. Every
  // descriptor a page can reach comes out of `queryLocalFonts()`.
  function FontData() {
    throw new TypeError('Illegal constructor');
  }
  defineToStringTag(FontData, 'FontData');

  function stateOf(obj) {
    var st = (obj !== null && typeof obj === 'object') ? STATE.get(obj) : undefined;
    if (st === undefined) throw new TypeError('Illegal invocation');
    return st;
  }

  function defineFontAttribute(name) {
    defineAttribute(FontData.prototype, name, function() { return stateOf(this)[name]; });
  }
  defineFontAttribute('postscriptName');
  defineFontAttribute('fullName');
  defineFontAttribute('family');
  defineFontAttribute('style');

  // WICG §3.1 — the raw font bytes. Phase 0 has none to hand over, and an empty
  // Blob would be a plausible-looking lie (a page cannot tell it apart from a
  // zero-byte font file), so the promise rejects instead.
  defineMethod(FontData.prototype, 'blob', function() {
    var st = stateOf(this);
    if (!NAT_BLOB) {
      return Promise.reject(new DOMException(
        'Local font data is not available in Lumen.', 'NotSupportedError'));
    }
    return Promise.resolve().then(function() {
      return new Blob([NAT_BLOB(st.postscriptName)]);
    });
  });

  function makeFontData(descriptor) {
    function str(v) { return (v === undefined || v === null) ? '' : String(v); }
    var fd = Object.create(FontData.prototype);
    STATE.set(fd, {
      postscriptName: str(descriptor.postscriptName),
      fullName:       str(descriptor.fullName),
      family:         str(descriptor.family),
      style:          str(descriptor.style)
    });
    return fd;
  }

  // -- queryLocalFonts (WICG §2) ----------------------------------------------

  // §2 step 3. `navigator.userActivation` is the engine's own answer to the
  // question (the same source `showOpenFilePicker` consults), so the gate
  // tracks whatever that reports instead of assuming an answer of its own.
  function requireTransientActivation() {
    var activation = navigator.userActivation;
    if (activation && activation.isActive === false) {
      throw new DOMException(
        'Transient activation is required to request local fonts.', 'SecurityError');
    }
  }

  // §2 step 4. Fails closed: no Permissions API, an unusable one, or anything
  // other than an explicit `granted` all mean "no font list". Phase 0 has no
  // list to withhold, so the gate is only load-bearing once the natives land —
  // which is exactly why it goes in now and not together with them (BUG-385).
  function requireLocalFontsPermission() {
    var permissions = navigator.permissions;
    if (!permissions || typeof permissions.query !== 'function') {
      return Promise.reject(new DOMException(
        'Permission to access local fonts could not be requested.', 'NotAllowedError'));
    }
    return permissions.query({ name: 'local-fonts' }).then(
      function(status) {
        if (!status || status.state !== 'granted') {
          throw new DOMException('Permission to access local fonts was denied.', 'NotAllowedError');
        }
      },
      function() {
        throw new DOMException(
          'Permission to access local fonts could not be requested.', 'NotAllowedError');
      });
  }

  // WebIDL `optional QueryOptions options = {}`, whose one member is
  // `sequence<DOMString> postscriptNames`. Returns the requested names, or null
  // when the caller asked for everything.
  function readQueryOptions(options) {
    if (options === undefined || options === null) return null;
    if (typeof options !== 'object' && typeof options !== 'function') {
      throw new TypeError(
        "Failed to execute 'queryLocalFonts': the provided value is not of type 'QueryOptions'");
    }
    var requested = options.postscriptNames;
    if (requested === undefined) return null;
    if (requested === null || typeof requested[Symbol.iterator] !== 'function') {
      throw new TypeError("Failed to execute 'queryLocalFonts': 'postscriptNames' is not iterable");
    }
    return Array.from(requested, function(name) { return String(name); });
  }

  function enumerateFonts(requestedNames) {
    // Phase 0: no OS font enumeration. An empty list is what this engine can
    // honestly report, and it is also the only answer that leaks nothing while
    // BUG-386 leaves `local-fonts` granted by default.
    if (!NAT_QUERY) return [];
    // A native that throws, or JSON that does not parse, rejects the promise:
    // swallowing either would report "no fonts installed" for a broken bridge.
    var descriptors = JSON.parse(NAT_QUERY());
    var out = [];
    for (var i = 0; i < descriptors.length; i++) {
      var descriptor = descriptors[i];
      if (requestedNames && requestedNames.indexOf(String(descriptor.postscriptName)) < 0) continue;
      out.push(makeFontData(descriptor));
    }
    return out;
  }

  function queryLocalFonts(options) {
    var requestedNames = null;
    // A promise-returning operation reports argument-conversion failures as a
    // rejection, never as a synchronous throw (WebIDL).
    return Promise.resolve().then(function() {
      requestedNames = readQueryOptions(options);
      requireTransientActivation();
      return requireLocalFontsPermission();
    }).then(function() {
      return enumerateFonts(requestedNames);
    });
  }

  defineGlobal('FontData', FontData);
  defineGlobal('queryLocalFonts', queryLocalFonts);
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal page environment the shim needs: `window`, a `navigator` carrying
    /// the two gates it consults, and `Blob`. `DOMException` arrives with
    /// `dom.rs`'s shim on a real page; without it here every
    /// `throw new DOMException(...)` would become a `ReferenceError` and no test
    /// could see what the module actually rejects with (BUG-373's lesson).
    const STUBS: &str = r#"
        var window = globalThis;
        function Blob(parts) { this._parts = parts || []; }
        globalThis.Blob = Blob;
        var navigator = {
            userActivation: { isActive: true, hasBeenActive: true },
            permissions: {
                query: function(descriptor) {
                    return Promise.resolve({ name: descriptor.name, state: 'granted' });
                }
            }
        };
    "#;

    /// Two fonts behind the Phase 1 natives, so the descriptor shape can be
    /// tested the only way a page can reach one: through `queryLocalFonts()`.
    const MOCK_NATIVES: &str = r#"
        globalThis._lumen_local_fonts_query = function() {
            return JSON.stringify([
                { postscriptName: 'Arial-BoldMT', fullName: 'Arial Bold',
                  family: 'Arial', style: 'Bold' },
                { postscriptName: 'Inter-Regular', fullName: 'Inter Regular',
                  family: 'Inter', style: 'Regular' }
            ]);
        };
        globalThis._lumen_local_font_blob = function(postscriptName) {
            return new Uint8Array([0, 1, 2]).buffer;
        };
    "#;

    fn with_local_fonts(f: impl FnOnce(&V8JsRuntime)) {
        with_local_fonts_setup("", f);
    }

    /// Same harness with `extra` evaluated between the stubs and the install.
    /// The shim captures its natives at eval time and reads nothing off the
    /// global object afterwards, so a mock has to exist before it runs.
    fn with_local_fonts_setup(extra: &str, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        rt.eval(crate::v8_runtime::DOM_EXCEPTION_POLYFILL).unwrap();
        if !extra.is_empty() {
            rt.eval(extra).unwrap();
        }
        install_local_font_access_api_v8(&rt).unwrap();
        f(&rt);
    }

    fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
        rt.eval(expr).unwrap() == JsValue::Bool(true)
    }

    fn string_eval(rt: &V8JsRuntime, expr: &str) -> String {
        match rt.eval(expr).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected a string from `{expr}`, got {other:?}"),
        }
    }

    /// Settles the promise `expr` and reports either `resolved|<probe>` or
    /// `<is DOMException>|<name>|<message>`. V8 drains microtasks at the end of
    /// each `eval()`, so an already-settled promise has run its callbacks by the
    /// time the next `eval()` reads the result.
    fn settle(rt: &V8JsRuntime, expr: &str, probe: &str) -> String {
        rt.eval(&format!(
            r#"
            var __out = 'never settled';
            ({expr}).then(
              function(value) {{ __out = 'resolved|' + ({probe}); }},
              function(e) {{ __out = (e instanceof DOMException) + '|' + e.name + '|' + e.message; }});
            "#
        ))
        .unwrap();
        string_eval(rt, "String(__out)")
    }

    // -- Entry point ------------------------------------------------------------

    /// BUG-385: the point every upstream `font-access` test starts from
    /// (`await self.queryLocalFonts()`) did not exist at all.
    #[test]
    fn query_local_fonts_is_installed_on_the_global() {
        with_local_fonts(|rt| {
            assert!(bool_eval(rt, "typeof queryLocalFonts === 'function'"));
            assert!(bool_eval(rt, "typeof window.queryLocalFonts === 'function'"));
            assert!(bool_eval(rt, "typeof globalThis.queryLocalFonts === 'function'"));
        });
    }

    /// WebIDL: neither an interface object nor a global operation is enumerable.
    #[test]
    fn globals_are_not_enumerable() {
        with_local_fonts(|rt| {
            assert!(bool_eval(
                rt,
                "Object.getOwnPropertyDescriptor(globalThis, 'queryLocalFonts').enumerable === false && \
                 Object.getOwnPropertyDescriptor(globalThis, 'FontData').enumerable === false"
            ));
        });
    }

    /// The 2020-draft surface is gone: no browser exposes it, no test calls it,
    /// and an own enumerable `navigator.fonts` is a fingerprint by itself.
    #[test]
    fn legacy_draft_surface_is_not_installed() {
        with_local_fonts(|rt| {
            assert!(bool_eval(rt, "navigator.fonts === undefined"));
            assert!(bool_eval(rt, "typeof globalThis.FontAccessManager === 'undefined'"));
        });
    }

    #[test]
    fn query_returns_a_promise() {
        with_local_fonts(|rt| {
            assert!(bool_eval(rt, "queryLocalFonts() instanceof Promise"));
        });
    }

    /// Phase 0: the gates pass and the answer is an empty list rather than a
    /// rejection — there is simply no OS enumeration behind it yet.
    #[test]
    fn query_phase0_resolves_empty_array() {
        with_local_fonts(|rt| {
            assert_eq!(
                settle(rt, "queryLocalFonts()", "Array.isArray(value) + ':' + value.length"),
                "resolved|true:0"
            );
        });
    }

    // -- FontData shape ---------------------------------------------------------

    /// The bug's own regression check: `window.FontData({family: 'X'})` used to
    /// return `undefined` and leave four writable enumerable globals behind
    /// (`family`, `style`, `fullName`, `postscriptName`), so `window.style`
    /// shadowed whatever a page expected to find there.
    #[test]
    fn font_data_called_as_a_function_throws_and_leaks_nothing() {
        with_local_fonts(|rt| {
            assert!(bool_eval(
                rt,
                r#"
                var threw = false;
                try { window.FontData({ family: 'LEAK' }); } catch (e) { threw = e instanceof TypeError; }
                threw && !('family' in window) && !('style' in window) &&
                  !('fullName' in window) && !('postscriptName' in window)
                "#
            ));
        });
    }

    /// WebIDL: the interface declares no constructor.
    #[test]
    fn font_data_is_not_constructible() {
        with_local_fonts(|rt| {
            assert!(bool_eval(
                rt,
                "var threw = false; \
                 try { new FontData({}); } catch (e) { threw = e instanceof TypeError; } threw"
            ));
        });
    }

    #[test]
    fn font_data_has_a_to_string_tag() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(
                settle(rt, "queryLocalFonts()", "Object.prototype.toString.call(value[0])"),
                "resolved|[object FontData]"
            );
        });
    }

    /// The four attributes read correctly, live on the prototype as accessors,
    /// and are not own properties of the instance.
    #[test]
    fn font_data_attributes_are_prototype_accessors() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(
                settle(
                    rt,
                    "queryLocalFonts()",
                    "[value[0].postscriptName, value[0].fullName, value[0].family, value[0].style, \
                      Object.keys(value[0]).length, \
                      typeof Object.getOwnPropertyDescriptor(FontData.prototype, 'family').get].join('|')"
                ),
                "resolved|Arial-BoldMT|Arial Bold|Arial|Bold|0|function"
            );
        });
    }

    /// `readonly attribute`: as own data properties these let a page rewrite a
    /// descriptor it had already been handed.
    #[test]
    fn font_data_attributes_are_readonly() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(
                settle(
                    rt,
                    "queryLocalFonts()",
                    "(function(fd) { try { fd.family = 'HACKED'; } catch (e) {} return fd.family; })(value[0])"
                ),
                "resolved|Arial"
            );
        });
    }

    /// An attribute getter called on something that is not a `FontData` is an
    /// `Illegal invocation`, not `undefined`.
    #[test]
    fn font_data_getter_rejects_a_foreign_receiver() {
        with_local_fonts(|rt| {
            assert!(bool_eval(
                rt,
                "var threw = false; \
                 try { Object.getOwnPropertyDescriptor(FontData.prototype, 'family').get.call({}); } \
                 catch (e) { threw = e instanceof TypeError; } threw"
            ));
        });
    }

    // -- Query options ----------------------------------------------------------

    #[test]
    fn query_filters_by_postscript_names() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(
                settle(
                    rt,
                    "queryLocalFonts({ postscriptNames: ['Inter-Regular'] })",
                    "value.length + ':' + value[0].postscriptName"
                ),
                "resolved|1:Inter-Regular"
            );
        });
    }

    #[test]
    fn query_without_options_returns_every_font() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(settle(rt, "queryLocalFonts()", "value.length"), "resolved|2");
        });
    }

    /// WebIDL dictionary conversion: a non-object that is neither `undefined`
    /// nor `null` is a `TypeError`, reported as a rejection because the
    /// operation returns a promise.
    #[test]
    fn query_rejects_a_non_dictionary() {
        with_local_fonts(|rt| {
            assert_eq!(
                settle(rt, "queryLocalFonts(42)", "'unexpected'"),
                "false|TypeError|Failed to execute 'queryLocalFonts': the provided value is not of \
                 type 'QueryOptions'"
            );
        });
    }

    #[test]
    fn query_rejects_a_non_iterable_postscript_names() {
        with_local_fonts(|rt| {
            assert_eq!(
                settle(rt, "queryLocalFonts({ postscriptNames: 5 })", "'unexpected'"),
                "false|TypeError|Failed to execute 'queryLocalFonts': 'postscriptNames' is not iterable"
            );
        });
    }

    // -- Gates ------------------------------------------------------------------

    /// WICG §2 step 3 — without transient activation the query is a
    /// `SecurityError`, so a page cannot enumerate fonts on load.
    #[test]
    fn query_requires_transient_activation() {
        with_local_fonts_setup(
            "navigator.userActivation = { isActive: false, hasBeenActive: false };",
            |rt| {
                assert_eq!(
                    settle(rt, "queryLocalFonts()", "'unexpected'"),
                    "true|SecurityError|Transient activation is required to request local fonts."
                );
            },
        );
    }

    /// WICG §2 step 4 — a denied `local-fonts` permission is a
    /// `NotAllowedError`.
    #[test]
    fn query_requires_the_local_fonts_permission() {
        with_local_fonts_setup(
            "navigator.permissions = { query: function(d) { \
               return Promise.resolve({ name: d.name, state: 'denied' }); } };",
            |rt| {
                assert_eq!(
                    settle(rt, "queryLocalFonts()", "'unexpected'"),
                    "true|NotAllowedError|Permission to access local fonts was denied."
                );
            },
        );
    }

    /// Fails closed: no Permissions API at all is a denial, not a free pass.
    #[test]
    fn query_denies_when_the_permissions_api_is_missing() {
        with_local_fonts_setup("delete navigator.permissions;", |rt| {
            assert_eq!(
                settle(rt, "queryLocalFonts()", "'unexpected'"),
                "true|NotAllowedError|Permission to access local fonts could not be requested."
            );
        });
    }

    /// The permission is asked for by its spec name, not inferred.
    #[test]
    fn query_asks_for_the_local_fonts_permission_by_name() {
        with_local_fonts_setup(
            "var __asked = null; \
             navigator.permissions = { query: function(d) { __asked = d.name; \
               return Promise.resolve({ name: d.name, state: 'granted' }); } };",
            |rt| {
                assert_eq!(
                    settle(rt, "queryLocalFonts()", "String(__asked)"),
                    "resolved|local-fonts"
                );
            },
        );
    }

    // -- blob() -----------------------------------------------------------------

    #[test]
    fn blob_returns_the_font_bytes_when_the_native_is_present() {
        with_local_fonts_setup(MOCK_NATIVES, |rt| {
            assert_eq!(
                settle(
                    rt,
                    "queryLocalFonts().then(function(fonts) { return fonts[0].blob(); })",
                    "(value instanceof Blob) + ':' + value._parts[0].byteLength"
                ),
                "resolved|true:3"
            );
        });
    }

    /// Phase 0 has no bytes: an empty `Blob` is indistinguishable from a
    /// zero-byte font file, so the promise rejects instead.
    #[test]
    fn blob_rejects_without_the_native() {
        with_local_fonts_setup(
            r#"globalThis._lumen_local_fonts_query = function() {
                 return JSON.stringify([{ postscriptName: 'Arial-BoldMT', family: 'Arial' }]);
               };"#,
            |rt| {
                assert_eq!(
                    settle(
                        rt,
                        "queryLocalFonts().then(function(fonts) { return fonts[0].blob(); })",
                        "'unexpected'"
                    ),
                    "true|NotSupportedError|Local font data is not available in Lumen."
                );
            },
        );
    }

    /// A broken bridge must not read as "no fonts installed".
    #[test]
    fn query_rejects_when_the_native_returns_garbage() {
        with_local_fonts_setup(
            "globalThis._lumen_local_fonts_query = function() { return 'not json'; };",
            |rt| {
                let out = settle(rt, "queryLocalFonts()", "'unexpected'");
                assert!(
                    out.starts_with("false|SyntaxError|"),
                    "expected a JSON parse failure to reject, got `{out}`"
                );
            },
        );
    }
}
