//! Permissions API — W3C Permissions (<https://w3c.github.io/permissions/>).
//!
//! Installs `navigator.permissions` (a `Permissions` singleton), the
//! `Permissions` and `PermissionStatus` interface objects, and the internal
//! `_lumen_permission_state_changed(name)` notifier the rest of the engine
//! calls when a permission's state really moves.
//!
//! # The two rules this module exists to enforce (BUG-386)
//!
//! **1. An unrecognised name is a `TypeError`, not a `granted`.** §5.2 step 2
//! converts the descriptor through WebIDL, and `PermissionName` is an enum — a
//! name outside it fails conversion and rejects the promise. That rejection is
//! the whole feature-detection contract: a page asks
//! `query({name: 'X'})` precisely to learn whether `X` exists. The former
//! implementation answered `granted` to everything outside an 11-name deny
//! list, including `totally-made-up-permission-xyz`, so the answer carried no
//! information at all.
//!
//! **2. The state must describe what actually happens.** [`RECOGNISED`
//! names](PERMISSIONS_SHIM) each carry an explicit state; there is no implicit
//! default, so adding a name forces a decision. A name is `granted` only when
//! calling the gated API today produces its specified observable effect
//! (`navigator.clipboard.readText()` really reads the OS clipboard; cookies are
//! genuinely unpartitioned, so `storage-access` is genuinely held; `IdleDetector`
//! really polls OS idle time). Everything else — a stub that resolves without
//! doing anything, an API that always fails, an API that does not exist — is
//! `denied`. Default-closed, so a page that feature-detects and then calls the
//! API is not lied to in either direction.
//!
//! `notifications` is resolved live off `Notification.permission` rather than
//! from the table: that value is the engine's own answer to the same question
//! and the shell can change it, so copying it into a table would only let the
//! two drift apart.
//!
//! `local-fonts` lands on `denied`, which is what keeps [BUG-385]'s
//! `queryLocalFonts()` gate closed: OS font enumeration is the strongest
//! fingerprinting vector this engine could hand out, and it must not turn on
//! silently the day the Phase 1 natives land.
//!
//! # Shape
//!
//! `PermissionStatus` extends the shim's `EventTarget`, so
//! `addEventListener('change', …)` is wired to a dispatch path that really
//! runs; `name`/`state` are readonly prototype getters (`state` recomputes on
//! every read, so it can never go stale) and `onchange` is an event-handler
//! accessor. Neither interface is constructible from page script.
//! `navigator.permissions` is a non-writable own property, so a third-party
//! script can no longer replace the whole object and hand every later caller
//! forged answers (the BUG-366 class).
//!
//! Not done here: `navigator.permissions` should be an accessor on
//! `Navigator.prototype`, but this engine has no `Navigator` interface at all —
//! `navigator` is a plain object literal in `WEB_API_SHIM` and all ~48 of its
//! members are own data properties. That is [BUG-624], one change for the whole
//! object; this module follows the `navigator.credentials` precedent
//! (`crates/js/src/credentials.rs`) instead.
//!
//! [BUG-385]: ../../../bugs/BUG-385-FIXED.md
//! [BUG-624]: ../../../bugs/BUG-624-OPEN.md

/// Install the Permissions API.
///
/// Must run after the core DOM install: the shim extends `WEB_API_SHIM`'s
/// `EventTarget` and builds its `change` events with its `Event`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_permissions_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(PERMISSIONS_SHIM)?;
    Ok(())
}

/// The Permissions API shim.
///
/// Replaces the 25 lines that used to sit in `WEB_API_SHIM` under the heading
/// «Permissions API (W3C Permissions §5)», whose entire policy was one
/// `_perm_denied` array of 11 names and `granted` for everything else — see the
/// module docs for why that is not a default policy but a missing validation.
#[cfg(feature = "v8-backend")]
const PERMISSIONS_SHIM: &str = r#"(function() {
  'use strict';
  if (typeof navigator === 'undefined') return;

  // The shim's own EventTarget (`WEB_API_SHIM`), not a V8 built-in — plain V8
  // has none. Without it `PermissionStatus` could not be an EventTarget, which
  // is half of what this module is for, so bail rather than install a
  // half-working interface: no `navigator.permissions` at all makes every
  // caller's gate fail closed (that is what `queryLocalFonts()` does), while a
  // `PermissionStatus` that silently swallows `addEventListener` would not.
  var ET = globalThis.EventTarget;
  if (typeof ET !== 'function') return;

  var GRANTED = 'granted', DENIED = 'denied', PROMPT = 'prompt';

  // -- The recognised-name registry -------------------------------------------
  //
  // Every name the W3C registry defines, each with an explicit state: there is
  // no fallback branch, so a name added here has to be classified. `granted`
  // means the gated operation really has its specified effect in this engine
  // today; everything else is `denied` (module docs, rule 2).
  var STATIC = {
    // navigator.clipboard.readText()/writeText() reach the OS clipboard
    // through _lumen_clipboard_read/_lumen_clipboard_write.
    'clipboard-read':           GRANTED,
    'clipboard-write':          GRANTED,
    // Lumen does not partition storage by top-level site, so a page already
    // has the unpartitioned access this permission is about asking for.
    'storage-access':           GRANTED,
    'top-level-storage-access': GRANTED,
    // IdleDetector.start() really polls OS idle time and fires `change`, and
    // IdleDetector.requestPermission() already answers 'granted' — the two
    // answers have to agree.
    'idle-detection':           GRANTED,

    // Sensors and AV hardware: no capture path exists.
    'accelerometer':            DENIED,
    'ambient-light-sensor':     DENIED,
    'camera':                   DENIED,
    'display-capture':          DENIED,
    'gyroscope':                DENIED,
    'magnetometer':             DENIED,
    'microphone':               DENIED,
    'speaker-selection':        DENIED,
    // Device buses.
    'bluetooth':                DENIED,
    'midi':                     DENIED,
    'nfc':                      DENIED,
    // getCurrentPosition()/watchPosition() call the error callback with
    // PERMISSION_DENIED unless the shell injects fake coordinates, and nothing
    // in the workspace ever does — `denied` is the literal truth here.
    'geolocation':              DENIED,
    // Phase 0 stubs that resolve without doing anything: no background task
    // runner, no payment handler, no OS wake lock, no window placement, no XR.
    'background-fetch':         DENIED,
    'background-sync':          DENIED,
    'periodic-background-sync': DENIED,
    'payment-handler':          DENIED,
    'push':                     DENIED,
    'screen-wake-lock':         DENIED,
    'system-wake-lock':         DENIED,
    'window-management':        DENIED,
    'xr-spatial-tracking':      DENIED,
    // PressureObserver registers a callback and never fires it.
    'compute-pressure':         DENIED,
    // navigator.storage.persist() resolves `true` without marking anything
    // persistent — the `true` is the stub, not an answer.
    'persistent-storage':       DENIED,
    // requestPointerLock() dispatches pointerlockchange without locking the
    // cursor; there is no keyboard lock and no captured surface at all.
    'pointer-lock':             DENIED,
    'keyboard-lock':            DENIED,
    'captured-surface-control': DENIED,
    'automatic-fullscreen':     DENIED,
    // No OS font enumeration (BUG-385 Phase 0), and this answer is what holds
    // queryLocalFonts()'s gate shut until there is a way to ask the user.
    'local-fonts':              DENIED,
  };

  // Names the engine can answer for real, asked at query time rather than
  // copied into the table above.
  var LIVE = {
    // The Notifications API keeps the authoritative value (the shell sets it
    // at install and Notification.requestPermission() can move it), and its
    // 'default' is this spec's 'prompt'.
    'notifications': function() {
      var N = globalThis.Notification;
      if (typeof N !== 'function') return DENIED;
      var p = N.permission;
      if (p === 'granted') return GRANTED;
      if (p === 'denied') return DENIED;
      return PROMPT;
    },
  };

  var owns = Function.prototype.call.bind(Object.prototype.hasOwnProperty);

  function isRecognised(name) { return owns(STATIC, name) || owns(LIVE, name); }

  function resolveState(name) {
    if (owns(LIVE, name)) {
      // A throwing resolver must not become a `granted`: the engine failing to
      // answer is not the user saying yes.
      try { return LIVE[name](); } catch (e) { return DENIED; }
    }
    return STATIC[name];
  }

  // -- WebIDL plumbing --------------------------------------------------------

  function def(obj, name, value) {
    Object.defineProperty(obj, name, {
      value: value, writable: true, enumerable: false, configurable: true });
  }

  function defAttr(obj, name, get, set) {
    Object.defineProperty(obj, name, {
      get: get, set: set, enumerable: true, configurable: true });
  }

  function defTag(obj, tag) {
    if (typeof Symbol === 'undefined' || !Symbol.toStringTag) return;
    Object.defineProperty(obj, Symbol.toStringTag, {
      value: tag, writable: false, enumerable: false, configurable: true });
  }

  // -- PermissionStatus (§4) --------------------------------------------------

  // Private state, so `name`/`state` cannot be written over the readonly
  // getters and `status.state = 'granted'` cannot promote an answer the engine
  // already gave.
  var SLOTS = new WeakMap();

  function PermissionStatus() { throw new TypeError('Illegal constructor'); }
  PermissionStatus.prototype = Object.create(ET.prototype);
  def(PermissionStatus.prototype, 'constructor', PermissionStatus);
  defTag(PermissionStatus.prototype, 'PermissionStatus');

  function slots(o) {
    var s = SLOTS.get(o);
    if (!s) throw new TypeError('Illegal invocation');
    return s;
  }

  defAttr(PermissionStatus.prototype, 'name', function() { return slots(this).name; });
  // Recomputed on every read: a stale snapshot would report a permission the
  // engine no longer gives (or still withholds) with nothing to reveal the gap.
  defAttr(PermissionStatus.prototype, 'state', function() { return resolveState(slots(this).name); });
  defAttr(PermissionStatus.prototype, 'onchange',
    function() { return slots(this).onchange; },
    function(fn) { slots(this).onchange = (typeof fn === 'function') ? fn : null; });

  // Every status handed out, so `change` can reach all of them. Weak where the
  // runtime allows it — a page that polls query() in a loop would otherwise
  // pin every status object it ever received.
  var HAS_WEAKREF = (typeof WeakRef === 'function');
  var ISSUED = [];

  function makeStatus(name) {
    var status;
    // Reflect.construct runs EventTarget's body (which installs the listener
    // map) against PermissionStatus.prototype without going through
    // PermissionStatus itself, which is the throwing no-constructor stub.
    try {
      status = Reflect.construct(ET, [], PermissionStatus);
    } catch (e) {
      status = Object.create(PermissionStatus.prototype);
      try { ET.call(status); } catch (e2) {}
    }
    SLOTS.set(status, { name: name, onchange: null, lastSeen: resolveState(name) });
    ISSUED.push(HAS_WEAKREF ? new WeakRef(status) : status);
    return status;
  }

  // §"permission state change": tell every live status for `name` that its
  // state moved. Called by the engine, never by page script — the sealing pass
  // (BUG-378) hides and freezes `_lumen_*` globals once the install finishes.
  function stateChanged(name) {
    name = String(name);
    var kept = [];
    for (var i = 0; i < ISSUED.length; i++) {
      var ref = ISSUED[i];
      var status = HAS_WEAKREF ? ref.deref() : ref;
      if (status === undefined) continue; // collected — drop the reference
      kept.push(ref);
      var s = SLOTS.get(status);
      if (!s || s.name !== name) continue;
      var current = resolveState(name);
      // Only a real move fires. The caller does not have to know whether
      // anything changed, and a `change` for an unchanged value would be a
      // lie about the engine's state.
      if (current === s.lastSeen) continue;
      s.lastSeen = current;
      var ev = (typeof Event === 'function') ? new Event('change') : { type: 'change' };
      try { status.dispatchEvent(ev); } catch (e) {}
    }
    ISSUED = kept;
  }

  globalThis._lumen_permission_state_changed = stateChanged;

  // -- Permissions (§5) -------------------------------------------------------

  function Permissions() { throw new TypeError('Illegal constructor'); }
  defTag(Permissions.prototype, 'Permissions');

  // §5.2 step 2 — WebIDL conversion of the PermissionDescriptor. Every failure
  // here is a rejection rather than a synchronous throw, because `query` is
  // declared to return a promise.
  function readDescriptor(descriptor) {
    if (descriptor === null || descriptor === undefined
        || (typeof descriptor !== 'object' && typeof descriptor !== 'function')) {
      throw new TypeError(
        "Failed to execute 'query' on 'Permissions': the provided value is not of type 'PermissionDescriptor'");
    }
    var raw = descriptor.name;
    if (raw === undefined) {
      throw new TypeError(
        "Failed to execute 'query' on 'Permissions': required member 'name' is undefined");
    }
    var name = String(raw);
    if (!isRecognised(name)) {
      throw new TypeError(
        "Failed to execute 'query' on 'Permissions': '" + name
        + "' is not a valid value for enumeration PermissionName");
    }
    return name;
  }

  def(Permissions.prototype, 'query', function query(descriptor) {
    try {
      return Promise.resolve(makeStatus(readDescriptor(descriptor)));
    } catch (e) {
      return Promise.reject(e);
    }
  });

  // -- Installation -----------------------------------------------------------

  var permissions = Object.create(Permissions.prototype);

  // Non-writable: the whole point of the container is that a script cannot
  // swap it out and answer for the engine (BUG-366 class).
  try {
    Object.defineProperty(navigator, 'permissions', {
      value: permissions, writable: false, enumerable: true, configurable: true });
  } catch (e) { navigator.permissions = permissions; }

  // WebIDL interface objects are non-enumerable own properties of the global.
  def(globalThis, 'Permissions', Permissions);
  def(globalThis, 'PermissionStatus', PermissionStatus);
})();"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // `panic!` — штатный способ провалить тест; исключение из clippy.toml не
    // достаёт до хелперов модуля (docs/lint-policy.md §10).
    #![allow(clippy::panic)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    /// The page environment the shim needs. `EventTarget` and `Event` arrive
    /// with `dom.rs`'s shim on a real page; the copies here match the contract
    /// the module depends on — a per-type listener map plus the `on<type>`
    /// handler call inside `dispatchEvent`. The real install path is covered by
    /// the `navigator_permissions_*` tests in `dom.rs`, which run against
    /// `WEB_API_SHIM`'s own `EventTarget`.
    const STUBS: &str = r#"
        var window = globalThis;
        function Event(type) { this.type = String(type); this.target = null; }
        globalThis.Event = Event;
        function EventTarget() {
            Object.defineProperty(this, '_listeners', { value: Object.create(null), writable: true });
        }
        EventTarget.prototype.addEventListener = function(type, cb) {
            if (!cb) return;
            type = String(type);
            (this._listeners[type] || (this._listeners[type] = [])).push(cb);
        };
        EventTarget.prototype.removeEventListener = function(type, cb) {
            var list = this._listeners[String(type)];
            if (!list) return;
            var i = list.indexOf(cb);
            if (i >= 0) list.splice(i, 1);
        };
        EventTarget.prototype.dispatchEvent = function(event) {
            var list = this._listeners[String(event.type)] || [];
            event.target = this;
            for (var i = 0; i < list.slice().length; i++) { list[i].call(this, event); }
            var on = this['on' + event.type];
            if (typeof on === 'function') on.call(this, event);
            return true;
        };
        globalThis.EventTarget = EventTarget;
        var navigator = {};
    "#;

    fn with_permissions(f: impl FnOnce(&V8JsRuntime)) {
        with_permissions_setup("", f);
    }

    /// Same harness with `extra` evaluated between the stubs and the install —
    /// used to put a `Notification` on the global before the shim runs.
    fn with_permissions_setup(extra: &str, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        if !extra.is_empty() {
            rt.eval(extra).unwrap();
        }
        install_permissions_api_v8(&rt).unwrap();
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

    /// Settles `navigator.permissions.query(<descriptor>)` and reports either
    /// `resolved|<state>` or `rejected|<constructor name>|<message>`. V8 drains
    /// microtasks at the end of each `eval()`, so an already-settled promise has
    /// run its callbacks by the time the next `eval()` reads the result.
    fn query(rt: &V8JsRuntime, descriptor: &str) -> String {
        rt.eval(&format!(
            r#"
            var __out = 'never settled';
            navigator.permissions.query({descriptor}).then(
              function(status) {{ __out = 'resolved|' + status.state; }},
              function(e) {{ __out = 'rejected|' + e.constructor.name + '|' + e.message; }});
            "#
        ))
        .unwrap();
        string_eval(rt, "String(__out)")
    }

    // -- Name validation (the headline defect) ----------------------------------

    /// BUG-386: the invented name that used to come back `granted`.
    #[test]
    fn invented_name_rejects_with_a_type_error() {
        with_permissions(|rt| {
            let out = query(rt, "{ name: 'totally-made-up-permission-xyz' }");
            assert!(out.starts_with("rejected|TypeError|"), "got `{out}`");
            assert!(out.contains("PermissionName"), "got `{out}`");
        });
    }

    #[test]
    fn a_typo_in_a_real_name_rejects() {
        with_permissions(|rt| {
            assert!(query(rt, "{ name: 'geolocaton' }").starts_with("rejected|TypeError|"));
            assert!(query(rt, "{ name: 'Geolocation' }").starts_with("rejected|TypeError|"));
            assert!(query(rt, "{ name: '' }").starts_with("rejected|TypeError|"));
        });
    }

    #[test]
    fn a_missing_or_non_object_descriptor_rejects() {
        with_permissions(|rt| {
            for descriptor in ["null", "undefined", "'camera'", "42", "{}", "{ name: undefined }"] {
                let out = query(rt, descriptor);
                assert!(
                    out.starts_with("rejected|TypeError|"),
                    "descriptor `{descriptor}` gave `{out}`"
                );
            }
        });
    }

    /// A descriptor is a dictionary: unknown members are ignored, and the
    /// per-permission extras the spec defines (`sysex`, `userVisibleOnly`,
    /// `panTiltZoom`) must not turn a valid query into a rejection.
    #[test]
    fn extra_descriptor_members_are_ignored() {
        with_permissions(|rt| {
            assert_eq!(query(rt, "{ name: 'midi', sysex: true }"), "resolved|denied");
            assert_eq!(query(rt, "{ name: 'push', userVisibleOnly: true }"), "resolved|denied");
            assert_eq!(query(rt, "{ name: 'camera', panTiltZoom: true, nonsense: 1 }"), "resolved|denied");
        });
    }

    /// WebIDL conversion failures reject the promise; `query` never throws
    /// synchronously.
    #[test]
    fn query_never_throws_synchronously() {
        with_permissions(|rt| {
            assert!(bool_eval(
                rt,
                "(function() { \
                   try { navigator.permissions.query(null); return true; } \
                   catch (e) { return false; } \
                 })()"
            ));
        });
    }

    // -- States -----------------------------------------------------------------

    /// The four names BUG-386 called out: all `granted` before, none of them
    /// backed by anything the engine actually does.
    #[test]
    fn unimplemented_permissions_are_denied() {
        with_permissions(|rt| {
            for name in ["local-fonts", "geolocation", "notifications", "persistent-storage"] {
                assert_eq!(query(rt, &format!("{{ name: '{name}' }}")), "resolved|denied", "{name}");
            }
        });
    }

    /// `granted` is reserved for operations that really happen — the clipboard
    /// natives, unpartitioned cookies, OS idle polling.
    #[test]
    fn working_permissions_are_granted() {
        with_permissions(|rt| {
            for name in [
                "clipboard-read",
                "clipboard-write",
                "storage-access",
                "top-level-storage-access",
                "idle-detection",
            ] {
                assert_eq!(query(rt, &format!("{{ name: '{name}' }}")), "resolved|granted", "{name}");
            }
        });
    }

    /// The old `_perm_denied` list still answers the way it did.
    #[test]
    fn hardware_permissions_stay_denied() {
        with_permissions(|rt| {
            for name in [
                "microphone", "camera", "midi", "speaker-selection", "ambient-light-sensor",
                "accelerometer", "gyroscope", "magnetometer", "display-capture",
                "screen-wake-lock", "nfc",
            ] {
                assert_eq!(query(rt, &format!("{{ name: '{name}' }}")), "resolved|denied", "{name}");
            }
        });
    }

    /// `notifications` is not a table entry: it reports whatever the
    /// Notifications API says, including the spec's 'default' → 'prompt'.
    #[test]
    fn notifications_mirrors_the_notification_api() {
        for (permission, expected) in
            [("granted", "resolved|granted"), ("denied", "resolved|denied"), ("default", "resolved|prompt")]
        {
            with_permissions_setup(
                &format!(
                    "function Notification() {{}} \
                     Notification.permission = '{permission}'; \
                     globalThis.Notification = Notification;"
                ),
                |rt| assert_eq!(query(rt, "{ name: 'notifications' }"), expected, "{permission}"),
            );
        }
    }

    /// No Notifications API at all is a denial, not a free pass.
    #[test]
    fn notifications_without_the_api_is_denied() {
        with_permissions(|rt| {
            assert_eq!(query(rt, "{ name: 'notifications' }"), "resolved|denied");
        });
    }

    /// `state` recomputes on read, so a status handed out before the shell
    /// moved the value does not keep reporting the old one.
    #[test]
    fn state_is_recomputed_on_every_read() {
        with_permissions_setup(
            "function Notification() {} \
             Notification.permission = 'denied'; \
             globalThis.Notification = Notification;",
            |rt| {
                rt.eval(
                    "var status = null; \
                     navigator.permissions.query({ name: 'notifications' }).then(function(s) { status = s; });",
                )
                .unwrap();
                assert!(bool_eval(rt, "status.state === 'denied'"));
                rt.eval("Notification.permission = 'granted';").unwrap();
                assert!(bool_eval(rt, "status.state === 'granted'"));
            },
        );
    }

    // -- Shape ------------------------------------------------------------------

    #[test]
    fn interface_objects_are_installed_and_not_enumerable() {
        with_permissions(|rt| {
            assert!(bool_eval(rt, "typeof Permissions === 'function'"));
            assert!(bool_eval(rt, "typeof PermissionStatus === 'function'"));
            assert!(bool_eval(
                rt,
                "Object.getOwnPropertyDescriptor(globalThis, 'Permissions').enumerable === false && \
                 Object.getOwnPropertyDescriptor(globalThis, 'PermissionStatus').enumerable === false"
            ));
        });
    }

    /// Neither interface has a constructor operation in the IDL.
    #[test]
    fn neither_interface_is_constructible() {
        with_permissions(|rt| {
            for expr in [
                "new Permissions()",
                "Permissions()",
                "new PermissionStatus()",
                "PermissionStatus()",
            ] {
                assert!(
                    bool_eval(
                        rt,
                        &format!(
                            "(function() {{ try {{ {expr}; return false; }} \
                               catch (e) {{ return e instanceof TypeError; }} }})()"
                        )
                    ),
                    "`{expr}` did not throw a TypeError"
                );
            }
        });
    }

    #[test]
    fn navigator_permissions_is_a_permissions_instance() {
        with_permissions(|rt| {
            assert!(bool_eval(rt, "navigator.permissions instanceof Permissions"));
            assert!(bool_eval(
                rt,
                "Object.prototype.toString.call(navigator.permissions) === '[object Permissions]'"
            ));
            // WebIDL operations live on the prototype, not the instance.
            assert!(bool_eval(rt, "Object.keys(navigator.permissions).length === 0"));
            assert!(bool_eval(
                rt,
                "!Object.prototype.hasOwnProperty.call(navigator.permissions, 'query') && \
                 typeof Permissions.prototype.query === 'function'"
            ));
        });
    }

    /// A third-party script must not be able to replace the container and
    /// answer for the engine.
    #[test]
    fn navigator_permissions_cannot_be_overwritten() {
        with_permissions(|rt| {
            rt.eval("navigator.permissions = { query: function() { return 'forged'; } };")
                .unwrap();
            assert!(bool_eval(rt, "navigator.permissions instanceof Permissions"));
        });
    }

    #[test]
    fn permission_status_is_an_event_target() {
        with_permissions(|rt| {
            rt.eval(
                "var status = null; \
                 navigator.permissions.query({ name: 'camera' }).then(function(s) { status = s; });",
            )
            .unwrap();
            assert!(bool_eval(rt, "status instanceof PermissionStatus"));
            assert!(bool_eval(rt, "status instanceof EventTarget"));
            assert!(bool_eval(rt, "typeof status.addEventListener === 'function'"));
            assert!(bool_eval(
                rt,
                "Object.prototype.toString.call(status) === '[object PermissionStatus]'"
            ));
        });
    }

    /// `name`/`state` are readonly prototype getters, so `status.state =
    /// 'granted'` can no longer promote an answer the engine already gave.
    #[test]
    fn name_and_state_are_readonly_prototype_getters() {
        with_permissions(|rt| {
            rt.eval(
                "var status = null; \
                 navigator.permissions.query({ name: 'camera' }).then(function(s) { status = s; });",
            )
            .unwrap();
            assert!(bool_eval(
                rt,
                "!Object.prototype.hasOwnProperty.call(status, 'state') && \
                 !Object.prototype.hasOwnProperty.call(status, 'name')"
            ));
            assert!(bool_eval(
                rt,
                "typeof Object.getOwnPropertyDescriptor(PermissionStatus.prototype, 'state').get === 'function' && \
                 Object.getOwnPropertyDescriptor(PermissionStatus.prototype, 'state').set === undefined"
            ));
            rt.eval("try { status.state = 'granted'; } catch (e) {}").unwrap();
            rt.eval("try { status.name = 'clipboard-read'; } catch (e) {}").unwrap();
            assert!(bool_eval(rt, "status.state === 'denied' && status.name === 'camera'"));
        });
    }

    /// The getters belong to `PermissionStatus`, not to any object that happens
    /// to be passed as `this`.
    #[test]
    fn state_getter_rejects_a_foreign_receiver() {
        with_permissions(|rt| {
            assert!(bool_eval(
                rt,
                "(function() { \
                   var get = Object.getOwnPropertyDescriptor(PermissionStatus.prototype, 'state').get; \
                   try { get.call({}); return false; } catch (e) { return e instanceof TypeError; } \
                 })()"
            ));
        });
    }

    // -- change events ----------------------------------------------------------

    /// The subscription is wired to a dispatch path that really runs — the
    /// point of making `PermissionStatus` an EventTarget rather than an object
    /// with an inert `onchange` field.
    #[test]
    fn change_reaches_listeners_and_onchange() {
        with_permissions_setup(
            "function Notification() {} \
             Notification.permission = 'denied'; \
             globalThis.Notification = Notification;",
            |rt| {
                rt.eval(
                    "var seen = []; var status = null; \
                     navigator.permissions.query({ name: 'notifications' }).then(function(s) { \
                       status = s; \
                       s.addEventListener('change', function(e) { seen.push('listener:' + e.type + ':' + s.state); }); \
                       s.onchange = function(e) { seen.push('onchange:' + e.type + ':' + s.state); }; \
                     });",
                )
                .unwrap();
                rt.eval("Notification.permission = 'granted'; _lumen_permission_state_changed('notifications');")
                    .unwrap();
                assert_eq!(
                    string_eval(rt, "seen.join(',')"),
                    "listener:change:granted,onchange:change:granted"
                );
            },
        );
    }

    /// A notification for a name that did not actually move fires nothing — an
    /// engine that has to guess whether something changed would otherwise wake
    /// every listener on every call.
    #[test]
    fn change_does_not_fire_when_the_state_is_unmoved() {
        with_permissions_setup(
            "function Notification() {} \
             Notification.permission = 'denied'; \
             globalThis.Notification = Notification;",
            |rt| {
                rt.eval(
                    "var fired = 0; \
                     navigator.permissions.query({ name: 'notifications' }).then(function(s) { \
                       s.onchange = function() { fired++; }; \
                     });",
                )
                .unwrap();
                rt.eval("_lumen_permission_state_changed('notifications');").unwrap();
                assert!(bool_eval(rt, "fired === 0"));
            },
        );
    }

    /// Statuses for other names are left alone.
    #[test]
    fn change_only_reaches_the_named_permission() {
        with_permissions_setup(
            "function Notification() {} \
             Notification.permission = 'denied'; \
             globalThis.Notification = Notification;",
            |rt| {
                rt.eval(
                    "var other = 0; \
                     navigator.permissions.query({ name: 'camera' }).then(function(s) { \
                       s.onchange = function() { other++; }; \
                     });",
                )
                .unwrap();
                rt.eval("Notification.permission = 'granted'; _lumen_permission_state_changed('notifications');")
                    .unwrap();
                assert!(bool_eval(rt, "other === 0"));
            },
        );
    }

    /// Two statuses for the same name both hear about the move.
    #[test]
    fn change_reaches_every_live_status_for_the_name() {
        with_permissions_setup(
            "function Notification() {} \
             Notification.permission = 'denied'; \
             globalThis.Notification = Notification;",
            |rt| {
                rt.eval(
                    "var fired = 0; var a = null, b = null; \
                     navigator.permissions.query({ name: 'notifications' }).then(function(s) { \
                       a = s; s.onchange = function() { fired++; }; \
                     }); \
                     navigator.permissions.query({ name: 'notifications' }).then(function(s) { \
                       b = s; s.onchange = function() { fired++; }; \
                     });",
                )
                .unwrap();
                assert!(bool_eval(rt, "a !== b"));
                rt.eval("Notification.permission = 'granted'; _lumen_permission_state_changed('notifications');")
                    .unwrap();
                assert!(bool_eval(rt, "fired === 2"));
            },
        );
    }

    /// Every name the registry claims to recognise resolves to one of the three
    /// spec states — a typo in the table would otherwise resolve `undefined`
    /// and read as "no answer" rather than as a broken entry.
    #[test]
    fn every_recognised_name_has_a_state() {
        const NAMES: [&str; 34] = [
            "accelerometer",
            "ambient-light-sensor",
            "automatic-fullscreen",
            "background-fetch",
            "background-sync",
            "bluetooth",
            "camera",
            "captured-surface-control",
            "clipboard-read",
            "clipboard-write",
            "compute-pressure",
            "display-capture",
            "geolocation",
            "gyroscope",
            "idle-detection",
            "keyboard-lock",
            "local-fonts",
            "magnetometer",
            "microphone",
            "midi",
            "nfc",
            "notifications",
            "payment-handler",
            "periodic-background-sync",
            "persistent-storage",
            "pointer-lock",
            "push",
            "screen-wake-lock",
            "speaker-selection",
            "storage-access",
            "system-wake-lock",
            "top-level-storage-access",
            "window-management",
            "xr-spatial-tracking",
        ];
        with_permissions(|rt| {
            for name in NAMES {
                let out = query(rt, &format!("{{ name: '{name}' }}"));
                assert!(
                    matches!(out.as_str(), "resolved|granted" | "resolved|denied" | "resolved|prompt"),
                    "`{name}` gave `{out}`"
                );
            }
        });
    }
}
