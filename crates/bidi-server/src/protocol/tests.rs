use super::*;

mod commands;

fn parse(s: &str) -> JsonValue {
    parse_json(s).unwrap()
}

/// Создать сессию, вернуть id дефолтного контекста.
fn new_session_ctx(state: &mut BidiState) -> String {
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, state);
    // getTree, чтобы достать id дефолтного контекста.
    let r = dispatch(r#"{"id":2,"method":"browsingContext.getTree","params":{}}"#, state);
    let v = parse(&r.frames[0]);
    v.get("result")
        .unwrap()
        .get("contexts")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .get("context")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

/// A fake "live window" for SDC-2 tests: answers `AutomationCommand`s from
/// a background thread without any real winit/GPU state, so
/// `LiveWindowSession` round-trips exercise the exact same channel path a
/// real shell window uses.
///
/// `Wait` replies `Ack` immediately (simulates a page that is already
/// ready) — the real `document.readyState` polling loop lives in
/// `crates/shell`'s `check_wait_condition`, out of reach of this
/// bidi-server-only fake; the timing behavior it implements is exercised
/// by the P2-wpt S1 manual verification, not this unit test.
fn fake_live_session() -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::Navigate(_) => AutomationReply::Ack,
                AutomationCommand::Eval(js, _) => AutomationReply::Eval(format!("\"{js}\"")),
                AutomationCommand::Screenshot => AutomationReply::Screenshot(vec![0x89, b'P', b'N', b'G']),
                AutomationCommand::Click(_)
                | AutomationCommand::Type(_, _)
                | AutomationCommand::Scroll(_)
                | AutomationCommand::PointerMove { .. }
                | AutomationCommand::Key { .. }
                | AutomationCommand::TypeFocused(_) => {
                    AutomationReply::Ack
                }
                AutomationCommand::Wait(_, _) => AutomationReply::Ack,
                _ => AutomationReply::Error("unsupported in fake_live_session".into()),
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

/// Same as [`fake_live_session`] but `Wait` always times out (never sends
/// `Ack`/`Error` for the wait itself — the real shell only replies once
/// `check_wait_condition` returns `true` or its deadline passes; here we
/// simulate "never becomes ready" by answering with the same timeout
/// error the real shell sends past its deadline).
fn fake_live_session_load_never_completes() -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::Navigate(_) => AutomationReply::Ack,
                AutomationCommand::Wait(cond, _) => {
                    AutomationReply::Error(format!("wait timeout: {cond:?}"))
                }
                _ => AutomationReply::Error("unsupported in fake_live_session".into()),
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

/// Same as [`fake_live_session`] but `Wait` answers the way the real
/// shell does since BUG-438: the navigation settled in a network/HTTP
/// error (a `LoadEvent::LoadError`/final-render `Err` — unreachable
/// host, missing file, malformed URL), so `check_wait_condition`
/// resolved the `DocumentReady` wait immediately (BUG-308) but as a
/// failure, not a loaded document.
fn fake_live_session_load_settled_error() -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::Navigate(_) => AutomationReply::Ack,
                AutomationCommand::Wait(_, _) => {
                    AutomationReply::Error("navigation failed: connection refused".into())
                }
                _ => AutomationReply::Error("unsupported in fake_live_session".into()),
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

/// A fake "live window" that emulates V8's `awaitPromise` two-round-trip
/// dance for BUG-319: the round-1 setup eval (`Promise.resolve(...).then`)
/// is answered `"pending"`, and the round-2 read of
/// `globalThis.__lumen_bidi_await` returns `settled` — the canned JSON a
/// real page's settle handler would have left after the microtask
/// checkpoint (`{state:"fulfilled",value:..}` / `{state:"rejected",..}` /
/// `{state:"pending"}`).
fn fake_live_session_await(settled: &'static str) -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    let settled = settled.to_owned();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::Eval(js, _) if js.contains("Promise.resolve") => {
                    AutomationReply::Eval("\"pending\"".into())
                }
                AutomationCommand::Eval(..) => AutomationReply::Eval(settled.clone()),
                _ => AutomationReply::Ack,
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

/// A fake "live window" for BUG-295 remainder tests: `PollIntercepts`
/// reports exactly one paused request (`"r1"`) on its first call and
/// nothing thereafter (mirrors `lumen_network::drain_new_intercept_
/// announcements`'s at-most-once-per-request semantics);
/// `ResolveIntercept` matches only that same id.
fn fake_live_session_with_intercept(phase: &'static str) -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationHandle, AutomationReply, InterceptedRequest};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        let mut polled_once = false;
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::AddIntercept { .. } | AutomationCommand::RemoveIntercept(_) => {
                    AutomationReply::Ack
                }
                AutomationCommand::PollIntercepts => {
                    if polled_once {
                        AutomationReply::Intercepts(Vec::new())
                    } else {
                        polled_once = true;
                        AutomationReply::Intercepts(vec![InterceptedRequest {
                            request_id: "r1".into(),
                            url: "http://example.test/paused".into(),
                            phase: phase.into(),
                            status: (phase == "responseStarted").then_some(200),
                        }])
                    }
                }
                AutomationCommand::ResolveIntercept { request_id, .. } => {
                    AutomationReply::InterceptResolved(request_id == "r1")
                }
                _ => AutomationReply::Ack,
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

/// A live window whose cookie jar is a vector behind the channel, with one
/// `HttpOnly` cookie the BiDi session never set itself (as a `Set-Cookie`
/// response would leave it) — DEVX-20.
fn fake_live_session_with_jar() -> LiveWindowSession {
    use lumen_driver::{AutomationCommand, AutomationCookie, AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        let mut jar = vec![AutomationCookie {
            name: "server".into(),
            value: "from-server".into(),
            domain: "127.0.0.1".into(),
            path: "/".into(),
            secure: false,
            http_only: true,
            same_site: "default".into(),
            expiry: 0,
        }];
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::GetCookies => AutomationReply::Cookies(jar.clone()),
                AutomationCommand::SetCookie(c) => {
                    jar.retain(|j| !(j.name == c.name && j.domain == c.domain && j.path == c.path));
                    jar.push(c);
                    AutomationReply::Ack
                }
                AutomationCommand::DeleteCookie { domain, path, name } => {
                    jar.retain(|j| !(j.name == name && j.domain == domain && j.path == path));
                    AutomationReply::Ack
                }
                _ => AutomationReply::Ack,
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

#[test]
fn storage_with_live_window_reads_and_writes_the_real_jar() {
    let mut state = BidiState::with_live_session(fake_live_session_with_jar(), None);
    let got = dispatch(r#"{"id":1,"method":"storage.getCookies","params":{"filter":{"name":"server"}}}"#, &mut state);
    assert!(got.frames[0].contains("from-server"), "jar cookie not visible: {}", got.frames[0]);
    assert!(got.frames[0].contains(r#""httpOnly":true"#), "got: {}", got.frames[0]);

    // BytesValue form of `value`; lands in the jar, not just in session memory.
    let set = dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"sid","value":{"type":"string","value":"abc"},"domain":"127.0.0.1"}}}"#,
        &mut state,
    );
    assert!(set.frames[0].contains("success"), "got: {}", set.frames[0]);
    assert_eq!(state.cookie_count(), 0, "live mode keeps no shadow copy");
    let got = dispatch(r#"{"id":3,"method":"storage.getCookies","params":{}}"#, &mut state);
    assert!(got.frames[0].contains("abc") && got.frames[0].contains("from-server"), "got: {}", got.frames[0]);

    let del = dispatch(r#"{"id":4,"method":"storage.deleteCookies","params":{"filter":{"name":"sid"}}}"#, &mut state);
    assert!(del.frames[0].contains("success"), "got: {}", del.frames[0]);
    let got = dispatch(r#"{"id":5,"method":"storage.getCookies","params":{}}"#, &mut state);
    assert!(!got.frames[0].contains("abc"), "deleted cookie still listed: {}", got.frames[0]);
    assert!(got.frames[0].contains("from-server"), "unfiltered cookie lost: {}", got.frames[0]);
}

#[test]
fn storage_live_set_over_existing_cookie_emits_changed() {
    let mut state = BidiState::with_live_session(fake_live_session_with_jar(), None);
    dispatch(r#"{"id":1,"method":"session.subscribe","params":{"events":["storage.cookieChanged","storage.cookieAdded"]}}"#, &mut state);
    let set = dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"server","value":"x","domain":"127.0.0.1"}}}"#,
        &mut state,
    );
    assert!(set.frames.iter().any(|f| f.contains("storage.cookieChanged")), "got: {:?}", set.frames);
}

#[test]
fn storage_set_cookie_rejects_base64_value() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":{"type":"base64","value":"YQ=="},"domain":"x.test"}}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("invalid argument"), "got: {}", r.frames[0]);
}

#[test]
fn before_request_sent_event_delivered_when_subscribed() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("beforeRequestSent"), None);
    // Subscribing itself is a dispatch call, so it can already opportunistically
    // drain the pause (this transport has no async push — see `dispatch`'s doc
    // comment: the event rides along on whatever call happens to run next).
    let r = dispatch(
        r#"{"id":1,"method":"session.subscribe","params":{"events":["network.beforeRequestSent"]}}"#,
        &mut state,
    );
    assert_eq!(r.frames.len(), 2, "expected event frame + response frame: {:?}", r.frames);
    assert!(r.frames[0].contains("network.beforeRequestSent"), "got: {}", r.frames[0]);
    assert!(r.frames[0].contains("http://example.test/paused"), "got: {}", r.frames[0]);
    assert!(r.frames[0].contains("\"r1\""), "got: {}", r.frames[0]);
    assert!(r.frames[1].contains("success"), "got: {}", r.frames[1]);

    // Already-announced pause must not repeat on a later dispatch.
    let r2 = dispatch(r#"{"id":2,"method":"session.status","params":{}}"#, &mut state);
    assert_eq!(r2.frames.len(), 1, "must not re-announce: {:?}", r2.frames);
}

#[test]
fn before_request_sent_event_not_delivered_without_subscription() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("beforeRequestSent"), None);
    let r = dispatch(r#"{"id":1,"method":"session.status","params":{}}"#, &mut state);
    assert_eq!(r.frames.len(), 1, "no subscription — no event: {:?}", r.frames);
    assert!(!r.frames[0].contains("beforeRequestSent"));
}

#[test]
fn continue_request_with_live_window_resolves_matching_id() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("beforeRequestSent"), None);
    let r = dispatch(
        r#"{"id":1,"method":"network.continueRequest","params":{"request":"r1"}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn response_started_event_delivered_with_status_when_subscribed() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("responseStarted"), None);
    let r = dispatch(
        r#"{"id":1,"method":"session.subscribe","params":{"events":["network.responseStarted"]}}"#,
        &mut state,
    );
    assert_eq!(r.frames.len(), 2, "expected event frame + response frame: {:?}", r.frames);
    assert!(r.frames[0].contains("network.responseStarted"), "got: {}", r.frames[0]);
    assert!(r.frames[0].contains("\"isBlocked\":true"), "got: {}", r.frames[0]);
    assert!(r.frames[0].contains("\"status\":200"), "got: {}", r.frames[0]);
    assert!(r.frames[0].contains("\"r1\""), "got: {}", r.frames[0]);
}

#[test]
fn response_started_pause_not_reported_as_before_request_sent() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("responseStarted"), None);
    let r = dispatch(
        r#"{"id":1,"method":"session.subscribe","params":{"events":["network.beforeRequestSent"]}}"#,
        &mut state,
    );
    assert_eq!(r.frames.len(), 1, "responseStarted pause must not surface as beforeRequestSent: {:?}", r.frames);
}

#[test]
fn continue_response_with_live_window_resolves_matching_id() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("responseStarted"), None);
    let ok = dispatch(
        r#"{"id":1,"method":"network.continueResponse","params":{"request":"r1"}}"#,
        &mut state,
    );
    assert!(ok.frames[0].contains("success"), "got: {}", ok.frames[0]);
    let unknown = dispatch(
        r#"{"id":2,"method":"network.continueResponse","params":{"request":"nope"}}"#,
        &mut state,
    );
    assert!(unknown.frames[0].contains("no such request"), "got: {}", unknown.frames[0]);
}

#[test]
fn continue_response_without_live_window_is_no_such_request() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.continueResponse","params":{"request":"r1"}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("no such request"), "got: {}", r.frames[0]);
}

#[test]
fn continue_response_overrides_are_unsupported() {
    let mut state = BidiState::with_live_session(fake_live_session_with_intercept("responseStarted"), None);
    let r = dispatch(
        r#"{"id":1,"method":"network.continueResponse","params":{"request":"r1","statusCode":201}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("unsupported operation"), "got: {}", r.frames[0]);
}

#[test]
fn add_intercept_rejects_auth_required_and_unknown_phase() {
    let mut state = BidiState::new();
    let auth = dispatch(
        r#"{"id":1,"method":"network.addIntercept","params":{"phases":["authRequired"]}}"#,
        &mut state,
    );
    assert!(auth.frames[0].contains("unsupported operation"), "got: {}", auth.frames[0]);
    let bogus = dispatch(
        r#"{"id":2,"method":"network.addIntercept","params":{"phases":["nonsense"]}}"#,
        &mut state,
    );
    assert!(bogus.frames[0].contains("invalid argument"), "got: {}", bogus.frames[0]);
    assert_eq!(state.intercept_count(), 0, "a rejected rule must not be stored");
}

#[test]
fn continue_with_auth_is_no_such_request() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.continueWithAuth","params":{"request":"r1","action":"default"}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("no such request"), "got: {}", r.frames[0]);
}

#[test]
fn navigate_with_live_window_executes_real_navigate() {
    let mut state = BidiState::with_live_session(fake_live_session(), None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"https://example.com/"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
}

/// P2-wpt S1: `browsingContext.load`'s `timestamp` must be a real
/// (non-zero) Unix-ms clock reading, not the old hardcoded `0.0` emitted
/// unconditionally before the load signal existed.
#[test]
fn navigate_with_live_window_emits_load_with_real_timestamp() {
    let mut state = BidiState::with_live_session(fake_live_session(), None);
    let cid = new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.load"]}}"#,
        &mut state,
    );
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"https://example.com/"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.load"));
    let ts = ev.get("params").unwrap().get("timestamp").and_then(|x| x.as_number()).unwrap();
    assert!(ts > 1_700_000_000_000.0, "timestamp should be a real Unix-ms clock reading, got {ts}");
}

/// P2-wpt S1: when the live window never reaches
/// `document.readyState == "complete"` within the wait's deadline, the
/// navigate command must fail loudly (`unknown error`), not silently
/// report success with a fabricated load event.
#[test]
fn navigate_with_live_window_errors_when_load_never_completes() {
    let mut state = BidiState::with_live_session(fake_live_session_load_never_completes(), None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"https://example.com/"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("unknown error"));
}

/// BUG-438: a navigation that settles in a network/HTTP error (closed
/// port, missing file, malformed URL) must not be reported as a
/// successful `browsingContext.navigate` — the previous document is
/// still showing, so `{navigation, url}` would be a lie about what the
/// context is displaying. Before the fix, `check_wait_condition`'s
/// BUG-308 early-out made `wait{document_ready}` resolve with `Ack` for
/// exactly this case; it must now resolve with an error instead, the
/// same way a wait timeout already does.
#[test]
fn navigate_with_live_window_errors_when_load_settles_in_error() {
    let mut state = BidiState::with_live_session(fake_live_session_load_settled_error(), None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"http://127.0.0.1:1/nope.html"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("unknown error"));
    assert!(
        v.get("message").and_then(|x| x.as_str()).is_some_and(|m| m.contains("navigation failed")),
        "got: {}",
        r.frames[0]
    );
}

#[test]
fn script_evaluate_with_live_window_returns_real_result() {
    let mut state = BidiState::with_live_session(fake_live_session(), None);
    let cmd = r#"{"id":1,"method":"script.evaluate","params":{"expression":"1+1","awaitPromise":false}}"#;
    let r = dispatch(cmd, &mut state);
    let v = parse(&r.frames[0]);
    let result = v.get("result").unwrap().get("result").unwrap();
    assert_eq!(result.get("type").and_then(|x| x.as_str()), Some("string"));
    assert_eq!(result.get("value").and_then(|x| x.as_str()), Some("1+1"));
}

/// BUG-319: `awaitPromise:true` on a promise-valued expression must return
/// the promise's resolved `RemoteValue`, not the promise object.
#[test]
fn script_evaluate_await_promise_returns_resolved_value() {
    let mut state =
        BidiState::with_live_session(fake_live_session_await(r#"{"state":"fulfilled","value":42}"#), None);
    let cmd = r#"{"id":1,"method":"script.evaluate","params":{"expression":"Promise.resolve(42)","awaitPromise":true}}"#;
    let r = dispatch(cmd, &mut state);
    let v = parse(&r.frames[0]);
    let result = v.get("result").unwrap().get("result").unwrap();
    assert_eq!(result.get("type").and_then(|x| x.as_str()), Some("number"));
    assert_eq!(result.get("value").and_then(|x| x.as_number()), Some(42.0));
}

/// BUG-319: a promise fulfilled with `undefined` (JSON drops the `value`
/// key) maps to the `{type:"undefined"}` RemoteValue.
#[test]
fn script_evaluate_await_promise_undefined_fulfillment() {
    let mut state =
        BidiState::with_live_session(fake_live_session_await(r#"{"state":"fulfilled"}"#), None);
    let cmd = r#"{"id":1,"method":"script.evaluate","params":{"expression":"Promise.resolve()","awaitPromise":true}}"#;
    let r = dispatch(cmd, &mut state);
    let v = parse(&r.frames[0]);
    let result = v.get("result").unwrap().get("result").unwrap();
    assert_eq!(result.get("type").and_then(|x| x.as_str()), Some("undefined"));
}

/// BUG-319: a rejected promise under `awaitPromise:true` surfaces as a
/// `javascript error`, not a success frame carrying the promise object.
#[test]
fn script_evaluate_await_promise_rejection_is_error() {
    let mut state = BidiState::with_live_session(fake_live_session_await(
        r#"{"state":"rejected","error":"boom"}"#,
    ), None);
    let cmd = r#"{"id":1,"method":"script.evaluate","params":{"expression":"Promise.reject(new Error('boom'))","awaitPromise":true}}"#;
    let r = dispatch(cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("javascript error"));
    assert!(v.get("message").and_then(|x| x.as_str()).unwrap_or("").contains("boom"));
}

/// BUG-319: a promise still pending after the microtask checkpoint (needs a
/// macrotask/IO) falls back to the promise object, same as the non-awaited
/// path — no regression, no fabricated value.
#[test]
fn script_evaluate_await_promise_pending_falls_back_to_promise_object() {
    let mut state =
        BidiState::with_live_session(fake_live_session_await(r#"{"state":"pending"}"#), None);
    let cmd = r#"{"id":1,"method":"script.evaluate","params":{"expression":"new Promise(function(){})","awaitPromise":true}}"#;
    let r = dispatch(cmd, &mut state);
    let v = parse(&r.frames[0]);
    let result = v.get("result").unwrap().get("result").unwrap();
    assert_eq!(result.get("type").and_then(|x| x.as_str()), Some("string"));
    assert_eq!(result.get("value").and_then(|x| x.as_str()), Some("{}"));
}

#[test]
fn capture_screenshot_with_live_window_returns_base64_data() {
    let mut state = BidiState::with_live_session(fake_live_session(), None);
    let r = dispatch(r#"{"id":1,"method":"browsingContext.captureScreenshot","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert!(!v.get("result").unwrap().get("data").unwrap().as_str().unwrap().is_empty());
}

#[test]
fn capture_screenshot_without_live_window_errors() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":1,"method":"browsingContext.captureScreenshot","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("unknown error"));
}

// ── ADR-024 §Access model (DEVX-15): token auth ──

#[test]
fn session_new_rejects_missing_token_when_required() {
    let mut state = BidiState::with_live_session(fake_live_session(), Some("secret".into()));
    let r = dispatch(
        r#"{"id":1,"method":"session.new","params":{"capabilities":{}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert!(state.session_id.is_none(), "no session must be created on auth failure");
}

#[test]
fn session_new_rejects_wrong_token_when_required() {
    let mut state = BidiState::with_live_session(fake_live_session(), Some("secret".into()));
    let r = dispatch(
        r#"{"id":1,"method":"session.new","params":{"capabilities":{"alwaysMatch":{"token":"wrong"}}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert!(state.session_id.is_none());
}

#[test]
fn session_new_accepts_correct_token_when_required() {
    let mut state = BidiState::with_live_session(fake_live_session(), Some("secret".into()));
    let r = dispatch(
        r#"{"id":1,"method":"session.new","params":{"capabilities":{"alwaysMatch":{"token":"secret"}}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert!(state.session_id.is_some());
}

#[test]
fn session_new_without_required_token_ignores_capabilities() {
    // BidiState::new() (test ctor) never sets a required token.
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"session.new","params":{"capabilities":{}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
}

#[test]
fn input_perform_actions_with_live_window_clicks() {
    let mut state = BidiState::with_live_session(fake_live_session(), None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.performActions","params":{{"context":"{cid}","actions":[
                {{"type":"pointer","id":"m","actions":[
                    {{"type":"pointerMove","x":10,"y":20}},
                    {{"type":"pointerDown","button":0}}
                ]}}
            ]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

/// Fake live window that records every `AutomationCommand` it receives.
fn recording_live_session() -> (LiveWindowSession, std::sync::mpsc::Receiver<String>) {
    use lumen_driver::{AutomationHandle, AutomationReply};
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    let (log_tx, log_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let _ = log_tx.send(format!("{cmd:?}"));
            let _ = reply_tx.send(AutomationReply::Ack);
        }
    });
    (LiveWindowSession::new(AutomationHandle::new(tx)), log_rx)
}

/// BUG-1194: `pointerMove` without a click moves the cursor, `` is an
/// Escape key press (not typed text), and a bare key chain never clicks.
#[test]
fn input_perform_actions_hover_and_special_keys() {
    let (live, log) = recording_live_session();
    let mut state = BidiState::with_live_session(live, None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.performActions","params":{{"context":"{cid}","actions":[
                {{"type":"pointer","id":"m","actions":[{{"type":"pointerMove","x":10,"y":20}}]}},
                {{"type":"key","id":"k","actions":[
                    {{"type":"keyDown","value":""}},{{"type":"keyUp","value":""}}
                ]}}
            ]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
    let seen: Vec<String> = log.try_iter().collect();
    assert!(seen.iter().any(|c| c.contains("PointerMove") && c.contains("10.0")), "{seen:?}");
    assert!(seen.iter().any(|c| c.contains("Key") && c.contains("Escape") && c.contains("down: true")), "{seen:?}");
    assert!(seen.iter().any(|c| c.contains("Key") && c.contains("down: false")), "{seen:?}");
    assert!(!seen.iter().any(|c| c.starts_with("Click") || c.starts_with("Type(")), "{seen:?}");
}

/// BUG-1194: ordinary characters without a preceding pointer action go to
/// the focused element; after a pointer action they are click-then-type.
#[test]
fn input_perform_actions_text_without_pointer_does_not_click() {
    let (live, log) = recording_live_session();
    let mut state = BidiState::with_live_session(live, None);
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.performActions","params":{{"context":"{cid}","actions":[
                {{"type":"key","id":"k","actions":[{{"type":"keyDown","value":"a"}},{{"type":"keyDown","value":"b"}}]}}
            ]}}}}"#
    );
    dispatch(&cmd, &mut state);
    let seen: Vec<String> = log.try_iter().collect();
    assert_eq!(seen, vec![r#"TypeFocused("ab")"#.to_owned()]);
}

#[test]
fn session_status_ready_before_session() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":1,"method":"session.status","params":{}}"#, &mut state);
    assert_eq!(r.frames.len(), 1);
    assert!(!r.close);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(v.get("id").and_then(|x| x.as_number()), Some(1.0));
    let result = v.get("result").unwrap();
    assert_eq!(result.get("ready").and_then(|x| x.as_bool()), Some(true));
}

#[test]
fn session_new_returns_session_without_event() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":2,"method":"session.new","params":{"capabilities":{}}}"#,
        &mut state,
    );
    // Только success — created-событие не эмитится (нет подписки).
    assert_eq!(r.frames.len(), 1);
    assert!(!r.close);

    let success = parse(&r.frames[0]);
    assert_eq!(success.get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(success.get("id").and_then(|x| x.as_number()), Some(2.0));
    let result = success.get("result").unwrap();
    assert!(result.get("sessionId").and_then(|x| x.as_str()).is_some());
    let caps = result.get("capabilities").unwrap();
    assert_eq!(caps.get("browserName").and_then(|x| x.as_str()), Some("Lumen"));
}

#[test]
fn session_status_not_ready_after_session_new() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let r = dispatch(r#"{"id":2,"method":"session.status","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let result = v.get("result").unwrap();
    assert_eq!(result.get("ready").and_then(|x| x.as_bool()), Some(false));
}

#[test]
fn second_session_new_errors() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let r = dispatch(r#"{"id":2,"method":"session.new","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("session not created"));
}

#[test]
fn get_tree_empty_before_session() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":1,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let contexts = v.get("result").unwrap().get("contexts").unwrap();
    assert_eq!(contexts.as_array().unwrap().len(), 0);
}

#[test]
fn get_tree_returns_default_context_after_session() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let r = dispatch(r#"{"id":2,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let contexts = v.get("result").unwrap().get("contexts").unwrap();
    let arr = contexts.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].get("url").and_then(|x| x.as_str()), Some("about:blank"));
    // getTree даёт children как массив (не null).
    assert!(matches!(arr[0].get("children"), Some(JsonValue::Array(_))));
}

#[test]
fn subscribe_acks_empty_result() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":3,"method":"session.subscribe","params":{"events":["browsingContext.load"]}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert!(v.get("result").unwrap().as_object().unwrap().is_empty());
}

#[test]
fn session_end_acks_and_closes() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":9,"method":"session.end","params":{}}"#, &mut state);
    assert!(r.close);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
}

#[test]
fn unknown_method_returns_unknown_command() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":4,"method":"foo.bar","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("unknown command"));
    assert_eq!(v.get("id").and_then(|x| x.as_number()), Some(4.0));
}

#[test]
fn invalid_json_returns_error_with_null_id() {
    let mut state = BidiState::new();
    let r = dispatch("not json", &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert!(matches!(v.get("id"), Some(JsonValue::Null)));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn missing_id_returns_invalid_argument() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"method":"session.status","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

// --- multi-context + subscriptions ---

#[test]
fn create_context_without_subscription_emits_no_event() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    assert_eq!(r.frames.len(), 1); // только success
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert!(v.get("result").unwrap().get("context").and_then(|x| x.as_str()).is_some());
}

#[test]
fn create_context_with_subscription_emits_created() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.created"]}}"#,
        &mut state,
    );
    let r = dispatch(r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    assert_eq!(r.frames.len(), 2); // success + event
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("type").and_then(|x| x.as_str()), Some("event"));
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.created"));
    // created-событие даёт children:null.
    assert!(matches!(ev.get("params").unwrap().get("children"), Some(JsonValue::Null)));
}

#[test]
fn module_subscription_matches_event() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    // Подписка на весь модуль browsingContext.
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext"]}}"#,
        &mut state,
    );
    let r = dispatch(r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    assert_eq!(r.frames.len(), 2);
}

#[test]
fn unsubscribe_stops_events() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.created"]}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":11,"method":"session.unsubscribe","params":{"events":["browsingContext.created"]}}"#,
        &mut state,
    );
    let r = dispatch(r#"{"id":12,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    assert_eq!(r.frames.len(), 1); // событие больше не приходит
}

#[test]
fn create_with_bad_reference_context_errors() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab","referenceContext":"nope"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn create_without_session_errors() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("session not created"));
}

#[test]
fn child_context_nested_in_get_tree() {
    let mut state = BidiState::new();
    let root = new_session_ctx(&mut state);
    let create_cmd = format!(
        r#"{{"id":10,"method":"browsingContext.create","params":{{"type":"tab","referenceContext":"{root}"}}}}"#
    );
    dispatch(&create_cmd, &mut state);
    let r = dispatch(r#"{"id":11,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let arr = v.get("result").unwrap().get("contexts").unwrap().as_array().unwrap();
    // top-level — только root.
    assert_eq!(arr.len(), 1);
    let children = arr[0].get("children").unwrap().as_array().unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get("parent").and_then(|x| x.as_str()), Some(root.as_str()));
}

#[test]
fn navigate_updates_url_and_returns_navigation() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"https://example.com/"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    let result = v.get("result").unwrap();
    assert_eq!(result.get("url").and_then(|x| x.as_str()), Some("https://example.com/"));
    assert!(result.get("navigation").and_then(|x| x.as_str()).is_some());

    // getTree отражает новый URL.
    let tr = dispatch(r#"{"id":11,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let tv = parse(&tr.frames[0]);
    let url = tv.get("result").unwrap().get("contexts").unwrap().as_array().unwrap()[0]
        .get("url")
        .and_then(|x| x.as_str());
    assert_eq!(url, Some("https://example.com/"));
}

#[test]
fn navigate_emits_load_when_subscribed() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.load"]}}"#,
        &mut state,
    );
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}","url":"https://x.test/"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 2);
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.load"));
    assert_eq!(ev.get("params").unwrap().get("url").and_then(|x| x.as_str()), Some("https://x.test/"));
}

#[test]
fn navigate_missing_url_errors() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.navigate","params":{{"context":"{cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn navigate_unknown_context_errors() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":10,"method":"browsingContext.navigate","params":{"context":"nope","url":"https://a/"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn close_removes_context() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let cr = dispatch(r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#, &mut state);
    let new_cid = parse(&cr.frames[0]).get("result").unwrap().get("context").unwrap().as_str().unwrap().to_owned();

    let cmd = format!(
        r#"{{"id":11,"method":"browsingContext.close","params":{{"context":"{new_cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 1);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));

    // getTree больше не содержит закрытый контекст (остаётся только default).
    let tr = dispatch(r#"{"id":12,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let tv = parse(&tr.frames[0]);
    let arr = tv.get("result").unwrap().get("contexts").unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 1);
}

#[test]
fn close_emits_destroyed_when_subscribed() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.contextDestroyed"]}}"#,
        &mut state,
    );
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.close","params":{{"context":"{cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 2);
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.contextDestroyed"));
    assert_eq!(ev.get("params").unwrap().get("context").and_then(|x| x.as_str()), Some(cid.as_str()));
}

#[test]
fn close_cascades_to_children() {
    let mut state = BidiState::new();
    let root = new_session_ctx(&mut state);
    let create_cmd = format!(
        r#"{{"id":10,"method":"browsingContext.create","params":{{"type":"tab","referenceContext":"{root}"}}}}"#
    );
    dispatch(&create_cmd, &mut state);
    // Закрываем root → потомок тоже исчезает.
    let close_cmd = format!(
        r#"{{"id":11,"method":"browsingContext.close","params":{{"context":"{root}"}}}}"#
    );
    dispatch(&close_cmd, &mut state);
    let tr = dispatch(r#"{"id":12,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let tv = parse(&tr.frames[0]);
    let arr = tv.get("result").unwrap().get("contexts").unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 0);
}

#[test]
fn close_missing_context_errors() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(r#"{"id":10,"method":"browsingContext.close","params":{"context":"nope"}}"#, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn activate_validates_context() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":10,"method":"browsingContext.activate","params":{{"context":"{cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));

    let bad = dispatch(r#"{"id":11,"method":"browsingContext.activate","params":{"context":"nope"}}"#, &mut state);
    assert_eq!(parse(&bad.frames[0]).get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn script_evaluate_unknown_context_returns_error() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":1,"method":"script.evaluate","params":{"expression":"1+1","target":{"context":"bad-ctx"},"awaitPromise":false}}"#,
        &mut state,
    );
    assert!(!result.close);
    assert!(result.frames[0].contains("no such frame"), "got: {}", result.frames[0]);
}

#[test]
fn script_evaluate_no_context_returns_stub() {
    let mut state = BidiState::new();
    // No context given — should return stub without error.
    let result = dispatch(
        r#"{"id":2,"method":"script.evaluate","params":{"expression":"1+1","awaitPromise":false}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("undefined"), "got: {}", result.frames[0]);
}

#[test]
fn script_add_preload_returns_script_id() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":3,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}"}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("script"), "got: {}", result.frames[0]);
}

#[test]
fn script_remove_preload_acks() {
    let mut state = BidiState::new();
    // Add a preload script first to get a valid ID.
    let add_result = dispatch(
        r#"{"id":3,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}"}}"#,
        &mut state,
    );
    let script_id = parse(&add_result.frames[0])
        .get("result")
        .unwrap()
        .get("script")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let cmd = format!(
        r#"{{"id":4,"method":"script.removePreloadScript","params":{{"script":"{script_id}"}}}}"#
    );
    let result = dispatch(&cmd, &mut state);
    assert!(result.frames[0].contains("success"), "got: {}", result.frames[0]);
    assert_eq!(state.preload_script_count(), 0);
}
