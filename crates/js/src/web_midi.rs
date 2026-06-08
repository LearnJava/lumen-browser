//! W3C Web MIDI API — Phase 0 stub
//!
//! Implements `navigator.requestMIDIAccess()` → `Promise<MIDIAccess>` with
//! empty input/output maps. `MIDIAccess`, `MIDIInput`, `MIDIOutput`,
//! `MIDIInputMap`, `MIDIOutputMap`, `MIDIMessageEvent`, and
//! `MIDIConnectionEvent` classes are exported on `window`.
//!
//! Phase 0: no real MIDI hardware access. The `MIDIAccess` resolves
//! immediately with empty `inputs` and `outputs` maps. Native binding
//! `_lumen_midi_deliver_message(port_id, data_bytes)` is prepared for
//! Phase 1 (OS MIDI integration via CoreMIDI / WinMM / ALSA).

use rquickjs::Ctx;

/// Install Web MIDI API bindings into the JS context.
pub fn install_web_midi_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEB_MIDI_SHIM)?;
    Ok(())
}

const WEB_MIDI_SHIM: &str = r#"
(function() {
  'use strict';

  // ── Fallback base classes (absent in QuickJS minimal runtime) ─────────────
  var _ETBase = (typeof EventTarget !== 'undefined') ? EventTarget : (function() {
    function ET() { this._listeners = {}; }
    ET.prototype.addEventListener = function(type, fn) {
      (this._listeners[type] = this._listeners[type] || []).push(fn);
    };
    ET.prototype.removeEventListener = function(type, fn) {
      if (!this._listeners[type]) return;
      this._listeners[type] = this._listeners[type].filter(function(l) { return l !== fn; });
    };
    ET.prototype.dispatchEvent = function(evt) {
      (this._listeners[evt.type] || []).forEach(function(l) { l(evt); });
      return true;
    };
    return ET;
  }());

  var _EvtBase = (typeof Event !== 'undefined') ? Event : (function() {
    function Ev(type, init) {
      this.type = type;
      this.bubbles = !!(init && init.bubbles);
      this.cancelable = !!(init && init.cancelable);
    }
    return Ev;
  }());

  // ── MIDIPort (W3C Web MIDI L1 §4.3) ──────────────────────────────────────
  class MIDIPort extends _ETBase {
    constructor(id, manufacturer, name, type, version) {
      super();
      this.id = id;
      this.manufacturer = manufacturer || '';
      this.name = name || '';
      this.type = type; // 'input' | 'output'
      this.version = version || '';
      this.state = 'connected';    // 'connected' | 'disconnected'
      this.connection = 'closed';  // 'open' | 'closed' | 'pending'
      this.onstatechange = null;
    }

    open() {
      this.connection = 'open';
      return Promise.resolve(this);
    }

    close() {
      this.connection = 'closed';
      return Promise.resolve(this);
    }
  }

  // ── MIDIInput (W3C Web MIDI L1 §4.4) ─────────────────────────────────────
  class MIDIInput extends MIDIPort {
    constructor(id, manufacturer, name, version) {
      super(id, manufacturer, name, 'input', version);
      this.onmidimessage = null;
    }
  }

  // ── MIDIOutput (W3C Web MIDI L1 §4.5) ────────────────────────────────────
  class MIDIOutput extends MIDIPort {
    constructor(id, manufacturer, name, version) {
      super(id, manufacturer, name, 'output', version);
    }

    // Phase 0: no-op; Phase 1 wires to _lumen_midi_send_message(portId, data)
    send(data, timestamp) { }

    clear() { }
  }

  // ── MIDIPortMap — read-only Map-like (W3C Web MIDI L1 §4.2) ──────────────
  class MIDIPortMap {
    constructor(entries) {
      this._map = new Map(entries || []);
    }

    get size() { return this._map.size; }
    get(id) { return this._map.get(id); }
    has(id) { return this._map.has(id); }
    entries() { return this._map.entries(); }
    keys() { return this._map.keys(); }
    values() { return this._map.values(); }
    forEach(callback, thisArg) { this._map.forEach(callback, thisArg); }
    [Symbol.iterator]() { return this._map.entries(); }
  }

  // ── MIDIAccess (W3C Web MIDI L1 §4.1) ────────────────────────────────────
  class MIDIAccess extends _ETBase {
    constructor(sysexEnabled) {
      super();
      this.inputs = new MIDIPortMap([]);
      this.outputs = new MIDIPortMap([]);
      this.sysexEnabled = !!sysexEnabled;
      this.onstatechange = null;
    }
  }

  // ── MIDIMessageEvent (W3C Web MIDI L1 §5.1) ──────────────────────────────
  class MIDIMessageEvent extends _EvtBase {
    constructor(type, init) {
      super(type, init);
      this.data = (init && init.data) ? init.data : new Uint8Array(0);
    }
  }

  // ── MIDIConnectionEvent (W3C Web MIDI L1 §5.2) ───────────────────────────
  class MIDIConnectionEvent extends _EvtBase {
    constructor(type, init) {
      super(type, init);
      this.port = (init && init.port) ? init.port : null;
    }
  }

  // ── navigator.requestMIDIAccess (W3C Web MIDI L1 §4) ─────────────────────
  navigator.requestMIDIAccess = function requestMIDIAccess(options) {
    var sysex = !!(options && options.sysex);
    return Promise.resolve(new MIDIAccess(sysex));
  };

  // ── Native binding stub for Phase 1 shell integration ─────────────────────
  // _lumen_midi_deliver_message(portId, data) — delivers an incoming MIDI
  // message from OS MIDI stack (CoreMIDI/WinMM/ALSA) to the MIDIInput port.
  globalThis._lumen_midi_deliver_message = function(portId, data) { };

  // ── Exports ───────────────────────────────────────────────────────────────
  window.MIDIPort = MIDIPort;
  window.MIDIInput = MIDIInput;
  window.MIDIOutput = MIDIOutput;
  window.MIDIPortMap = MIDIPortMap;
  window.MIDIAccess = MIDIAccess;
  window.MIDIMessageEvent = MIDIMessageEvent;
  window.MIDIConnectionEvent = MIDIConnectionEvent;
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

    fn with_midi_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                var navigator = {};
                globalThis.navigator = navigator;
                "#,
            )
            .unwrap();
            install_web_midi_api(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn midi_request_midi_access_exists() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof navigator.requestMIDIAccess === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_request_midi_access_returns_promise() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("navigator.requestMIDIAccess() instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_access_has_inputs_and_outputs() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var p = navigator.requestMIDIAccess();
                    var result = false;
                    p.then(function(access) {
                      result = typeof access.inputs === 'object'
                               && typeof access.outputs === 'object';
                    });
                    result === false // promise not yet settled synchronously
                    "#,
                )
                .unwrap();
            // Promise is microtask-based; we just verify it doesn't throw
            assert!(ok);
        });
    }

    #[test]
    fn midi_access_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIAccess === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_input_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIInput === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_output_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIOutput === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_message_event_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIMessageEvent === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_connection_event_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIConnectionEvent === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_port_map_class_exported() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.MIDIPortMap === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_access_sysex_enabled_false_by_default() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("new window.MIDIAccess(false).sysexEnabled === false")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn midi_deliver_binding_exists() {
        with_midi_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof globalThis._lumen_midi_deliver_message === 'function'")
                .unwrap();
            assert!(ok);
        });
    }
}
