//! WebRTC is deliberately not declared (COMPAT-API-01-WEBRTC, ADR-034 п.2).
//!
//! There is no ICE/DTLS/SRTP/SCTP stack, so the former mDNS-only
//! `RTCPeerConnection` stub answered offers with a fake SDP and left every
//! call hanging in `connecting` forever. A page feature-detecting
//! `typeof RTCPeerConnection` must see the truth and take its fallback path.
#![cfg(feature = "v8-backend")]

use lumen_core::JsRuntime as _;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;
use std::sync::{Arc, Mutex};

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "about:blank", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

#[test]
fn rtc_peer_connection_is_undefined() {
    let rt = make_rt();
    assert!(bool_eval(&rt, "typeof RTCPeerConnection === 'undefined'"));
    assert!(bool_eval(&rt, "!('RTCPeerConnection' in globalThis)"));
}

#[test]
fn webrtc_neighbours_are_not_declared() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        "['RTCSessionDescription', 'RTCIceCandidate', 'RTCDataChannel', 'RTCRtpSender', \
          'RTCRtpReceiver', 'RTCRtpTransceiver', 'RTCCertificate', 'webkitRTCPeerConnection'] \
         .every(n => !(n in globalThis))",
    ));
}
