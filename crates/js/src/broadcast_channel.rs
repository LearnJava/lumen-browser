//! Broadcast Channel API (WHATWG HTML §9.5).
//!
//! `new BroadcastChannel(name)` lets independent browsing contexts of the *same
//! origin* (tabs, iframes, workers) talk to each other by channel name.  A
//! message posted on one channel is delivered asynchronously to every other
//! `BroadcastChannel` with the same `name` — but never back to the sender.
//!
//! **Routing model.** A process-global [`BroadcastHub`] keyed by channel name
//! holds an `mpsc::Sender<String>` (`ChannelSender`) per live channel instance.
//! `postMessage` clones the JSON payload to every same-name sender except the
//! one that posted it.  Each runtime keeps its own [`BroadcastRegistry`] of the
//! matching `Receiver`s; the shell drains them each event-loop tick via
//! `QuickJsRuntime::pump_broadcast_channels()`, which calls
//! `_lumen_deliver_broadcast_messages(msgs)` in JS so `onmessage` /
//! `addEventListener('message', fn)` handlers fire.
//!
//! Cross-thread delivery (e.g. main thread ↔ Web Worker) works because the hub
//! is a `static` shared by all runtimes in the process.

use rquickjs::{Ctx, Function};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

// ─── process-global hub ─────────────────────────────────────────────────────────

/// One subscriber of a named channel: the sender half of its delivery queue.
struct ChannelSender {
    /// Unique channel-instance id (matches a [`LocalChannel`] in some runtime).
    id: u32,
    /// Sender half; `post` pushes the JSON payload here for delivery.
    tx: Sender<String>,
}

/// Process-global registry mapping channel name → live subscribers.
///
/// Shared across every runtime/thread so same-origin contexts (including Web
/// Workers) exchange messages. Access only through [`hub`].
struct BroadcastHub {
    /// Channel name → subscribers currently listening on that name.
    channels: HashMap<String, Vec<ChannelSender>>,
    /// Monotonic counter assigning a unique id to each new channel instance.
    next_id: u32,
}

static HUB: OnceLock<Mutex<BroadcastHub>> = OnceLock::new();

/// Lazily-initialised handle to the process-global broadcast hub.
fn hub() -> &'static Mutex<BroadcastHub> {
    HUB.get_or_init(|| {
        Mutex::new(BroadcastHub {
            channels: HashMap::new(),
            next_id: 1,
        })
    })
}

// ─── per-runtime registry ───────────────────────────────────────────────────────

/// A channel instance owned by the current runtime: the receiver half plus its id.
pub struct LocalChannel {
    /// Unique channel-instance id assigned by [`register`].
    id: u32,
    /// Receiver half; drained by [`drain`] each event-loop tick.
    rx: Receiver<String>,
}

/// All `BroadcastChannel` instances created in this runtime.
///
/// Held by `QuickJsRuntime`; cloned into the native bindings so JS
/// `new BroadcastChannel(name)` / `close()` can register and unregister.
pub type BroadcastRegistry = Arc<Mutex<Vec<LocalChannel>>>;

// ─── public API ─────────────────────────────────────────────────────────────────

/// Register a new channel instance for `name` and return its unique id.
///
/// Creates an `mpsc` pair: the sender joins the global hub under `name`, the
/// receiver is stored in this runtime's `registry` for later draining.
pub fn register(registry: &BroadcastRegistry, name: &str) -> u32 {
    let (tx, rx) = mpsc::channel::<String>();
    let id = {
        let mut h = hub().lock().unwrap();
        let id = h.next_id;
        h.next_id += 1;
        h.channels
            .entry(name.to_string())
            .or_default()
            .push(ChannelSender { id, tx });
        id
    };
    registry.lock().unwrap().push(LocalChannel { id, rx });
    id
}

/// Deliver `json` to every channel named `name` except the sender (`sender_id`).
///
/// Subscribers whose receiver has been dropped (closed channel or dropped
/// runtime) are pruned on send failure, so a stale hub self-heals.
pub fn post(name: &str, sender_id: u32, json: &str) {
    let mut h = hub().lock().unwrap();
    if let Some(subs) = h.channels.get_mut(name) {
        subs.retain(|cs| {
            if cs.id == sender_id {
                return true;
            }
            cs.tx.send(json.to_string()).is_ok()
        });
        if subs.is_empty() {
            h.channels.remove(name);
        }
    }
}

/// Remove the channel instance `id` from the global hub and this runtime.
///
/// After `close`, the instance no longer receives messages and its name is
/// dropped from the hub once it has no remaining subscribers.
pub fn close(registry: &BroadcastRegistry, id: u32, name: &str) {
    {
        let mut h = hub().lock().unwrap();
        if let Some(subs) = h.channels.get_mut(name) {
            subs.retain(|cs| cs.id != id);
            if subs.is_empty() {
                h.channels.remove(name);
            }
        }
    }
    registry.lock().unwrap().retain(|c| c.id != id);
}

/// Drain all pending messages addressed to this runtime's channels.
///
/// Returns `(channel_id, json)` pairs; the receiver queues are emptied.
pub fn drain(registry: &BroadcastRegistry) -> Vec<(u32, String)> {
    let reg = registry.lock().unwrap();
    let mut out = Vec::new();
    for ch in reg.iter() {
        while let Ok(msg) = ch.rx.try_recv() {
            out.push((ch.id, msg));
        }
    }
    out
}

/// Install the `_lumen_bc_*` native bindings and the `BroadcastChannel` JS class.
///
/// Must be called after the core DOM shim so that `Event`, `MessageEvent`,
/// `DOMException`, and `JSON` are available.
pub fn install_broadcast_channel_bindings(
    ctx: &Ctx<'_>,
    registry: &BroadcastRegistry,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_bc_register(name: String) → u32
    {
        let r = Arc::clone(registry);
        reg!("_lumen_bc_register", move |name: String| -> u32 {
            register(&r, &name)
        });
    }

    // _lumen_bc_post(id: u32, name: String, json: String)
    reg!(
        "_lumen_bc_post",
        move |id: u32, name: String, json: String| {
            post(&name, id, &json);
        }
    );

    // _lumen_bc_close(id: u32, name: String)
    {
        let r = Arc::clone(registry);
        reg!("_lumen_bc_close", move |id: u32, name: String| {
            close(&r, id, &name);
        });
    }

    ctx.eval::<(), _>(BROADCAST_CHANNEL_SHIM)?;
    Ok(())
}

// ─── BroadcastChannel JS class ──────────────────────────────────────────────────

/// IIFE defining `globalThis.BroadcastChannel` and `_lumen_deliver_broadcast_messages`.
///
/// Depends on `MessageEvent` and `DOMException` (defined earlier in the DOM shim)
/// and the `_lumen_bc_register` / `_lumen_bc_post` / `_lumen_bc_close` native
/// bindings installed above.
const BROADCAST_CHANNEL_SHIM: &str = r#"(function() {
  // Registry: channel-instance id (u32) → BroadcastChannel instance.
  var _bcRegistry = {};

  function BroadcastChannel(name) {
    if (name === undefined) {
      throw new TypeError("Failed to construct 'BroadcastChannel': 1 argument required, but only 0 present.");
    }
    this.name = String(name);
    this._closed = false;
    this._onmessage = null;
    this._onmessageerror = null;
    this._listeners = { message: [], messageerror: [] };
    this._id = _lumen_bc_register(this.name);
    _bcRegistry[this._id] = this;
  }

  // postMessage(message) — broadcast a structured-clone copy to same-name channels.
  BroadcastChannel.prototype.postMessage = function(message) {
    if (this._closed) {
      throw new DOMException("BroadcastChannel is closed", "InvalidStateError");
    }
    var json;
    try {
      json = JSON.stringify(message === undefined ? null : message);
      if (json === undefined) json = 'null';
    } catch (e) {
      throw new DOMException("Failed to execute 'postMessage' on 'BroadcastChannel': value could not be cloned.", "DataCloneError");
    }
    _lumen_bc_post(this._id, this.name, json);
  };

  // close() — detach from the channel; no further messages are sent or received.
  BroadcastChannel.prototype.close = function() {
    if (this._closed) return;
    this._closed = true;
    _lumen_bc_close(this._id, this.name);
    delete _bcRegistry[this._id];
  };

  Object.defineProperty(BroadcastChannel.prototype, 'onmessage', {
    get: function() { return this._onmessage; },
    set: function(fn) { this._onmessage = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });

  Object.defineProperty(BroadcastChannel.prototype, 'onmessageerror', {
    get: function() { return this._onmessageerror; },
    set: function(fn) { this._onmessageerror = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });

  BroadcastChannel.prototype.addEventListener = function(type, fn, _opts) {
    if ((type === 'message' || type === 'messageerror') && typeof fn === 'function') {
      this._listeners[type].push(fn);
    }
  };

  BroadcastChannel.prototype.removeEventListener = function(type, fn) {
    var arr = this._listeners[type];
    if (arr) {
      var i = arr.indexOf(fn);
      if (i !== -1) arr.splice(i, 1);
    }
  };

  BroadcastChannel.prototype.dispatchEvent = function(ev) {
    this._fire(ev);
    return !(ev && ev.defaultPrevented);
  };

  // Internal: run on* handler and registered listeners for an event.
  BroadcastChannel.prototype._fire = function(ev) {
    ev.target = this;
    ev.currentTarget = this;
    var on = (ev.type === 'message') ? this._onmessage : this._onmessageerror;
    if (typeof on === 'function') { try { on.call(this, ev); } catch (e) {} }
    var arr = this._listeners[ev.type] || [];
    for (var i = 0; i < arr.length; i++) {
      try { arr[i].call(this, ev); } catch (e) {}
    }
  };

  // Internal: deliver one message routed from the hub. `data` is already the
  // structured-clone value — pump_broadcast_channels embeds the JSON payload
  // raw into the delivery array literal, so it arrives parsed (not a string).
  BroadcastChannel.prototype._deliver = function(data) {
    if (this._closed) return;
    var ev;
    // Lumen's MessageEvent constructor takes (data, init) — see dom.rs shim.
    try { ev = new MessageEvent(data); } catch (e) { ev = { type: 'message', data: data }; }
    ev.data = data;
    this._fire(ev);
  };

  globalThis.BroadcastChannel = BroadcastChannel;
  if (typeof window !== 'undefined') window.BroadcastChannel = BroadcastChannel;

  // Called by QuickJsRuntime::pump_broadcast_channels() with an array of
  // { id: u32, json: String } objects representing messages from the hub.
  globalThis._lumen_deliver_broadcast_messages = function(msgs) {
    for (var i = 0; i < msgs.length; i++) {
      var m = msgs[i];
      var ch = _bcRegistry[m.id];
      if (ch) ch._deliver(m.json);
    }
  };
})();
"#;

#[cfg(test)]
mod tests {
    use crate::QuickJsRuntime;
    use lumen_core::JsRuntime;
    use lumen_dom::Document;
    use std::sync::{Arc, Mutex};

    fn runtime() -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        let doc = Arc::new(Mutex::new(Document::new()));
        rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false).unwrap();
        rt
    }

    #[test]
    fn constructor_exposes_name() {
        let rt = runtime();
        let r = rt
            .eval("var c = new BroadcastChannel('bc-name'); c.name")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::String("bc-name".into()));
    }

    #[test]
    fn constructor_requires_name() {
        let rt = runtime();
        let r = rt
            .eval("var threw=false; try { new BroadcastChannel(); } catch(e){ threw=true; } threw")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn name_is_stringified() {
        let rt = runtime();
        let r = rt.eval("new BroadcastChannel(42).name").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("42".into()));
    }

    #[test]
    fn same_name_channels_receive_messages() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-1'); \
             var b = new BroadcastChannel('room-1'); \
             var got = null; \
             b.onmessage = function(e){ got = e.data; }; \
             a.postMessage({hello: 'world'});",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        let r = rt.eval("got && got.hello").unwrap();
        assert_eq!(r, lumen_core::JsValue::String("world".into()));
    }

    #[test]
    fn sender_does_not_receive_own_message() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-2'); \
             var b = new BroadcastChannel('room-2'); \
             var aGot = false; var bGot = false; \
             a.onmessage = function(){ aGot = true; }; \
             b.onmessage = function(){ bGot = true; }; \
             a.postMessage('ping');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(
            rt.eval("aGot").unwrap(),
            lumen_core::JsValue::Bool(false),
            "sender must not receive its own message"
        );
        assert_eq!(rt.eval("bGot").unwrap(), lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn different_names_are_isolated() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-3a'); \
             var b = new BroadcastChannel('room-3b'); \
             var got = false; \
             b.onmessage = function(){ got = true; }; \
             a.postMessage('x');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("got").unwrap(), lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn add_event_listener_receives() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-4'); \
             var b = new BroadcastChannel('room-4'); \
             var got = null; \
             b.addEventListener('message', function(e){ got = e.data; }); \
             a.postMessage(123);",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("got").unwrap(), lumen_core::JsValue::Number(123.0));
    }

    #[test]
    fn remove_event_listener_stops_delivery() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-5'); \
             var b = new BroadcastChannel('room-5'); \
             var count = 0; \
             var fn = function(){ count++; }; \
             b.addEventListener('message', fn); \
             b.removeEventListener('message', fn); \
             a.postMessage('x');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("count").unwrap(), lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn closed_channel_stops_receiving() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-6'); \
             var b = new BroadcastChannel('room-6'); \
             var got = false; \
             b.onmessage = function(){ got = true; }; \
             b.close(); \
             a.postMessage('x');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("got").unwrap(), lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn post_on_closed_channel_throws() {
        let rt = runtime();
        let r = rt
            .eval(
                "var c = new BroadcastChannel('room-7'); \
                 c.close(); \
                 var threw = false; \
                 try { c.postMessage('x'); } catch(e){ threw = true; } \
                 threw",
            )
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn message_delivered_as_message_event() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-8'); \
             var b = new BroadcastChannel('room-8'); \
             var type = null; \
             b.onmessage = function(e){ type = e.type; }; \
             a.postMessage('hi');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(
            rt.eval("type").unwrap(),
            lumen_core::JsValue::String("message".into())
        );
    }

    #[test]
    fn three_channels_fan_out() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-9'); \
             var b = new BroadcastChannel('room-9'); \
             var c = new BroadcastChannel('room-9'); \
             var bGot = 0, cGot = 0; \
             b.onmessage = function(){ bGot++; }; \
             c.onmessage = function(){ cGot++; }; \
             a.postMessage('x');",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("bGot").unwrap(), lumen_core::JsValue::Number(1.0));
        assert_eq!(rt.eval("cGot").unwrap(), lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn structured_data_roundtrips() {
        let rt = runtime();
        rt.eval(
            "var a = new BroadcastChannel('room-10'); \
             var b = new BroadcastChannel('room-10'); \
             var got = null; \
             b.onmessage = function(e){ got = e.data; }; \
             a.postMessage({n: 1, arr: [1,2,3], s: 'x'});",
        )
        .unwrap();
        rt.pump_broadcast_channels();
        assert_eq!(rt.eval("got.n").unwrap(), lumen_core::JsValue::Number(1.0));
        assert_eq!(
            rt.eval("got.arr.length").unwrap(),
            lumen_core::JsValue::Number(3.0)
        );
        assert_eq!(
            rt.eval("got.s").unwrap(),
            lumen_core::JsValue::String("x".into())
        );
    }

    #[test]
    fn window_exposes_constructor() {
        let rt = runtime();
        let r = rt
            .eval("typeof window.BroadcastChannel === 'function'")
            .unwrap();
        assert_eq!(r, lumen_core::JsValue::Bool(true));
    }
}
