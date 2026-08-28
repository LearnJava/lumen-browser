//! Процесс-глобальные переопределения `navigator.userAgent` и таймзоны
//! (WebDriver BiDi `emulation.setUserAgentOverride` /
//! `browser.setTimezoneOverride`, BUG-295).
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.

use super::*;

/// Process-global `navigator.userAgent` override (WebDriver BiDi
/// `emulation.setUserAgentOverride`, BUG-295). `None` = the WEB_API_SHIM
/// default (`Lumen/<version>`).
///
/// A process-global rather than a `V8JsRuntime` field: the shell constructs a
/// **fresh** `V8JsRuntime` on every navigation (`run_scripts_with_dom`,
/// `bfcache_thaw`, …) — there is no single long-lived instance to carry a
/// per-session override on across navigations. Lumen also runs one JS
/// runtime at a time per process, so a process-global reads identically to a
/// "session-level" BiDi override in practice (mirrors `lumen_network`'s
/// `GLOBAL_UA_OVERRIDE`/`GLOBAL_OFFLINE` statics, same rationale).
static GLOBAL_UA_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Set (or clear with `None`) the process-global `navigator.userAgent` override.
/// Consulted by every subsequent `install_dom` call (new navigation); does
/// **not** retroactively affect an already-loaded page — see
/// [`V8JsRuntime::eval`] for re-injecting into the current page.
pub fn set_global_user_agent_override(ua: Option<String>) {
    if let Ok(mut guard) = GLOBAL_UA_OVERRIDE.lock() {
        *guard = ua;
    }
}

/// The active `navigator.userAgent` override, if any.
pub(super) fn global_user_agent_override() -> Option<String> {
    GLOBAL_UA_OVERRIDE.lock().ok().and_then(|g| g.clone())
}

/// Build the JS snippet that redefines `navigator.userAgent` to `ua`
/// (BUG-295). `navigator` is a plain object literal in `WEB_API_SHIM`
/// (writable, configurable `userAgent` property), so a direct assignment is
/// enough — no `Object.defineProperty` needed. Shared between `install_dom`
/// (next-navigation application) and the shell's immediate-apply path (the
/// already-loaded page), so both go through the same escaping.
pub fn user_agent_override_script(ua: &str) -> String {
    let escaped = ua.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!("navigator.userAgent = \"{escaped}\";")
}

/// Process-global `Intl`/`Date` timezone override (WebDriver BiDi
/// `browser.setTimezoneOverride`, BUG-295). `None` = host timezone.
///
/// Same process-global rationale as [`GLOBAL_UA_OVERRIDE`] (fresh
/// `V8JsRuntime` per navigation, one runtime per process).
static GLOBAL_TIMEZONE_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Set (or clear with `None`) the process-global IANA timezone override.
/// Consulted by every subsequent `install_dom` call (new navigation); does
/// **not** retroactively affect an already-loaded page — see
/// [`timezone_override_script`] for re-injecting into the current page.
pub fn set_global_timezone_override(timezone_id: Option<String>) {
    if let Ok(mut guard) = GLOBAL_TIMEZONE_OVERRIDE.lock() {
        *guard = timezone_id;
    }
}

/// The active timezone override, if any.
pub(super) fn global_timezone_override() -> Option<String> {
    GLOBAL_TIMEZONE_OVERRIDE.lock().ok().and_then(|g| g.clone())
}

/// Build the JS snippet that sets the global timezone-override marker
/// (`globalThis.__lumen_timezone_override`) and, the first time it runs on a
/// given context, wraps `Intl.DateTimeFormat` so a construction without an
/// explicit `options.timeZone` picks up the marker (BUG-295).
///
/// Two `Intl` surfaces exist in this codebase (`crate::intl_bindings`'s
/// pure-JS ECMA-402 shim, active when the `v8` crate build lacks ICU i18n
/// data, defers to a native `Intl` otherwise) — the wrapper here covers
/// **both**: on a build with a native `Intl.DateTimeFormat` (the common
/// case; V8's bundled ICU already has full IANA tzdata, so an override like
/// `"Pacific/Kiritimati"` resolves and formats correctly, no offset table
/// needed on the Rust side) it wraps that constructor directly; the shim
/// path additionally reads the same marker itself
/// (`intl_bindings.rs::DateTimeFormat`'s `this._tz` line) so the wrap here
/// is redundant-but-harmless there. The wrap is idempotent
/// (`Intl.DateTimeFormat.__lumenPatched`) and reads the marker dynamically
/// on each call — re-running just this script (no navigation) to change the
/// override doesn't need a re-wrap, only the assignment line takes effect.
///
/// Explicit `options.timeZone` from calling JS always wins (spec behaviour
/// — a caller who names a zone should get exactly that zone, not a
/// session-wide emulation override); only the *default* (no `timeZone` key)
/// case is redirected.
pub fn timezone_override_script(timezone_id: &str) -> String {
    let escaped = timezone_id.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!(
        r#"(function() {{
  globalThis.__lumen_timezone_override = "{escaped}";
  if (typeof Intl !== 'undefined' && Intl.DateTimeFormat && !Intl.DateTimeFormat.__lumenPatched) {{
    var _Orig = Intl.DateTimeFormat;
    function LumenDateTimeFormat(locales, options) {{
      var opts = options ? Object.assign({{}}, options) : {{}};
      if (!('timeZone' in opts) && globalThis.__lumen_timezone_override) {{
        opts.timeZone = globalThis.__lumen_timezone_override;
      }}
      if (!(this instanceof LumenDateTimeFormat)) return new LumenDateTimeFormat(locales, opts);
      return new _Orig(locales, opts);
    }}
    LumenDateTimeFormat.prototype = _Orig.prototype;
    LumenDateTimeFormat.__lumenPatched = true;
    if (_Orig.supportedLocalesOf) {{
      LumenDateTimeFormat.supportedLocalesOf = _Orig.supportedLocalesOf.bind(_Orig);
    }}
    Intl.DateTimeFormat = LumenDateTimeFormat;
  }}
}})();"#
    )
}
