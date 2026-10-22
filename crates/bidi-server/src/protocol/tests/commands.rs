//! Тесты команд network.*, emulation, script.disown/getRealms, input.*,
//! userPrompt, preload, загрузок, storage (cookie) и setViewport.

use super::*;

// --- network.* tests ---

#[test]
fn get_response_body_returns_base64() {
    let mut state = BidiState::new();
    state.record_response_body(42, b"hello".to_vec());
    let r = dispatch(
        r#"{"id":1,"method":"network.getResponseBody","params":{"request":{"requestId":42}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    let body = v.get("result").unwrap().get("body").unwrap();
    assert_eq!(body.get("type").and_then(|x| x.as_str()), Some("base64"));
    // "hello" in base64 is "aGVsbG8="
    assert_eq!(body.get("value").and_then(|x| x.as_str()), Some("aGVsbG8="));
}

#[test]
fn get_response_body_missing_request_id_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":2,"method":"network.getResponseBody","params":{"request":{}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn get_response_body_unknown_request_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":3,"method":"network.getResponseBody","params":{"request":{"requestId":999}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such request"));
}

#[test]
fn record_response_body_emits_event_when_subscribed() {
    let mut state = BidiState::new();
    dispatch(
        r#"{"id":1,"method":"session.subscribe","params":{"events":["network.responseBodyReceived"]}}"#,
        &mut state,
    );
    let frames = state.record_response_body(7, b"data".to_vec());
    assert_eq!(frames.len(), 1);
    let ev = parse(&frames[0]);
    assert_eq!(ev.get("type").and_then(|x| x.as_str()), Some("event"));
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("network.responseBodyReceived"));
    let params = ev.get("params").unwrap();
    assert_eq!(params.get("request").unwrap().get("requestId").and_then(|x| x.as_number()), Some(7.0));
}

#[test]
fn record_response_body_no_event_when_not_subscribed() {
    let mut state = BidiState::new();
    let frames = state.record_response_body(5, b"body".to_vec());
    assert!(frames.is_empty());
    // Body is still buffered — can be retrieved via command.
    let r = dispatch(
        r#"{"id":1,"method":"network.getResponseBody","params":{"request":{"requestId":5}}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn second_record_overwrites_first() {
    let mut state = BidiState::new();
    state.record_response_body(10, b"first".to_vec());
    state.record_response_body(10, b"second".to_vec());
    let r = dispatch(
        r#"{"id":1,"method":"network.getResponseBody","params":{"request":{"requestId":10}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    let val = v.get("result").unwrap().get("body").unwrap().get("value").and_then(|x| x.as_str()).unwrap();
    // "second" in base64 is "c2Vjb25k"
    assert_eq!(val, "c2Vjb25k");
}

// --- G-2: locale / timezone / offline / UA override tests ---

#[test]
fn set_locale_stores_and_returns_success() {
    let mut state = BidiState::new();
    assert!(state.locale().is_none());
    let r = dispatch(
        r#"{"id":1,"method":"session.setDefaultUserContextLocale","params":{"locale":"ru-RU"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.locale(), Some("ru-RU"));
    // Overwrite with a new locale.
    dispatch(
        r#"{"id":2,"method":"session.setDefaultUserContextLocale","params":{"locale":"en-US"}}"#,
        &mut state,
    );
    assert_eq!(state.locale(), Some("en-US"));
}

#[test]
fn set_timezone_stores_and_returns_success() {
    let mut state = BidiState::new();
    assert!(state.timezone().is_none());
    let r = dispatch(
        r#"{"id":1,"method":"browser.setTimezoneOverride","params":{"timezoneId":"Europe/Moscow"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.timezone(), Some("Europe/Moscow"));
}

#[test]
fn set_permission_validates_and_stores() {
    let mut state = BidiState::new();
    let ok = dispatch(
        r#"{"id":1,"method":"permissions.setPermission","params":{"descriptor":{"name":"geolocation"},"state":"granted","origin":"http://a.test"}}"#,
        &mut state,
    );
    assert_eq!(parse(&ok.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.permission_state("geolocation"), Some("granted"));
    let bad = dispatch(
        r#"{"id":2,"method":"permissions.setPermission","params":{"descriptor":{"name":"geolocation"},"state":"maybe","origin":"http://a.test"}}"#,
        &mut state,
    );
    assert_eq!(parse(&bad.frames[0]).get("error").and_then(|x| x.as_str()), Some("invalid argument"));
    assert_eq!(state.permission_state("geolocation"), Some("granted"));
}

#[test]
fn network_offline_status_toggled() {
    let mut state = BidiState::new();
    assert!(!state.is_offline());
    // Включить offline через {"status":{"offline":true}}.
    let r = dispatch(
        r#"{"id":1,"method":"network.setOfflineStatus","params":{"status":{"offline":true}}}"#,
        &mut state,
    );
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert!(state.is_offline());
    // Отключить через упрощённую форму {"offline":false}.
    dispatch(
        r#"{"id":2,"method":"network.setOfflineStatus","params":{"offline":false}}"#,
        &mut state,
    );
    assert!(!state.is_offline());
}

#[test]
fn ua_override_session_and_per_context() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);

    // До установки — нет UA для контекста.
    assert!(state.user_agent_for(&cid).is_none());

    // Сессионный UA.
    dispatch(
        r#"{"id":1,"method":"emulation.setUserAgentOverride","params":{"userAgent":"Lumen/Test"}}"#,
        &mut state,
    );
    assert_eq!(state.user_agent_for(&cid), Some("Lumen/Test"));

    // Per-context UA переопределяет сессионный.
    let cmd = format!(
        r#"{{"id":2,"method":"emulation.setUserAgentOverride","params":{{"userAgent":"CtxUA","contexts":["{cid}"]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.user_agent_for(&cid), Some("CtxUA"));

    // Несуществующий контекст — ошибка.
    let bad = dispatch(
        r#"{"id":3,"method":"emulation.setUserAgentOverride","params":{"userAgent":"X","contexts":["bad-id"]}}"#,
        &mut state,
    );
    assert_eq!(parse(&bad.frames[0]).get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

// --- script.disown / script.getRealms ---

#[test]
fn script_disown_acks() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":1,"method":"script.disown","params":{"handles":["h1"],"target":{"context":"c1"}}}"#,
        &mut state,
    );
    assert!(result.frames[0].contains("success"), "got: {}", result.frames[0]);
}

#[test]
fn script_get_realms_returns_empty_array() {
    let mut state = BidiState::new();
    let result = dispatch(
        r#"{"id":2,"method":"script.getRealms","params":{}}"#,
        &mut state,
    );
    let v = parse(&result.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    let realms = v.get("result").unwrap().get("realms").unwrap();
    assert_eq!(realms.as_array().unwrap().len(), 0);
}

// --- network.addIntercept / removeIntercept ---

#[test]
fn network_add_intercept_returns_id() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.addIntercept","params":{"phases":["beforeRequestSent"]}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    let intercept_id = v.get("result").unwrap().get("intercept").and_then(|x| x.as_str());
    assert!(intercept_id.is_some(), "expected intercept id");
    assert_eq!(state.intercept_count(), 1);
}

#[test]
fn network_add_intercept_empty_phases_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.addIntercept","params":{"phases":[]}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("error"));
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn network_remove_intercept_removes_rule() {
    let mut state = BidiState::new();
    let add = dispatch(
        r#"{"id":1,"method":"network.addIntercept","params":{"phases":["responseStarted"]}}"#,
        &mut state,
    );
    let intercept_id = parse(&add.frames[0])
        .get("result").unwrap()
        .get("intercept").unwrap()
        .as_str().unwrap()
        .to_owned();
    assert_eq!(state.intercept_count(), 1);

    let cmd = format!(
        r#"{{"id":2,"method":"network.removeIntercept","params":{{"intercept":"{intercept_id}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.intercept_count(), 0);
}

#[test]
fn network_remove_intercept_unknown_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.removeIntercept","params":{"intercept":"nope"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such intercept"));
}

#[test]
fn network_set_cache_behavior_stores_value() {
    let mut state = BidiState::new();
    assert!(state.cache_behavior().is_none());
    let r = dispatch(
        r#"{"id":1,"method":"network.setCacheBehavior","params":{"cacheBehavior":"bypass"}}"#,
        &mut state,
    );
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.cache_behavior(), Some("bypass"));
}

#[test]
fn network_continue_request_acks() {
    // BiDi §12.6.13: `request` is the opaque request id string itself,
    // not an object — matches `verify_devx6_bidi_scenarios.py`'s shape.
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.continueRequest","params":{"request":"nonexistent-request-id"}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn network_fail_request_acks() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"network.failRequest","params":{"request":"nonexistent-request-id"}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn network_continue_request_missing_request_id_errors() {
    let mut state = BidiState::new();
    let r = dispatch(r#"{"id":1,"method":"network.continueRequest","params":{}}"#, &mut state);
    assert!(r.frames[0].contains("invalid argument"), "got: {}", r.frames[0]);
}

// --- input.* tests ---

#[test]
fn input_perform_actions_acks() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.performActions","params":{{"context":"{cid}","actions":[{{"type":"key","id":"k","actions":[]}}]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn input_perform_actions_unknown_context_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"input.performActions","params":{"context":"nope","actions":[]}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn input_perform_actions_bad_actions_type_errors() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.performActions","params":{{"context":"{cid}","actions":"bad"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn input_release_actions_acks() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.releaseActions","params":{{"context":"{cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

#[test]
fn input_release_actions_unknown_context_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"input.releaseActions","params":{"context":"nope"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn input_set_files_acks() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"input.setFiles","params":{{"context":"{cid}","element":{{"sharedId":"el1"}},"files":["/tmp/test.txt"]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
}

// ── userPrompt (browsingContext.handleUserPrompt + fire_user_prompt) ──

#[test]
fn handle_user_prompt_missing_context_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"browsingContext.handleUserPrompt","params":{}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn handle_user_prompt_no_open_prompt_errors() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.handleUserPrompt","params":{{"context":"{cid}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such alert"));
}

#[test]
fn fire_and_handle_user_prompt_emits_events() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    // Subscribe to both events.
    dispatch(
        r#"{"id":1,"method":"session.subscribe","params":{"events":["browsingContext.userPromptOpened","browsingContext.userPromptClosed"]}}"#,
        &mut state,
    );

    // Fire a prompt.
    let (_, open_frames) =
        state.fire_user_prompt(&cid, "alert", "Are you sure?", "");
    assert_eq!(open_frames.len(), 1);
    let ev = parse(&open_frames[0]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.userPromptOpened"));
    let p = ev.get("params").unwrap();
    assert_eq!(p.get("type").and_then(|x| x.as_str()), Some("alert"));
    assert_eq!(open_prompt_count(&state), 1);

    // Handle (dismiss) the prompt.
    let cmd = format!(
        r#"{{"id":2,"method":"browsingContext.handleUserPrompt","params":{{"context":"{cid}","accept":false}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 2); // success + event
    let closed_ev = parse(&r.frames[1]);
    assert_eq!(
        closed_ev.get("method").and_then(|x| x.as_str()),
        Some("browsingContext.userPromptClosed")
    );
    let cp = closed_ev.get("params").unwrap();
    assert_eq!(cp.get("accepted").and_then(|x| x.as_bool()), Some(false));
    assert_eq!(open_prompt_count(&state), 0);
}

fn open_prompt_count(state: &BidiState) -> usize {
    state.open_prompt_count()
}

// ── preload per-context ──

#[test]
fn add_preload_script_stores_globally() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":1,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>1"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("type").and_then(|x| x.as_str()), Some("success"));
    assert!(v.get("result").unwrap().get("script").is_some());
    assert_eq!(state.preload_script_count(), 1);
}

#[test]
fn add_preload_script_with_contexts_stores_per_context() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"script.addPreloadScript","params":{{"functionDeclaration":"()=>1","contexts":["{cid}"]}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"));
    assert_eq!(state.preload_script_count(), 1);
    let scripts = state.preload_scripts_for_context(&cid);
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0], "()=>1");
    // Global (no-context) script should not appear for this context check with context filter.
    let global_count = state.preload_scripts_for_context("other-context").len();
    assert_eq!(global_count, 0, "per-context script must not appear for other contexts");
}

#[test]
fn add_preload_unknown_context_errors() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":1,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}","contexts":["bad-id"]}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn remove_preload_script_decrements_count() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":1,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}"}}"#,
        &mut state,
    );
    let script_id = parse(&r.frames[0])
        .get("result")
        .unwrap()
        .get("script")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(state.preload_script_count(), 1);

    let cmd = format!(
        r#"{{"id":2,"method":"script.removePreloadScript","params":{{"script":"{script_id}"}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert!(r.frames[0].contains("success"));
    assert_eq!(state.preload_script_count(), 0);
}

#[test]
fn remove_preload_unknown_id_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"script.removePreloadScript","params":{"script":"nope"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such script"));
}

// ── download lifecycle ──

#[test]
fn begin_download_emits_event_when_subscribed() {
    let mut state = BidiState::new();
    dispatch(
        r#"{"id":1,"method":"session.new","params":{}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":2,"method":"session.subscribe","params":{"events":["browser.downloadWillBegin"]}}"#,
        &mut state,
    );
    let (dl_id, frames) = state.begin_download("https://example.com/file.zip".into(), "file.zip".into());
    assert!(!dl_id.is_empty());
    assert_eq!(frames.len(), 1);
    let ev = parse(&frames[0]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browser.downloadWillBegin"));
    assert_eq!(state.download_count(), 1);
}

#[test]
fn complete_download_emits_completed_event() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"session.subscribe","params":{"events":["browser"]}}"#,
        &mut state,
    );
    let (dl_id, _) = state.begin_download("https://x.com/a.tar.gz".into(), "a.tar.gz".into());
    let frames = state.complete_download(&dl_id);
    assert_eq!(frames.len(), 1);
    let ev = parse(&frames[0]);
    assert_eq!(
        ev.get("method").and_then(|x| x.as_str()),
        Some("browser.downloadItemCompleted")
    );
    let p = ev.get("params").unwrap();
    assert_eq!(p.get("state").and_then(|x| x.as_str()), Some("completed"));
}

#[test]
fn abort_download_emits_aborted_event() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"session.subscribe","params":{"events":["browser"]}}"#,
        &mut state,
    );
    let (dl_id, _) = state.begin_download("https://x.com/b.exe".into(), "b.exe".into());
    let frames = state.abort_download(&dl_id);
    assert_eq!(frames.len(), 1);
    let ev = parse(&frames[0]);
    assert_eq!(
        ev.get("method").and_then(|x| x.as_str()),
        Some("browser.downloadItemAborted")
    );
}

// ── storage (cookie) commands ──

#[test]
fn set_and_get_cookie() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let r = dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"sid","value":"abc","domain":"example.com"}}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"));
    assert_eq!(state.cookie_count(), 1);

    let r2 = dispatch(
        r#"{"id":3,"method":"storage.getCookies","params":{"filter":{"domain":"example.com"}}}"#,
        &mut state,
    );
    let v = parse(&r2.frames[0]);
    let cookies = v.get("result").unwrap().get("cookies").unwrap().as_array().unwrap();
    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].get("name").and_then(|x| x.as_str()), Some("sid"));
}

#[test]
fn delete_cookies_by_domain() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":"1","domain":"example.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":3,"method":"storage.setCookie","params":{"cookie":{"name":"b","value":"2","domain":"other.com"}}}"#,
        &mut state,
    );
    assert_eq!(state.cookie_count(), 2);

    let r = dispatch(
        r#"{"id":4,"method":"storage.deleteCookies","params":{"filter":{"domain":"example.com"}}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"));
    assert_eq!(state.cookie_count(), 1);
}

#[test]
fn cookie_change_event_emitted_when_subscribed() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"session.subscribe","params":{"events":["storage.cookieAdded"]}}"#,
        &mut state,
    );
    let cookie = BidiCookie {
        name: "x".into(),
        value: "1".into(),
        domain: "foo.com".into(),
        path: "/".into(),
        secure: false,
        http_only: false,
        same_site: "lax".into(),
        expiry: 0,
    };
    let frames = state.record_cookie_change("added", cookie);
    assert_eq!(frames.len(), 1);
    let ev = parse(&frames[0]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("storage.cookieAdded"));
    assert_eq!(state.cookie_count(), 1);
}

// ── browsingContext.setViewport (viewport-before-popup) ──

#[test]
fn set_viewport_stores_dimensions() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    assert!(state.viewport_for(&cid).is_none());

    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.setViewport","params":{{"context":"{cid}","viewport":{{"width":1280,"height":720}}}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(parse(&r.frames[0]).get("type").and_then(|x| x.as_str()), Some("success"));
    assert_eq!(state.viewport_for(&cid), Some((1280, 720)));
}

#[test]
fn set_viewport_emits_event_when_subscribed() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    dispatch(
        r#"{"id":9,"method":"session.subscribe","params":{"events":["browsingContext.viewportChanged"]}}"#,
        &mut state,
    );
    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.setViewport","params":{{"context":"{cid}","viewport":{{"width":800,"height":600}}}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 2); // success + event
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("type").and_then(|x| x.as_str()), Some("event"));
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("browsingContext.viewportChanged"));
    let p = ev.get("params").unwrap();
    assert_eq!(p.get("context").and_then(|x| x.as_str()), Some(cid.as_str()));
    let vp = p.get("viewport").unwrap();
    assert_eq!(vp.get("width").and_then(|x| x.as_number()), Some(800.0));
    assert_eq!(vp.get("height").and_then(|x| x.as_number()), Some(600.0));
}

#[test]
fn set_viewport_no_event_without_subscription() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.setViewport","params":{{"context":"{cid}","viewport":{{"width":1024,"height":768}}}}}}"#
    );
    let r = dispatch(&cmd, &mut state);
    assert_eq!(r.frames.len(), 1); // only success, no event
}

#[test]
fn set_viewport_unknown_context_errors() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":1,"method":"browsingContext.setViewport","params":{"context":"nope","viewport":{"width":800,"height":600}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("no such frame"));
}

#[test]
fn set_viewport_missing_context_errors() {
    let mut state = BidiState::new();
    let r = dispatch(
        r#"{"id":1,"method":"browsingContext.setViewport","params":{"viewport":{"width":800,"height":600}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    assert_eq!(v.get("error").and_then(|x| x.as_str()), Some("invalid argument"));
}

#[test]
fn set_viewport_null_clears_dimensions() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    // Set a viewport first.
    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.setViewport","params":{{"context":"{cid}","viewport":{{"width":640,"height":480}}}}}}"#
    );
    dispatch(&cmd, &mut state);
    assert_eq!(state.viewport_for(&cid), Some((640, 480)));
    // Omit viewport to reset.
    let cmd2 = format!(
        r#"{{"id":2,"method":"browsingContext.setViewport","params":{{"context":"{cid}"}}}}"#
    );
    dispatch(&cmd2, &mut state);
    assert!(state.viewport_for(&cid).is_none());
}

#[test]
fn get_tree_includes_viewport_in_context_info() {
    let mut state = BidiState::new();
    let cid = new_session_ctx(&mut state);
    let cmd = format!(
        r#"{{"id":1,"method":"browsingContext.setViewport","params":{{"context":"{cid}","viewport":{{"width":1920,"height":1080}}}}}}"#
    );
    dispatch(&cmd, &mut state);
    let r = dispatch(r#"{"id":2,"method":"browsingContext.getTree","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let ctx = &v.get("result").unwrap().get("contexts").unwrap().as_array().unwrap()[0];
    let vp = ctx.get("viewport").unwrap();
    assert_eq!(vp.get("width").and_then(|x| x.as_number()), Some(1920.0));
    assert_eq!(vp.get("height").and_then(|x| x.as_number()), Some(1080.0));
}

// ── preload per-context ──

#[test]
fn create_context_preload_scripts_empty_when_none() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    let r = dispatch(
        r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    let scripts = v
        .get("result")
        .unwrap()
        .get("preloadScripts")
        .and_then(|x| x.as_array())
        .unwrap();
    assert!(scripts.is_empty());
}

#[test]
fn create_context_preload_scripts_global_script_included() {
    let mut state = BidiState::new();
    new_session_ctx(&mut state);
    // Register a global preload script (no specific contexts).
    let r = dispatch(
        r#"{"id":5,"method":"script.addPreloadScript","params":{"functionDeclaration":"()=>{}"}}"#,
        &mut state,
    );
    let script_id = parse(&r.frames[0])
        .get("result")
        .unwrap()
        .get("script")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();

    let r2 = dispatch(
        r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#,
        &mut state,
    );
    let v = parse(&r2.frames[0]);
    let scripts = v
        .get("result")
        .unwrap()
        .get("preloadScripts")
        .and_then(|x| x.as_array())
        .unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].as_str(), Some(script_id.as_str()));
}

#[test]
fn create_context_targeted_preload_not_included_for_different_context() {
    let mut state = BidiState::new();
    let root_ctx = new_session_ctx(&mut state);
    // Preload script targeting only the root context.
    let cmd = format!(
        r#"{{"id":5,"method":"script.addPreloadScript","params":{{"functionDeclaration":"()=>{{}}","contexts":["{root_ctx}"]}}}}"#
    );
    dispatch(&cmd, &mut state);

    // New context should NOT get the script (it targets root, not new_ctx).
    let r = dispatch(
        r#"{"id":10,"method":"browsingContext.create","params":{"type":"tab"}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    let scripts = v
        .get("result")
        .unwrap()
        .get("preloadScripts")
        .and_then(|x| x.as_array())
        .unwrap();
    assert!(scripts.is_empty());
}

// ── download lifecycle: browser.getDownloads ──

#[test]
fn get_downloads_empty_initially() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let r = dispatch(r#"{"id":2,"method":"browser.getDownloads","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let downloads = v
        .get("result")
        .unwrap()
        .get("downloads")
        .and_then(|x| x.as_array())
        .unwrap();
    assert!(downloads.is_empty());
}

#[test]
fn get_downloads_returns_in_progress_item() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let (item_id, _) =
        state.begin_download("https://ex.com/file.zip".into(), "file.zip".into());

    let r = dispatch(r#"{"id":2,"method":"browser.getDownloads","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let downloads = v
        .get("result")
        .unwrap()
        .get("downloads")
        .and_then(|x| x.as_array())
        .unwrap();
    assert_eq!(downloads.len(), 1);
    assert_eq!(downloads[0].get("downloadId").and_then(|x| x.as_str()), Some(item_id.as_str()));
    assert_eq!(downloads[0].get("state").and_then(|x| x.as_str()), Some("inProgress"));
}

#[test]
fn get_downloads_reflects_completed_state() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    let (item_id, _) = state.begin_download("https://ex.com/a.zip".into(), "a.zip".into());
    state.complete_download(&item_id);

    let r = dispatch(r#"{"id":2,"method":"browser.getDownloads","params":{}}"#, &mut state);
    let v = parse(&r.frames[0]);
    let dl = &v.get("result").unwrap().get("downloads").unwrap().as_array().unwrap()[0];
    assert_eq!(dl.get("state").and_then(|x| x.as_str()), Some("completed"));
}

// ── cookie-change events from storage commands ──

#[test]
fn set_cookie_emits_added_event_when_subscribed() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"session.subscribe","params":{"events":["storage.cookieAdded"]}}"#,
        &mut state,
    );
    let r = dispatch(
        r#"{"id":3,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":"1","domain":"ex.com"}}}"#,
        &mut state,
    );
    assert_eq!(r.frames.len(), 2); // success + event
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("storage.cookieAdded"));
    let c = ev.get("params").unwrap().get("cookie").unwrap();
    assert_eq!(c.get("name").and_then(|x| x.as_str()), Some("a"));
}

#[test]
fn set_cookie_emits_changed_event_on_replace() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"x","value":"1","domain":"ex.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":3,"method":"session.subscribe","params":{"events":["storage.cookieChanged"]}}"#,
        &mut state,
    );
    // Replace same name+domain+path → should emit cookieChanged.
    let r = dispatch(
        r#"{"id":4,"method":"storage.setCookie","params":{"cookie":{"name":"x","value":"2","domain":"ex.com"}}}"#,
        &mut state,
    );
    assert_eq!(r.frames.len(), 2);
    let ev = parse(&r.frames[1]);
    assert_eq!(ev.get("method").and_then(|x| x.as_str()), Some("storage.cookieChanged"));
}

#[test]
fn delete_cookies_emits_removed_events_when_subscribed() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":"1","domain":"ex.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":3,"method":"storage.setCookie","params":{"cookie":{"name":"b","value":"2","domain":"ex.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":4,"method":"session.subscribe","params":{"events":["storage.cookieRemoved"]}}"#,
        &mut state,
    );
    let r = dispatch(
        r#"{"id":5,"method":"storage.deleteCookies","params":{"filter":{"domain":"ex.com"}}}"#,
        &mut state,
    );
    // success + 2 removed events (one per cookie).
    assert_eq!(r.frames.len(), 3);
    let ev0 = parse(&r.frames[1]);
    let ev1 = parse(&r.frames[2]);
    assert_eq!(ev0.get("method").and_then(|x| x.as_str()), Some("storage.cookieRemoved"));
    assert_eq!(ev1.get("method").and_then(|x| x.as_str()), Some("storage.cookieRemoved"));
    assert_eq!(state.cookie_count(), 0);
}

// ── per-origin clear: partition.sourceOrigin ──

#[test]
fn get_cookies_filters_by_partition_source_origin() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":"1","domain":"example.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":3,"method":"storage.setCookie","params":{"cookie":{"name":"b","value":"2","domain":"other.com"}}}"#,
        &mut state,
    );
    let r = dispatch(
        r#"{"id":4,"method":"storage.getCookies","params":{"partition":{"sourceOrigin":"https://example.com"}}}"#,
        &mut state,
    );
    let v = parse(&r.frames[0]);
    let cookies = v.get("result").unwrap().get("cookies").unwrap().as_array().unwrap();
    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].get("name").and_then(|x| x.as_str()), Some("a"));
}

#[test]
fn delete_cookies_by_partition_source_origin() {
    let mut state = BidiState::new();
    dispatch(r#"{"id":1,"method":"session.new","params":{}}"#, &mut state);
    dispatch(
        r#"{"id":2,"method":"storage.setCookie","params":{"cookie":{"name":"a","value":"1","domain":"example.com"}}}"#,
        &mut state,
    );
    dispatch(
        r#"{"id":3,"method":"storage.setCookie","params":{"cookie":{"name":"b","value":"2","domain":"other.com"}}}"#,
        &mut state,
    );
    assert_eq!(state.cookie_count(), 2);
    let r = dispatch(
        r#"{"id":4,"method":"storage.deleteCookies","params":{"partition":{"sourceOrigin":"https://example.com"}}}"#,
        &mut state,
    );
    assert!(r.frames[0].contains("success"));
    assert_eq!(state.cookie_count(), 1);
}

#[test]
fn origin_to_domain_strips_scheme_and_port() {
    assert_eq!(origin_to_domain("https://example.com"), "example.com");
    assert_eq!(origin_to_domain("http://sub.example.com:8080"), "sub.example.com");
    assert_eq!(origin_to_domain("//example.com/path"), "example.com");
}
