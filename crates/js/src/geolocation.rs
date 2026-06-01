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
//! accuracy })` to `install_geolocation_bindings`.  Shell code can obtain these
//! from a `FingerprintProfile` configuration field.

use rquickjs::Ctx;

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

/// Install the Geolocation API stub into the JS context.
///
/// Defines `navigator.geolocation` with `getCurrentPosition`, `watchPosition`,
/// and `clearWatch`.  Must be called **after** `dom::install_dom_api` so that
/// `navigator` and `setTimeout` are already present.
///
/// - `fake_coords = None` → every position request triggers `PERMISSION_DENIED`.
/// - `fake_coords = Some(c)` → every position request fires the success callback
///   with the provided coordinates.
pub fn install_geolocation_bindings(ctx: &Ctx, fake_coords: Option<FakeCoords>) -> rquickjs::Result<()> {
    // Inject the coords as a JS global so the IIFE can read them.
    // Assign via globalThis (not `var`) so the JS shim can delete the property.
    // `var` declarations are non-configurable and cannot be removed with `delete`.
    let init = match fake_coords {
        Some(c) => format!(
            "globalThis._LUMEN_GEO_COORDS = {{lat:{},lon:{},acc:{}}};",
            c.latitude, c.longitude, c.accuracy
        ),
        None => "globalThis._LUMEN_GEO_COORDS = null;".to_string(),
    };
    ctx.eval::<(), _>(init.as_str())?;
    ctx.eval::<(), _>(GEO_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing the W3C Geolocation API Level 2.
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

  var _geo = {
    getCurrentPosition: function(success, error) {
      if (_coords) {
        var pos = makePosition(_coords);
        _defer(function() { if (typeof success === 'function') success(pos); });
      } else {
        var err = permDenied();
        _defer(function() { if (typeof error === 'function') error(err); });
      }
    },

    watchPosition: function(success, error) {
      var id = _nextId++;
      if (_coords) {
        var fire = function() {
          if (!_watches.hasOwnProperty(id)) return;
          if (typeof success === 'function') success(makePosition(_coords));
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    /// Minimal stubs so tests don't require the full DOM shim.
    fn install_stubs(ctx: &rquickjs::Ctx) {
        // navigator + synchronous setTimeout so callbacks fire immediately.
        ctx.eval::<(), _>(
            "var navigator = {}; \
             var _timeouts = []; \
             function setTimeout(fn) { fn(); return 0; } \
             function clearTimeout(id) {}",
        )
        .unwrap();
    }

    #[test]
    fn install_succeeds_no_nav() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_geolocation_bindings(&ctx, None).expect("should succeed without navigator");
        });
    }

    #[test]
    fn install_succeeds_with_nav() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).expect("should succeed");
        });
    }

    #[test]
    fn geolocation_is_object() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let ty: String = ctx.eval("typeof navigator.geolocation").unwrap();
            assert_eq!(ty, "object");
        });
    }

    #[test]
    fn methods_are_functions() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let gcp: String = ctx
                .eval("typeof navigator.geolocation.getCurrentPosition")
                .unwrap();
            let wp: String = ctx
                .eval("typeof navigator.geolocation.watchPosition")
                .unwrap();
            let cw: String = ctx
                .eval("typeof navigator.geolocation.clearWatch")
                .unwrap();
            assert_eq!(gcp, "function");
            assert_eq!(wp, "function");
            assert_eq!(cw, "function");
        });
    }

    #[test]
    fn permission_denied_no_fake_coords() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            // setTimeout is synchronous in our stub, so the callback fires immediately.
            let code: f64 = ctx
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
            assert_eq!(code as u32, 1, "must call error with PERMISSION_DENIED=1");
        });
    }

    #[test]
    fn permission_denied_message() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let msg: String = ctx
                .eval(
                    "(function() { \
                       var m = ''; \
                       navigator.geolocation.getCurrentPosition( \
                         function() {}, \
                         function(e) { m = e.message; } \
                       ); \
                       return m; \
                     })()",
                )
                .unwrap();
            assert!(!msg.is_empty(), "error must have a message");
        });
    }

    #[test]
    fn fake_coords_success_callback() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            let coords = FakeCoords {
                latitude: 51.5074,
                longitude: -0.1278,
                accuracy: 100.0,
            };
            install_geolocation_bindings(&ctx, Some(coords)).unwrap();
            let lat: f64 = ctx
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
            assert!(
                (lat - 51.5074).abs() < 1e-6,
                "latitude must match fake coords"
            );
        });
    }

    #[test]
    fn fake_coords_longitude() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            let coords = FakeCoords {
                latitude: 48.8566,
                longitude: 2.3522,
                accuracy: 50.0,
            };
            install_geolocation_bindings(&ctx, Some(coords)).unwrap();
            let lon: f64 = ctx
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
            assert!((lon - 2.3522).abs() < 1e-6, "longitude must match fake coords");
        });
    }

    #[test]
    fn fake_coords_has_null_altitude() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            let coords = FakeCoords {
                latitude: 0.0,
                longitude: 0.0,
                accuracy: 1.0,
            };
            install_geolocation_bindings(&ctx, Some(coords)).unwrap();
            let alt_null: bool = ctx
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
            assert!(alt_null, "altitude must be null");
        });
    }

    #[test]
    fn position_has_timestamp() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            // Need Date.now for timestamp; provide a stub.
            ctx.eval::<(), _>("var Date = { now: function() { return 1000; } };")
                .unwrap();
            let coords = FakeCoords {
                latitude: 0.0,
                longitude: 0.0,
                accuracy: 1.0,
            };
            install_geolocation_bindings(&ctx, Some(coords)).unwrap();
            let ts_type: String = ctx
                .eval(
                    "(function() { \
                       var t = ''; \
                       navigator.geolocation.getCurrentPosition( \
                         function(pos) { t = typeof pos.timestamp; }, \
                         function() {} \
                       ); \
                       return t; \
                     })()",
                )
                .unwrap();
            assert_eq!(ts_type, "number", "timestamp must be a number");
        });
    }

    #[test]
    fn watch_position_returns_number() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let ty: String = ctx
                .eval(
                    "typeof navigator.geolocation.watchPosition(\
                       function(){}, function(){})",
                )
                .unwrap();
            assert_eq!(ty, "number", "watchPosition must return a numeric ID");
        });
    }

    #[test]
    fn watch_ids_are_unique() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let same: bool = ctx
                .eval(
                    "(function() { \
                       var g = navigator.geolocation; \
                       var id1 = g.watchPosition(function(){}, function(){}); \
                       var id2 = g.watchPosition(function(){}, function(){}); \
                       return id1 === id2; \
                     })()",
                )
                .unwrap();
            assert!(!same, "consecutive watch IDs must differ");
        });
    }

    #[test]
    fn clear_watch_does_not_throw() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let ok: bool = ctx
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
            assert!(ok, "clearWatch must not throw");
        });
    }

    #[test]
    fn error_constants_on_prototype() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let pd: f64 = ctx
                .eval("GeolocationPositionError.PERMISSION_DENIED")
                .unwrap();
            let pu: f64 = ctx
                .eval("GeolocationPositionError.POSITION_UNAVAILABLE")
                .unwrap();
            let to: f64 = ctx.eval("GeolocationPositionError.TIMEOUT").unwrap();
            assert_eq!(pd as u32, 1);
            assert_eq!(pu as u32, 2);
            assert_eq!(to as u32, 3);
        });
    }

    #[test]
    fn watch_permission_denied_no_coords() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let code: f64 = ctx
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
            assert_eq!(code as u32, 1, "watchPosition must error with PERMISSION_DENIED");
        });
    }

    #[test]
    fn coords_global_cleaned_up() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            // _LUMEN_GEO_COORDS must be deleted after install.
            let ty: String = ctx.eval("typeof _LUMEN_GEO_COORDS").unwrap();
            assert_eq!(ty, "undefined", "_LUMEN_GEO_COORDS must be cleaned up");
        });
    }

    #[test]
    fn geolocation_is_non_writable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_geolocation_bindings(&ctx, None).unwrap();
            let same: bool = ctx
                .eval(
                    "(function() { \
                       var orig = navigator.geolocation; \
                       try { navigator.geolocation = {}; } catch(_) {} \
                       return navigator.geolocation === orig; \
                     })()",
                )
                .unwrap();
            assert!(same, "navigator.geolocation must be non-writable");
        });
    }
}
