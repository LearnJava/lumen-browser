//! WebRTC mDNS-only stub (W3C WebRTC §9D.5).
//!
//! Implements `RTCPeerConnection`, `RTCSessionDescription`, and `RTCIceCandidate`
//! as no-op stubs that never leak the real IP address.  Instead of gathering
//! real network candidates, `onicecandidate` fires exactly one synthetic candidate
//! whose host address is a UUID-based mDNS `.local` name, followed by the
//! end-of-candidates `null` event.
//!
//! This covers the common feature-detection pattern:
//! ```js
//! const pc = new RTCPeerConnection();
//! pc.onicecandidate = e => { if (e.candidate) use(e.candidate); };
//! const offer = await pc.createOffer();
//! await pc.setLocalDescription(offer);
//! ```
//! while keeping IP addresses private (§12 Unique Features — anti-fingerprinting).

use rquickjs::Ctx;

/// Install the WebRTC mDNS-only stub into the JS context.
///
/// Defines `RTCPeerConnection`, `RTCSessionDescription`, and `RTCIceCandidate`
/// on `globalThis`.  Must be called **after** `dom::install_dom_api` so that
/// `setTimeout`, `Promise`, and `queueMicrotask` are already available.
///
/// No native Rust bindings are needed — the stub is a pure JS shim.
pub fn install_webrtc_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEBRTC_SHIM)
}

/// JavaScript shim implementing a WebRTC mDNS-only stub.
///
/// Candidate format: `candidate:1 1 UDP 2122262783 <uuid>.local <port> typ host`
/// where `<uuid>` is a random UUID v4 and `<port>` is a random ephemeral port.
/// Both are generated once per page load to be stable within a session but
/// uncorrelated with the real network interface.
const WEBRTC_SHIM: &str = r#"(function() {
  'use strict';

  // Defer helper: prefer queueMicrotask, fall back to setTimeout, then sync.
  var _defer = typeof queueMicrotask === 'function'
    ? function(fn) { queueMicrotask(fn); }
    : typeof setTimeout === 'function'
    ? function(fn) { setTimeout(fn, 0); }
    : function(fn) { fn(); };

  // Wrap val in a resolved Promise-like.  Uses real Promise when available
  // (QuickJS includes it); falls back to a synchronous thenable for test stubs.
  function _resolved(val) {
    if (typeof Promise !== 'undefined') {
      return Promise.resolve(val);
    }
    var called = false;
    return {
      then: function(fn, _rej) {
        if (!called) { called = true; _defer(function() { fn(val); }); }
        return _resolved(undefined);
      },
      catch: function() { return this; },
      finally: function(fn) { _defer(fn); return this; }
    };
  }

  // Generate a random UUID v4 string for the mDNS candidate address.
  // Uses Math.random — no crypto required, privacy guarantee is the .local suffix.
  function _uuid4() {
    var h = '0123456789abcdef';
    var u = '';
    for (var i = 0; i < 32; i++) {
      if (i === 8 || i === 12 || i === 16 || i === 20) u += '-';
      var r = Math.floor(Math.random() * 16);
      if (i === 12) r = 4;
      if (i === 16) r = (r & 3) | 8;
      u += h[r];
    }
    return u;
  }

  // Stable per-page-load values — generated once at shim install time.
  var _MDNS_UUID = _uuid4();
  var _MDNS_PORT = 10000 + Math.floor(Math.random() * 55535);
  var _MDNS_ADDR = _MDNS_UUID + '.local';
  var _MDNS_CANDIDATE_STR =
    'candidate:1 1 UDP 2122262783 ' + _MDNS_ADDR + ' ' + _MDNS_PORT + ' typ host';

  // ── RTCSessionDescription ────────────────────────────────────────────────

  function RTCSessionDescription(init) {
    this.type = (init && init.type) || '';
    this.sdp  = (init && init.sdp)  || '';
  }
  RTCSessionDescription.prototype.toJSON = function() {
    return { type: this.type, sdp: this.sdp };
  };

  // ── RTCIceCandidate ──────────────────────────────────────────────────────

  function RTCIceCandidate(init) {
    this.candidate       = (init && init.candidate !== undefined) ? init.candidate : '';
    this.sdpMid          = (init && init.sdpMid !== undefined) ? init.sdpMid : null;
    this.sdpMLineIndex   = (init && init.sdpMLineIndex !== undefined) ? init.sdpMLineIndex : null;
    this.usernameFragment = null;
    this.foundation      = '1';
    this.component       = 'rtp';
    this.priority        = 2122262783;
    this.address         = null;   // intentionally hidden — use .local in candidate string
    this.protocol        = 'udp';
    this.port            = null;
    this.type            = 'host';
    this.tcpType         = null;
    this.relatedAddress  = null;
    this.relatedPort     = null;
  }
  RTCIceCandidate.prototype.toJSON = function() {
    return {
      candidate:        this.candidate,
      sdpMid:           this.sdpMid,
      sdpMLineIndex:    this.sdpMLineIndex,
      usernameFragment: this.usernameFragment
    };
  };

  // ── RTCPeerConnection ────────────────────────────────────────────────────

  function RTCPeerConnection(config) {
    this._config            = config || {};
    this._localDescription  = null;
    this._remoteDescription = null;
    this._signalingState    = 'stable';
    this._iceGatheringState = 'new';
    this._iceConnState      = 'new';
    this._closed            = false;
    this._listeners         = {};
    // Public event handler properties.
    this.onicecandidate         = null;
    this.onicecandidateerror    = null;
    this.onsignalingstatechange = null;
    this.oniceconnectionstatechange = null;
    this.onicegatheringstatechange  = null;
    this.onnegotiationneeded    = null;
    this.ontrack                = null;
    this.ondatachannel          = null;
    this.onconnectionstatechange = null;
  }

  Object.defineProperties(RTCPeerConnection.prototype, {
    localDescription:  { get: function() { return this._localDescription; } },
    remoteDescription: { get: function() { return this._remoteDescription; } },
    signalingState:    { get: function() { return this._signalingState; } },
    iceGatheringState: { get: function() { return this._iceGatheringState; } },
    iceConnectionState: { get: function() { return this._iceConnState; } },
    connectionState:   { get: function() { return this._closed ? 'closed' : 'new'; } }
  });

  RTCPeerConnection.prototype._dispatch = function(type, evt) {
    var handlers = this._listeners[type];
    if (handlers) {
      for (var i = 0; i < handlers.length; i++) handlers[i](evt);
    }
  };

  RTCPeerConnection.prototype._gatherMdns = function() {
    if (this._closed) return;
    this._iceGatheringState = 'gathering';
    var cand = new RTCIceCandidate({
      candidate: _MDNS_CANDIDATE_STR,
      sdpMid: '0',
      sdpMLineIndex: 0
    });
    var evt = { candidate: cand };
    if (typeof this.onicecandidate === 'function') this.onicecandidate(evt);
    this._dispatch('icecandidate', evt);
    // End-of-candidates.
    this._iceGatheringState = 'complete';
    var endEvt = { candidate: null };
    if (typeof this.onicecandidate === 'function') this.onicecandidate(endEvt);
    this._dispatch('icecandidate', endEvt);
  };

  RTCPeerConnection.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };
  RTCPeerConnection.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(h) { return h !== fn; });
  };
  RTCPeerConnection.prototype.dispatchEvent = function(evt) {
    this._dispatch(evt.type, evt);
    return true;
  };

  // Minimal SDP to satisfy `pc.localDescription.sdp` checks.
  var _OFFER_SDP =
    'v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n' +
    'a=group:BUNDLE 0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n' +
    'c=IN IP4 0.0.0.0\r\na=mid:0\r\n';
  var _ANSWER_SDP =
    'v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\n' +
    'm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\na=mid:0\r\n';

  RTCPeerConnection.prototype.createOffer = function(_opts) {
    return _resolved(new RTCSessionDescription({ type: 'offer', sdp: _OFFER_SDP }));
  };
  RTCPeerConnection.prototype.createAnswer = function(_opts) {
    return _resolved(new RTCSessionDescription({ type: 'answer', sdp: _ANSWER_SDP }));
  };

  RTCPeerConnection.prototype.setLocalDescription = function(desc) {
    var d = (desc instanceof RTCSessionDescription) ? desc : new RTCSessionDescription(desc);
    this._localDescription = d;
    if (d.type === 'offer' || d.type === 'pranswer') {
      this._signalingState = 'have-local-offer';
      var self = this;
      _defer(function() { self._gatherMdns(); });
    } else if (d.type === 'answer') {
      this._signalingState = 'stable';
    }
    return _resolved(undefined);
  };
  RTCPeerConnection.prototype.setRemoteDescription = function(desc) {
    var d = (desc instanceof RTCSessionDescription) ? desc : new RTCSessionDescription(desc);
    this._remoteDescription = d;
    if (d.type === 'offer') {
      this._signalingState = 'have-remote-offer';
    } else if (d.type === 'answer') {
      this._signalingState = 'stable';
    }
    return _resolved(undefined);
  };

  RTCPeerConnection.prototype.addIceCandidate = function(_cand) {
    return _resolved(undefined);
  };
  RTCPeerConnection.prototype.close = function() {
    this._closed = true;
    this._signalingState = 'closed';
    this._iceConnState   = 'closed';
  };

  // Stub media/track methods — enough to satisfy feature detection.
  RTCPeerConnection.prototype.addTransceiver    = function() { return null; };
  RTCPeerConnection.prototype.addTrack          = function() { return null; };
  RTCPeerConnection.prototype.removeTrack       = function() {};
  RTCPeerConnection.prototype.getTransceivers   = function() { return []; };
  RTCPeerConnection.prototype.getSenders        = function() { return []; };
  RTCPeerConnection.prototype.getReceivers      = function() { return []; };
  RTCPeerConnection.prototype.getStats          = function() { return _resolved(new Map()); };
  RTCPeerConnection.prototype.createDataChannel = function(label) {
    return {
      label: label || '',
      readyState: 'connecting',
      bufferedAmount: 0,
      send: function() {},
      close: function() {},
      onopen: null, onmessage: null, onerror: null, onclose: null,
      addEventListener: function() {}, removeEventListener: function() {}
    };
  };

  RTCPeerConnection.generateCertificate = function() { return _resolved(null); };

  // Register on globalThis.
  globalThis.RTCPeerConnection    = RTCPeerConnection;
  globalThis.RTCSessionDescription = RTCSessionDescription;
  globalThis.RTCIceCandidate      = RTCIceCandidate;
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

    /// Synchronous setTimeout + minimal stubs for tests that need deferred callbacks.
    fn install_stubs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            "function setTimeout(fn, d) { fn(); return 0; } \
             function clearTimeout(id) {} \
             function queueMicrotask(fn) { fn(); }",
        )
        .unwrap();
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).expect("install must succeed");
        });
    }

    #[test]
    fn rtcpeerconnection_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof RTCPeerConnection").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn rtcsessiondescription_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof RTCSessionDescription").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn rtcicecandidate_is_function() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx.eval("typeof RTCIceCandidate").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn rtcsessiondescription_has_type_and_sdp() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("new RTCSessionDescription({type:'offer',sdp:'v=0'}).type")
                .unwrap();
            let sdp: String = ctx
                .eval("new RTCSessionDescription({type:'offer',sdp:'v=0'}).sdp")
                .unwrap();
            assert_eq!(ty, "offer");
            assert_eq!(sdp, "v=0");
        });
    }

    #[test]
    fn rtcicecandidate_has_candidate_string() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let cand: String = ctx
                .eval("new RTCIceCandidate({candidate:'candidate:1 1 UDP 123 x.local 9 typ host'}).candidate")
                .unwrap();
            assert!(cand.contains("candidate:"));
        });
    }

    #[test]
    fn create_offer_returns_thenable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval("typeof new RTCPeerConnection().createOffer().then")
                .unwrap();
            assert_eq!(ty, "function", "createOffer() must return a thenable");
        });
    }

    #[test]
    fn create_offer_resolves_to_offer_type() {
        let (rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            // Schedule Promise callback — result is written to a global.
            ctx.eval::<(), _>(
                "var _offer_type = ''; \
                 new RTCPeerConnection().createOffer() \
                   .then(function(o) { _offer_type = o.type; });",
            )
            .unwrap();
        });
        // Drain Promise microtask queue so the .then callback runs.
        while rt.execute_pending_job().unwrap_or(false) {}
        ctx.with(|ctx| {
            let ty: String = ctx.eval("_offer_type").unwrap();
            assert_eq!(ty, "offer", "createOffer Promise must resolve with type='offer'");
        });
    }

    #[test]
    fn set_local_description_returns_thenable() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ty: String = ctx
                .eval(
                    "typeof new RTCPeerConnection() \
                       .setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})) \
                       .then",
                )
                .unwrap();
            assert_eq!(ty, "function", "setLocalDescription() must return a thenable");
        });
    }

    #[test]
    fn onicecandidate_fires_mdns_candidate() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            // synchronous setTimeout means _gatherMdns runs inline.
            let fired: bool = ctx
                .eval(
                    "(function() { \
                       var fired = false; \
                       var pc = new RTCPeerConnection(); \
                       pc.onicecandidate = function(e) { \
                         if (e.candidate && e.candidate.candidate) fired = true; \
                       }; \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return fired; \
                     })()",
                )
                .unwrap();
            assert!(fired, "onicecandidate must fire with a candidate object");
        });
    }

    #[test]
    fn mdns_candidate_ends_with_local() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            let cand: String = ctx
                .eval(
                    "(function() { \
                       var c = ''; \
                       var pc = new RTCPeerConnection(); \
                       pc.onicecandidate = function(e) { \
                         if (e.candidate && e.candidate.candidate) \
                           c = e.candidate.candidate; \
                       }; \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return c; \
                     })()",
                )
                .unwrap();
            assert!(
                cand.contains(".local"),
                "candidate must use a .local mDNS address, got: {cand}"
            );
        });
    }

    #[test]
    fn mdns_candidate_no_real_ip() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            let cand: String = ctx
                .eval(
                    "(function() { \
                       var c = ''; \
                       var pc = new RTCPeerConnection(); \
                       pc.onicecandidate = function(e) { \
                         if (e.candidate && e.candidate.candidate) \
                           c = e.candidate.candidate; \
                       }; \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return c; \
                     })()",
                )
                .unwrap();
            // Must not contain a bare IPv4 address (x.x.x.x).
            let has_ip = cand
                .split_whitespace()
                .any(|tok| tok.split('.').count() == 4 && tok.chars().all(|c| c.is_ascii_digit() || c == '.'));
            assert!(!has_ip, "candidate must not expose a real IP, got: {cand}");
        });
    }

    #[test]
    fn null_candidate_fires_at_end_of_gathering() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            let null_fired: bool = ctx
                .eval(
                    "(function() { \
                       var got_null = false; \
                       var pc = new RTCPeerConnection(); \
                       pc.onicecandidate = function(e) { \
                         if (e.candidate === null) got_null = true; \
                       }; \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return got_null; \
                     })()",
                )
                .unwrap();
            assert!(null_fired, "null candidate must fire to signal end-of-gathering");
        });
    }

    #[test]
    fn close_does_not_throw() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       try { new RTCPeerConnection().close(); return true; } \
                       catch(e) { return false; } \
                     })()",
                )
                .unwrap();
            assert!(ok, "close() must not throw");
        });
    }

    #[test]
    fn signaling_state_after_set_local_offer() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            let state: String = ctx
                .eval(
                    "(function() { \
                       var pc = new RTCPeerConnection(); \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return pc.signalingState; \
                     })()",
                )
                .unwrap();
            assert_eq!(state, "have-local-offer");
        });
    }

    #[test]
    fn add_event_listener_icecandidate() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_stubs(&ctx);
            install_webrtc_bindings(&ctx).unwrap();
            let fired: bool = ctx
                .eval(
                    "(function() { \
                       var fired = false; \
                       var pc = new RTCPeerConnection(); \
                       pc.addEventListener('icecandidate', function(e) { \
                         if (e.candidate && e.candidate.candidate) fired = true; \
                       }); \
                       pc.setLocalDescription(new RTCSessionDescription({type:'offer',sdp:'v=0'})); \
                       return fired; \
                     })()",
                )
                .unwrap();
            assert!(fired, "addEventListener('icecandidate') must also receive the candidate");
        });
    }

    #[test]
    fn feature_detection_pattern() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_webrtc_bindings(&ctx).unwrap();
            let supported: bool = ctx
                .eval("typeof RTCPeerConnection === 'function' && typeof RTCSessionDescription === 'function'")
                .unwrap();
            assert!(supported, "WebRTC feature detection must pass");
        });
    }
}
