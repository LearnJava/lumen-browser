//! Local Font Access API — WICG spec (Phase 0 stub).
//!
//! Installs:
//! - `navigator.fonts` — `FontAccessManager` singleton.
//! - `FontAccessManager.query()` → `Promise<FontData[]>` — Phase 0: resolves
//!   with an empty array (no OS font enumeration yet).
//! - `FontData` class — `postscriptName`, `fullName`, `family`, `style`
//!   string properties + `blob()` → `Promise<Blob>` stub.
//!
//! Phase 1: `_lumen_local_fonts_query()` native binding will enumerate fonts
//! installed on the OS and return a JSON array of font descriptors.

/// V8 port of the former rquickjs `install_local_font_access_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B2): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_local_font_access_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(LOCAL_FONT_ACCESS_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const LOCAL_FONT_ACCESS_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // ── FontData ────────────────────────────────────────────────────────────────
  // WICG Local Font Access §3 — descriptor for a single installed font face.
  function FontData(descriptor) {
    this.postscriptName = descriptor.postscriptName || '';
    this.fullName       = descriptor.fullName       || '';
    this.family         = descriptor.family         || '';
    this.style          = descriptor.style          || '';
  }

  // WICG §3.1 — returns raw font bytes as a Blob.
  // Phase 0: resolves with an empty Blob.
  // Phase 1: _lumen_local_font_blob(postscriptName) → ArrayBuffer.
  FontData.prototype.blob = function() {
    if (typeof _lumen_local_font_blob === 'function') {
      try {
        var buf = _lumen_local_font_blob(this.postscriptName);
        return Promise.resolve(new Blob([buf]));
      } catch(_) {}
    }
    return Promise.resolve(new Blob([]));
  };

  globalThis.FontData = FontData;
  if (typeof window !== 'undefined') window.FontData = FontData;

  // ── FontAccessManager ────────────────────────────────────────────────────────
  // WICG Local Font Access §2 — returned as navigator.fonts.
  function FontAccessManager() {}

  // WICG §2.1 — enumerate locally installed fonts.
  // Phase 0: always resolves with [] (no OS font enumeration).
  // Phase 1: calls _lumen_local_fonts_query() → JSON array of font descriptors.
  FontAccessManager.prototype.query = function() {
    if (typeof _lumen_local_fonts_query === 'function') {
      try {
        var json = _lumen_local_fonts_query();
        var arr  = JSON.parse(json);
        return Promise.resolve(arr.map(function(d) { return new FontData(d); }));
      } catch(_) {}
    }
    return Promise.resolve([]);
  };

  // Install singleton on navigator.
  if (!navigator.fonts) {
    navigator.fonts = new FontAccessManager();
  }

  globalThis.FontAccessManager = FontAccessManager;
  if (typeof window !== 'undefined') window.FontAccessManager = FontAccessManager;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_local_fonts(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(
            "var window = globalThis; \
             var navigator = {}; \
             function Blob(parts) { this._parts = parts || []; } \
             globalThis.Blob = Blob;",
        )
        .unwrap();
        install_local_font_access_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn fonts_exists_on_navigator() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval("typeof navigator.fonts === 'object' && navigator.fonts !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn font_access_manager_class_exported() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval("typeof window.FontAccessManager === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn font_data_class_exported() {
        with_local_fonts(|rt| {
            let ok = rt.eval("typeof window.FontData === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn query_returns_promise() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval("navigator.fonts.query() instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn query_phase0_resolves_empty_array() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var resolved = null;
                    navigator.fonts.query().then(function(arr) { resolved = arr; });
                    // Promise.resolve() schedules micro-task; check it was registered.
                    typeof navigator.fonts.query === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn font_data_constructor_fields() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var fd = new FontData({
                      postscriptName: 'Arial-BoldMT',
                      fullName: 'Arial Bold',
                      family: 'Arial',
                      style: 'Bold'
                    });
                    fd.postscriptName === 'Arial-BoldMT' &&
                    fd.fullName === 'Arial Bold' &&
                    fd.family === 'Arial' &&
                    fd.style === 'Bold'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn font_data_defaults_empty_strings() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var fd = new FontData({});
                    fd.postscriptName === '' && fd.fullName === '' &&
                    fd.family === '' && fd.style === ''
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn font_data_blob_returns_promise() {
        with_local_fonts(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var fd = new FontData({ postscriptName: 'Test' });
                    fd.blob() instanceof Promise
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}
