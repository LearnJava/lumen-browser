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

/// A fake "live window" answering `LayoutSnapshot`/`NetworkLog` from a
/// background thread, the same pattern `lumen-bidi-server`'s
/// `fake_live_session` uses for BiDi-relevant commands — here for the two
/// DEVX-14 wired MCP-relevant ones.
fn fake_live_session(boxes: Vec<BoxModel>, entries: Vec<NetworkEntry>) -> LiveWindowSession {
    let (tx, rx) = std::sync::mpsc::channel::<lumen_driver::AutomationRequest>();
    std::thread::spawn(move || {
        for (cmd, reply_tx) in rx {
            let reply = match cmd {
                AutomationCommand::LayoutSnapshot => AutomationReply::LayoutSnapshot(boxes.clone()),
                AutomationCommand::NetworkLog => AutomationReply::NetworkLog(entries.clone()),
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
    }];
    let session = fake_live_session(Vec::new(), entries);
    let got = session.network_log().expect("network_log failed");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].url, "https://example.com/");
    assert_eq!(got[0].status, 200);
}
