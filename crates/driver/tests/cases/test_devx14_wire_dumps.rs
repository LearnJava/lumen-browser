//! DEVX-14: `resource://x-display-list`/`x-computed-style` in MCP, plus
//! `resource://layout`/`resource://network` wired to the live window.
//!
//! Covers the DoD from `docs/tasks/p1-introspection-track.md` §DEVX-14:
//! `BrowserSession::display_list()` (whole-page, unlike the already-covered
//! `display_list_scoped` from DEVX-12) and `LiveWindowSession`'s new
//! `layout_snapshot()`/`network_log()` round-trips through the automation
//! channel instead of returning their old hardcoded-empty defaults.
#![cfg(all(feature = "v8", feature = "cpu-render"))]

use lumen_driver::{
    AutomationCommand, AutomationHandle, AutomationReply, BoxModel, BrowserSession,
    InProcessSession, LiveWindowSession, NetworkEntry, WinitSession,
};

#[test]
fn display_list_contains_commands_from_the_whole_page_unlike_the_scoped_variant() {
    let mut session = InProcessSession::new();
    session
        .navigate_html(
            r#"<html><body style="margin:0">
                 <div id="a" style="width:10px;height:10px;background:blue"></div>
                 <div id="b" style="width:10px;height:10px;background:red"></div>
               </body></html>"#,
        )
        .expect("navigate_html failed");

    let dump = session.display_list().expect("display_list failed");
    assert!(dump.contains("#0000ffff"), "expected #a's color in the whole-page dump:\n{dump}");
    assert!(dump.contains("#ff0000ff"), "expected #b's color in the whole-page dump:\n{dump}");

    // The scoped variant (DEVX-12), by contrast, must exclude the sibling.
    let scoped = session.display_list_scoped("#b").expect("display_list_scoped failed");
    assert!(!scoped.contains("#0000ffff"), "scoped dump must not include #a's color:\n{scoped}");
}

#[test]
fn winit_session_display_list_contains_page_commands() {
    let mut session = WinitSession::new();
    session
        .navigate_html(r#"<div style="width:10px;height:10px;background:lime"></div>"#)
        .expect("navigate_html failed");

    let dump = session.display_list().expect("display_list failed");
    assert!(dump.contains("FillRect"), "expected a fill command in the dump:\n{dump}");
}

/// A fake "live window" answering `LayoutSnapshot`/`NetworkLog`/`DisplayList`/
/// `DisplayListScoped` from a background thread, the same pattern
/// `lumen-bidi-server`'s `fake_live_session` uses for BiDi-relevant commands —
/// here for the DEVX-14/S2b wired MCP-relevant ones.
fn fake_live_session(boxes: Vec<BoxModel>, entries: Vec<NetworkEntry>) -> LiveWindowSession {
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::LayoutSnapshot => AutomationReply::LayoutSnapshot(boxes.clone()),
                AutomationCommand::NetworkLog => AutomationReply::NetworkLog(entries.clone()),
                AutomationCommand::DisplayList => AutomationReply::DisplayList("whole-page dump".into()),
                AutomationCommand::MemReport => AutomationReply::MemReport("MEM_REPORT dl_cmds=0".into()),
                AutomationCommand::FrameStats => AutomationReply::FrameStats("FRAME_STATS shown=1 capacity=120 recorded=1".into()),
                AutomationCommand::DisplayListScoped { selector } => {
                    AutomationReply::DisplayList(format!("scoped [{selector}] dump"))
                }
                // MCP-TOTAL-S2c: echo the selectors back so the round-trip test
                // can see they reached the arm.
                AutomationCommand::LayoutSnapshotScoped { selector } => {
                    AutomationReply::LayoutSnapshot(vec![BoxModel {
                        node_id: 7,
                        tag_name: format!("scoped [{selector}]"),
                        border_box: lumen_core::geom::Rect::ZERO,
                        margin_box: lumen_core::geom::Rect::ZERO,
                    }])
                }
                AutomationCommand::QueryScoped { root_selector, selector } => {
                    AutomationReply::Query(vec![lumen_driver::NodeRef {
                        node_id: 9,
                        tag_name: format!("a within [{root_selector}]"),
                        text_content: format!("matched [{selector}]"),
                        bounding_rect: lumen_core::geom::Rect::ZERO,
                    }])
                }
                // MCP-TOTAL-S2d: canned answers echoing the selector through,
                // so the round-trip test sees both the selector and the
                // payload survive the channel.
                AutomationCommand::ComputedStyle { selector } => {
                    AutomationReply::ComputedStyle(Some(lumen_driver::ComputedProperties {
                        properties: [("color".to_string(), format!("scoped [{selector}]"))].into(),
                    }))
                }
                AutomationCommand::ExplainElement { selector } => {
                    AutomationReply::ExplainElement(lumen_driver::ExplainElement {
                        in_dom: true,
                        heuristic: Some(format!("scoped [{selector}]")),
                        ..Default::default()
                    })
                }
                AutomationCommand::ExplainPage => AutomationReply::ExplainPage(lumen_driver::ExplainPage {
                    box_count: 5,
                    ..Default::default()
                }),
                // MCP-TOTAL-S2e: canned answers echoing the selector/code
                // through, so the round-trip test sees them survive the channel.
                AutomationCommand::ScreenshotScoped { selector } => {
                    AutomationReply::Screenshot(format!("png [{selector}]").into_bytes())
                }
                AutomationCommand::RelayoutScoped { selector: _ } => AutomationReply::Ack,
                AutomationCommand::EvalScoped { selector, code } => {
                    AutomationReply::Eval(format!("eval [{selector}] [{code}]"))
                }
                _ => AutomationReply::Error("unsupported in fake_live_session".into()),
            };
            let _ = reply_tx.send(reply);
        }
    });
    LiveWindowSession::new(AutomationHandle::new(tx))
}

#[test]
fn live_window_layout_snapshot_round_trips_through_automation_channel() {
    let boxes = vec![BoxModel {
        node_id: 1,
        tag_name: "div".to_string(),
        border_box: lumen_core::geom::Rect::new(0.0, 0.0, 10.0, 10.0),
        margin_box: lumen_core::geom::Rect::new(0.0, 0.0, 10.0, 10.0),
    }];
    let session = fake_live_session(boxes, Vec::new());
    let got = session.layout_snapshot().expect("layout_snapshot failed");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tag_name, "div");
}

#[test]
fn live_window_network_log_round_trips_through_automation_channel() {
    let entries = vec![NetworkEntry {
        url: "https://example.com/".to_string(),
        method: "GET".to_string(),
        status: 200,
        size_bytes: 0,
        // MCP-TOTAL-S5a: blocked/failed/reason/duration_ms ride the same wire.
        blocked: false,
        failed: false,
        reason: None,
        duration_ms: Some(42),
    }];
    let session = fake_live_session(Vec::new(), entries);
    let got = session.network_log().expect("network_log failed");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].url, "https://example.com/");
    assert_eq!(got[0].status, 200);
    assert_eq!(got[0].duration_ms, Some(42));
}

/// MCP-TOTAL-S5a: wire payloads recorded before the new fields existed (and
/// headless sessions that never fill them) deserialize with serde defaults.
#[test]
fn network_entry_deserializes_without_the_new_fields() {
    let entry: NetworkEntry = serde_json::from_str(
        r#"{"url":"https://example.com/","method":"GET","status":200,"size_bytes":10}"#,
    )
    .expect("old wire payload must deserialize");
    assert!(!entry.blocked);
    assert!(!entry.failed);
    assert_eq!(entry.reason, None);
    assert_eq!(entry.duration_ms, None);
}

/// MCP-TOTAL-S2b: both display-list reads round-trip through the automation
/// channel — the whole-page dump verbatim, the scoped one carrying the
/// selector through to the arm.
#[test]
fn live_window_display_list_round_trips_through_automation_channel() {
    let session = fake_live_session(Vec::new(), Vec::new());
    assert_eq!(session.display_list().expect("display_list failed"), "whole-page dump");
    assert_eq!(
        session.display_list_scoped("#b").expect("display_list_scoped failed"),
        "scoped [#b] dump"
    );
}

/// MCP-TOTAL-S5b: the memory report round-trips through the automation
/// channel; a headless session answers the honest "live window only" error
/// (the trait default — headless sessions collect no mem report).
#[test]
fn live_window_mem_report_round_trips_and_headless_errors() {
    let session = fake_live_session(Vec::new(), Vec::new());
    assert_eq!(
        session.mem_report().expect("mem_report failed"),
        "MEM_REPORT dl_cmds=0"
    );

    let headless = InProcessSession::new();
    let err = headless.mem_report().expect_err("headless mem_report must be an error");
    assert!(err.to_string().contains("только для живого окна"), "wrong error: {err}");
}

/// MCP-TOTAL-S5c: the frame-stats resource round-trips through the
/// automation channel; a headless session answers the honest "live window
/// only" error (the trait default — headless sessions count no frames).
#[test]
fn live_window_frame_stats_round_trips_and_headless_errors() {
    let session = fake_live_session(Vec::new(), Vec::new());
    assert_eq!(
        session.frame_stats().expect("frame_stats failed"),
        "FRAME_STATS shown=1 capacity=120 recorded=1"
    );

    let headless = InProcessSession::new();
    let err = headless.frame_stats().expect_err("headless frame_stats must be an error");
    assert!(err.to_string().contains("только для живого окна"), "wrong error: {err}");
}

/// MCP-TOTAL-S2c: the scoped layout snapshot and the subtree-scoped query
/// round-trip through the automation channel, carrying the selectors through
/// to the arm.
#[test]
fn live_window_scoped_layout_and_query_round_trip_through_automation_channel() {
    let session = fake_live_session(Vec::new(), Vec::new());
    let boxes = session.layout_snapshot_scoped("#box").expect("layout_snapshot_scoped failed");
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].tag_name, "scoped [#box]");
    let nodes = session.query_scoped("#box", "a").expect("query_scoped failed");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag_name, "a within [#box]");
    assert_eq!(nodes[0].text_content, "matched [a]");
}

/// MCP-TOTAL-S2d: computed style and both explain reads round-trip through
/// the automation channel, carrying the selector through to the arm.
#[test]
fn live_window_computed_style_and_explain_round_trip_through_automation_channel() {
    let session = fake_live_session(Vec::new(), Vec::new());
    let props = session.computed_style("#box").expect("computed_style failed").expect("Some");
    assert_eq!(props.properties["color"], "scoped [#box]");
    let chain = session.explain_element("#box").expect("explain_element failed");
    assert!(chain.in_dom);
    assert_eq!(chain.heuristic.as_deref(), Some("scoped [#box]"));
    let page = session.explain_page().expect("explain_page failed");
    assert_eq!(page.box_count, 5);
}

/// MCP-TOTAL-S2e: the scoped screenshot, relayout and eval round-trip through
/// the automation channel — the PNG bytes and the eval JSON verbatim, the
/// selector carried through to the arm.
#[test]
fn live_window_scoped_screenshot_relayout_eval_round_trip_through_automation_channel() {
    let mut session = fake_live_session(Vec::new(), Vec::new());
    let png = session.screenshot_scoped("#box").expect("screenshot_scoped failed");
    assert_eq!(png, b"png [#box]");
    session.relayout_scoped("#box").expect("relayout_scoped failed");
    let json = session
        .eval_scoped("#box", "1 + 1")
        .expect("eval_scoped failed");
    assert_eq!(json, "eval [#box] [1 + 1]");
}

/// MCP-TOTAL-S2a: the remaining silent x-surface stubs (`Ok(пусто)`/
/// `Ok(None)`) answer an honest `Err` instead. The receiver half is dropped,
/// so a stub that still tried an automation round-trip would fail on `send` —
/// the only `Err` these methods can return is their own. (S2c un-stubbed
/// `layout_snapshot_scoped`/`query_scoped`, S2b `display_list*`, S2d
/// `computed_style`/`explain_element`/`explain_page`, S2e
/// `screenshot_scoped`/`relayout_scoped`/`eval_scoped`.)
#[test]
fn live_window_x_surface_stubs_are_honest_errors() {
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    drop(rx);
    let session = LiveWindowSession::new(AutomationHandle::new(tx));

    let check = |r: lumen_core::error::Result<()>| {
        let msg = r.expect_err("stub must answer Err").to_string();
        assert!(msg.contains("не реализовано для LiveWindowSession"), "wrong stub error: {msg}");
    };
    check(session.computed_style_snapshot("#a").map(drop));
    check(session.layout_box_by_selector("#a").map(drop));
    check(session.all_layout_boxes_by_selector("#a").map(drop));
}

/// MCP-TOTAL-S4a: `set_viewport` round-trips through the automation channel —
/// the fake arm records exactly what arrived, the session gets its `Ack`,
/// and the reset form (`None`/`None`) carries through too.
#[test]
fn live_window_set_viewport_round_trips_through_automation_channel() {
    let got = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    let rec = std::sync::Arc::clone(&got);
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = if let AutomationCommand::SetViewport { width, height } = cmd {
                *rec.lock().unwrap() = Some((width, height));
                AutomationReply::Ack
            } else {
                AutomationReply::Error("unsupported".into())
            };
            let _ = reply_tx.send(reply);
        }
    });
    let mut session = LiveWindowSession::new(AutomationHandle::new(tx));

    session.set_viewport(Some(375.0), Some(667.0)).expect("set_viewport failed");
    assert_eq!(*got.lock().unwrap(), Some((Some(375.0), Some(667.0))));
    session.set_viewport(None, None).expect("viewport reset failed");
    assert_eq!(*got.lock().unwrap(), Some((None, None)));
}

/// MCP-TOTAL-S4b: the window tools round-trip through the automation
/// channel — `enabled` carries into `SetFullscreen`, `FocusWindow` and
/// `GetWindowRect` arrive as their own commands, and the rect comes back
/// as the typed [`lumen_driver::WindowRect`].
#[test]
fn live_window_window_tools_round_trip_through_automation_channel() {
    use lumen_driver::WindowRect;

    let got_fullscreen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    let rec = std::sync::Arc::clone(&got_fullscreen);
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::SetFullscreen { enabled } => {
                    *rec.lock().unwrap() = Some(enabled);
                    AutomationReply::Ack
                }
                AutomationCommand::FocusWindow => AutomationReply::Ack,
                AutomationCommand::GetWindowRect => AutomationReply::WindowRect(WindowRect {
                    x: 12.0,
                    y: 34.0,
                    width: 1280.0,
                    height: 720.0,
                    maximized: false,
                    minimized: true,
                }),
                _ => AutomationReply::Error("unsupported".into()),
            };
            let _ = reply_tx.send(reply);
        }
    });
    let session = LiveWindowSession::new(AutomationHandle::new(tx));

    session.set_fullscreen(true).expect("set_fullscreen failed");
    assert_eq!(*got_fullscreen.lock().unwrap(), Some(true));
    session.focus_window().expect("focus_window failed");
    let rect = session.get_window_rect().expect("get_window_rect failed");
    assert_eq!(
        rect,
        WindowRect {
            x: 12.0,
            y: 34.0,
            width: 1280.0,
            height: 720.0,
            maximized: false,
            minimized: true,
        }
    );
}
