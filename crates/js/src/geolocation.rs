//! Geolocation API stub (W3C Geolocation API Level 2, §5).
//!
//! Implements `navigator.geolocation` with three methods:
//! - `getCurrentPosition(success[, error[, options]])` — calls `success` once
//! - `watchPosition(success[, error[, options]])` — calls `success` on a timer loop
//! - `clearWatch(id)` — cancels a watch
//!
//! Default behaviour (no fake coords): both `getCurrentPosition` and
//! `watchPosition` immediately call the error callback with
//! `GeolocationPositionError.PERMISSION_DENIED` (code 1), matching the browser
//! behaviour when the user denies the location prompt.
//!
//! Opt-in fake coordinates: pass `Some(FakeCoords { latitude, longitude,
//! accuracy })` to `install_geolocation_bindings_v8`.  Shell code can obtain these
//! from a `FingerprintProfile` configuration field.
//!
//! Arguments of `getCurrentPosition`/`watchPosition` go through WebIDL
//! conversion first (`_checkArgs` in the shim): a missing or non-callable
//! success callback, a non-callable non-null error callback, or a
//! non-object `options` all throw `TypeError` synchronously.  `clearWatch`
//! takes no callbacks and, per spec, never throws.

/// Fake geographic coordinates injected into the Geolocation API.
///
/// When `Some`, `getCurrentPosition` and `watchPosition` call their success
/// callbacks with a synthetic `GeolocationPosition` built from these values.
/// When `None`, both methods call the error callback with `PERMISSION_DENIED`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FakeCoords {
    /// WGS-84 latitude in decimal degrees (−90 … +90).
    pub latitude: f64,
    /// WGS-84 longitude in decimal degrees (−180 … +180).
    pub longitude: f64,
    /// Estimated accuracy radius in metres (positive).
    pub accuracy: f64,
}

/// Install the Geolocation API stub into the JS context (Ph3 V8 migration S5-S7 batch 3):
/// no natives at all — `fake_coords` is only baked into an injected JS global,
/// the shim is unchanged. Must be called after the core DOM install.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_geolocation_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    fake_coords: Option<FakeCoords>,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;

    let init = match fake_coords {
        Some(c) => format!(
            "globalThis._LUMEN_GEO_COORDS = {{lat:{},lon:{},acc:{}}};",
            c.latitude, c.longitude, c.accuracy
        ),
        None => "globalThis._LUMEN_GEO_COORDS = null;".to_string(),
    };
    rt.eval(&init)?;
    rt.eval(GEO_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Geolocation API Level 2.
#[cfg(feature = "v8-backend")]
const GEO_SHIM: &str = r#"(function() {
  if (typeof navigator === 'undefined') return;

  var _coords = _LUMEN_GEO_COORDS;

  // Clean up injected global.
  try { delete globalThis._LUMEN_GEO_COORDS; } catch(_) {}

  var _watches = {};
  var _nextId = 1;

  // Polyfill for environments (tests) where setTimeout may be absent.
  var _defer = typeof setTimeout === 'function'
    ? function(fn) { setTimeout(fn, 0); }
    : function(fn) { fn(); };

  function GeolocationPositionError(code, msg) {
    this.code = code;
    this.message = msg;
  }
  GeolocationPositionError.prototype.PERMISSION_DENIED    = 1;
  GeolocationPositionError.prototype.POSITION_UNAVAILABLE = 2;
  GeolocationPositionError.prototype.TIMEOUT              = 3;
  GeolocationPositionError.PERMISSION_DENIED    = 1;
  GeolocationPositionError.POSITION_UNAVAILABLE = 2;
  GeolocationPositionError.TIMEOUT              = 3;

  function makePosition(c) {
    return {
      timestamp: typeof Date !== 'undefined' ? Date.now() : 0,
      coords: {
        latitude:         c.lat,
        longitude:        c.lon,
        accuracy:         c.acc,
        altitude:         null,
        altitudeAccuracy: null,
        heading:          null,
        speed:            null
      }
    };
  }

  function permDenied() {
    return new GeolocationPositionError(1, 'User denied Geolocation');
  }

  // WebIDL argument conversion for
  //   undefined getCurrentPosition(PositionCallback successCallback,
  //                                optional PositionErrorCallback? errorCallback = null,
  //                                optional PositionOptions options = {});
  // and the identically-typed watchPosition.  Runs synchronously, before any
  // observable side effect (no watch id is consumed on a rejected call).
  function _checkArgs(name, args) {
    var where = "Failed to execute '" + name + "' on 'Geolocation': ";
    if (args.length < 1) {
      throw new TypeError(where + '1 argument required, but only 0 present.');
    }
    // A callback function type accepts only callables: a legacy event handler
    // object ({handleEvent}) is not one.
    if (typeof args[0] !== 'function') {
      throw new TypeError(where + "parameter 1 is not of type 'Function'.");
    }
    // Nullable callback, defaulting to null: null/undefined mean "not given".
    var error = args.length > 1 ? args[1] : undefined;
    if (error !== undefined && error !== null && typeof error !== 'function') {
      throw new TypeError(where + "parameter 2 is not of type 'Function'.");
    }
    // Dictionary type: only undefined/null/Object convert (a function is an
    // Object too); members themselves are never type-checked.
    var options = args.length > 2 ? args[2] : undefined;
    if (options !== undefined && options !== null
        && typeof options !== 'object' && typeof options !== 'function') {
      throw new TypeError(where + "parameter 3 is not of type 'PositionOptions'.");
    }
  }

  var _geo = {
    getCurrentPosition: function(success, error) {
      _checkArgs('getCurrentPosition', arguments);
      if (_coords) {
        var pos = makePosition(_coords);
        _defer(function() { success(pos); });
      } else {
        var err = permDenied();
        _defer(function() { if (typeof error === 'function') error(err); });
      }
    },

    watchPosition: function(success, error) {
      _checkArgs('watchPosition', arguments);
      var id = _nextId++;
      if (_coords) {
        var fire = function() {
          if (!_watches.hasOwnProperty(id)) return;
          success(makePosition(_coords));
          _watches[id] = _defer(fire);
        };
        _watches[id] = true;
        _defer(fire);
      } else {
        var err = permDenied();
        _watches[id] = null;
        _defer(function() { if (typeof error === 'function') error(err); });
      }
      return id;
    },

    clearWatch: function(id) {
      delete _watches[id];
    }
  };

  try {
    Object.defineProperty(navigator, 'geolocation', {
      value: _geo,
      writable: false,
      configurable: true,
      enumerable: true
    });
  } catch(_) {}

  globalThis.GeolocationPositionError = GeolocationPositionError;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Minimal stubs so tests don't require the full DOM shim: `navigator` plus
    /// a synchronous `setTimeout` so `_defer`-scheduled callbacks fire inline
    /// (matches the rquickjs-era harness — avoids depending on V8's real timer
    /// queue for a test that only cares about callback ordering, not timing).
    fn with_geo(fake_coords: Option<FakeCoords>, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var navigator = {}; \
             function setTimeout(fn) { fn(); return 0; } \
             function clearTimeout(id) {}",
        )
        .unwrap();
        install_geolocation_bindings_v8(&rt, fake_coords).expect("install must succeed");
        f(&rt);
    }

    #[test]
    fn install_succeeds_no_nav() {
        // No navigator stub — the shim's `typeof navigator === 'undefined'` guard
        // must make install a no-op rather than error.
        let rt = V8JsRuntime::new().unwrap();
        install_geolocation_bindings_v8(&rt, None).expect("should succeed without navigator");
    }

    #[test]
    fn install_succeeds_with_nav() {
        with_geo(None, |_rt| {});
    }

    #[test]
    fn geolocation_is_object() {
        with_geo(None, |rt| {
            let ty = rt.eval("typeof navigator.geolocation").unwrap();
            assert_eq!(ty, JsValue::String("object".to_string()));
        });
    }

    #[test]
    fn methods_are_functions() {
        with_geo(None, |rt| {
            let ok = rt
                .eval(
                    "typeof navigator.geolocation.getCurrentPosition === 'function' && \
                     typeof navigator.geolocation.watchPosition === 'function' && \
                     typeof navigator.geolocation.clearWatch === 'function'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn permission_denied_no_fake_coords() {
        with_geo(None, |rt| {
            // setTimeout is synchronous in our stub, so the callback fires immediately.
            let code = rt
                .eval(
                    "(function() { \
                       var code = -1; \
                       navigator.geolocation.getCurrentPosition( \
                         function() { code = 0; }, \
                         function(e) { code = e.code; } \
                       ); \
                       return code; \
                     })()",
                )
                .unwrap();
            assert_eq!(code, JsValue::Number(1.0), "must call error with PERMISSION_DENIED=1");
        });
    }

    #[test]
    fn permission_denied_message() {
        with_geo(None, |rt| {
            let empty = rt
                .eval(
                    "(function() { \
                       var m = ''; \
                       navigator.geolocation.getCurrentPosition( \
                         function() {}, \
                         function(e) { m = e.message; } \
                       ); \
                       return m === ''; \
                     })()",
                )
                .unwrap();
            assert_eq!(empty, JsValue::Bool(false), "error must have a message");
        });
    }

    #[test]
    fn fake_coords_success_callback() {
        let coords = FakeCoords { latitude: 51.5074, longitude: -0.1278, accuracy: 100.0 };
        with_geo(Some(coords), |rt| {
            let lat = rt
                .eval(
                    "(function() { \
                       var lat = 0; \
                       navigator.geolocation.getCurrentPosition( \
                         function(pos) { lat = pos.coords.latitude; }, \
                         function() {} \
                       ); \
                       return lat; \
                     })()",
                )
                .unwrap();
            match lat {
                JsValue::Number(n) => assert!((n - 51.5074).abs() < 1e-6, "latitude must match fake coords"),
                other => panic!("expected number, got {other:?}"),
            }
        });
    }

    #[test]
    fn fake_coords_longitude() {
        let coords = FakeCoords { latitude: 48.8566, longitude: 2.3522, accuracy: 50.0 };
        with_geo(Some(coords), |rt| {
            let lon = rt
                .eval(
                    "(function() { \
                       var lon = 0; \
                       navigator.geolocation.getCurrentPosition( \
                         function(pos) { lon = pos.coords.longitude; }, \
                         function() {} \
                       ); \
                       return lon; \
                     })()",
                )
                .unwrap();
            match lon {
                JsValue::Number(n) => assert!((n - 2.3522).abs() < 1e-6, "longitude must match fake coords"),
                other => panic!("expected number, got {other:?}"),
            }
        });
    }

    #[test]
    fn fake_coords_has_null_altitude() {
        let coords = FakeCoords { latitude: 0.0, longitude: 0.0, accuracy: 1.0 };
        with_geo(Some(coords), |rt| {
            let alt_null = rt
                .eval(
                    "(function() { \
                       var r = false; \
                       navigator.geolocation.getCurrentPosition( \
                         function(pos) { r = pos.coords.altitude === null; }, \
                         function() {} \
                       ); \
                       return r; \
                     })()",
                )
                .unwrap();
            assert_eq!(alt_null, JsValue::Bool(true), "altitude must be null");
        });
    }

    #[test]
    fn position_has_timestamp() {
        let coords = FakeCoords { latitude: 0.0, longitude: 0.0, accuracy: 1.0 };
        with_geo(Some(coords), |rt| {
            let is_number = rt
                .eval(
                    "(function() { \
                       var t = ''; \
                       navigator.geolocation.getCurrentPosition( \
                         function(pos) { t = typeof pos.timestamp; }, \
                         function() {} \
                       ); \
                       return t === 'number'; \
                     })()",
                )
                .unwrap();
            assert_eq!(is_number, JsValue::Bool(true), "timestamp must be a number");
        });
    }

    #[test]
    fn watch_position_returns_number() {
        with_geo(None, |rt| {
            let ty = rt
                .eval(
                    "typeof navigator.geolocation.watchPosition(\
                       function(){}, function(){})",
                )
                .unwrap();
            assert_eq!(ty, JsValue::String("number".to_string()), "watchPosition must return a numeric ID");
        });
    }

    #[test]
    fn watch_ids_are_unique() {
        with_geo(None, |rt| {
            let same = rt
                .eval(
                    "(function() { \
                       var g = navigator.geolocation; \
                       var id1 = g.watchPosition(function(){}, function(){}); \
                       var id2 = g.watchPosition(function(){}, function(){}); \
                       return id1 === id2; \
                     })()",
                )
                .unwrap();
            assert_eq!(same, JsValue::Bool(false), "consecutive watch IDs must differ");
        });
    }

    #[test]
    fn clear_watch_does_not_throw() {
        with_geo(None, |rt| {
            let ok = rt
                .eval(
                    "(function() { \
                       try { \
                         var id = navigator.geolocation.watchPosition(function(){}, function(){}); \
                         navigator.geolocation.clearWatch(id); \
                         navigator.geolocation.clearWatch(999); \
                         return true; \
                       } catch(e) { return false; } \
                     })()",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true), "clearWatch must not throw");
        });
    }

    #[test]
    fn error_constants_on_prototype() {
        with_geo(None, |rt| {
            let ok = rt
                .eval(
                    "GeolocationPositionError.PERMISSION_DENIED === 1 && \
                     GeolocationPositionError.POSITION_UNAVAILABLE === 2 && \
                     GeolocationPositionError.TIMEOUT === 3",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn watch_permission_denied_no_coords() {
        with_geo(None, |rt| {
            let code = rt
                .eval(
                    "(function() { \
                       var code = -1; \
                       navigator.geolocation.watchPosition( \
                         function() {}, \
                         function(e) { code = e.code; } \
                       ); \
                       return code; \
                     })()",
                )
                .unwrap();
            assert_eq!(code, JsValue::Number(1.0), "watchPosition must error with PERMISSION_DENIED");
        });
    }

    #[test]
    fn coords_global_cleaned_up() {
        with_geo(None, |rt| {
            // _LUMEN_GEO_COORDS must be deleted after install.
            let ty = rt.eval("typeof _LUMEN_GEO_COORDS").unwrap();
            assert_eq!(ty, JsValue::String("undefined".to_string()), "_LUMEN_GEO_COORDS must be cleaned up");
        });
    }

    /// Evaluate `expr` and report whether it threw a `TypeError`.
    ///
    /// Returns `"TypeError"`, `"no-throw"`, or the constructor name of whatever
    /// else was thrown, so a wrong exception type is distinguishable from none.
    fn throw_kind(rt: &V8JsRuntime, expr: &str) -> String {
        let js = format!(
            "(function() {{ try {{ {expr}; return 'no-throw'; }} \
             catch (e) {{ return e instanceof TypeError ? 'TypeError' : String(e && e.constructor && e.constructor.name); }} }})()"
        );
        match rt.eval(&js).unwrap() {
            JsValue::String(s) => s,
            other => panic!("expected string, got {other:?}"),
        }
    }

    /// Every `assert_throws_js(TypeError, …)` case of the vendored
    /// `getCurrentPosition_TypeError.https.html` / `watchPosition_TypeError.https.html`.
    #[test]
    fn callback_webidl_type_errors() {
        // Both fake-coords modes: validation must precede the coords branch.
        for coords in [None, Some(FakeCoords { latitude: 1.0, longitude: 2.0, accuracy: 3.0 })] {
            with_geo(coords, |rt| {
                for method in ["getCurrentPosition", "watchPosition"] {
                    for args in [
                        "",                        // no arguments
                        "null",                    // null success callback
                        "null, null",              // null success and error callbacks
                        "3",                       // non-callable success callback
                        "()=>{}, 4",               // non-callable error callback
                        "()=>{}, ()=>{}, 4",       // non-object options
                        "{ handleEvent: ()=>{} }", // legacy event handler object as success
                        "()=>{}, { handleEvent: ()=>{} }", // ditto as error callback
                    ] {
                        let expr = format!("navigator.geolocation.{method}({args})");
                        assert_eq!(
                            throw_kind(rt, &expr),
                            "TypeError",
                            "{expr} must throw TypeError (coords: {coords:?})"
                        );
                    }
                }
            });
        }
    }

    /// Forms the spec accepts must keep working — in particular an omitted or
    /// explicitly null/undefined error callback and an omitted/null/object
    /// `options`, none of which may be turned into a `TypeError` by the check.
    #[test]
    fn valid_callback_forms_do_not_throw() {
        with_geo(None, |rt| {
            for method in ["getCurrentPosition", "watchPosition"] {
                for args in [
                    "()=>{}",
                    "()=>{}, null",
                    "()=>{}, undefined",
                    "()=>{}, ()=>{}",
                    "()=>{}, null, {}",
                    "()=>{}, null, null",
                    "()=>{}, null, undefined",
                    // Dictionary members are never type-checked (WPT
                    // `PositionOptions.https.html`: "No exception expected").
                    "()=>{}, null, { enableHighAccuracy: 'boom' }",
                ] {
                    let expr = format!("navigator.geolocation.{method}({args})");
                    assert_eq!(throw_kind(rt, &expr), "no-throw", "{expr} must not throw");
                }
            }
        });
    }

    /// `clearWatch` has no callback arguments: per spec (and the vendored
    /// `clearWatch_TypeError.https.html`) every invalid id is silently ignored.
    #[test]
    fn clear_watch_never_throws_on_invalid_id() {
        with_geo(None, |rt| {
            for id in ["", "NaN", "-1", "0", "1", "2147483648", "Infinity", "-Infinity", "null", "'x'"] {
                let expr = format!("navigator.geolocation.clearWatch({id})");
                assert_eq!(throw_kind(rt, &expr), "no-throw", "{expr} must not throw");
            }
        });
    }

    /// Validation happens before any side effect: a rejected `watchPosition`
    /// call must not consume a watch id.
    #[test]
    fn rejected_watch_does_not_consume_id() {
        with_geo(None, |rt| {
            let consecutive = rt
                .eval(
                    "(function() { \
                       var g = navigator.geolocation; \
                       var id1 = g.watchPosition(function(){}); \
                       try { g.watchPosition(3); } catch(_) {} \
                       var id2 = g.watchPosition(function(){}); \
                       return id2 - id1; \
                     })()",
                )
                .unwrap();
            assert_eq!(consecutive, JsValue::Number(1.0), "throwing call must not burn a watch id");
        });
    }

    #[test]
    fn geolocation_is_non_writable() {
        with_geo(None, |rt| {
            let same = rt
                .eval(
                    "(function() { \
                       var orig = navigator.geolocation; \
                       try { navigator.geolocation = {}; } catch(_) {} \
                       return navigator.geolocation === orig; \
                     })()",
                )
                .unwrap();
            assert_eq!(same, JsValue::Bool(true), "navigator.geolocation must be non-writable");
        });
    }
}
