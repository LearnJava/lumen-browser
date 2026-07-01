//! [`BrowserSession`] over a live shell window (SDC-2).
//!
//! `WinitSession` (see [`crate::winit_session`]) is a standalone skeleton
//! session, not the actual `lumen-shell` binary's window — the real live
//! window is `Lumen` (a private struct in `lumen-shell::main`), reachable
//! only through the [`AutomationHandle`] channel wired up in SDC-1b. This
//! module adapts that channel to the same [`BrowserSession`] trait every
//! other session implements, so `lumen-bidi-server` and `lumen-mcp` can
//! drive a real, visible window with the exact same API as headless tests.
//!
//! MVP scope (ROADMAP SDC-2): navigate/click/type/scroll/wait/eval/screenshot/
//! query/a11y_tree are real round-trips to the live window. The remaining
//! `BrowserSession` methods (layout snapshots, computed style, network/console
//! logs, fingerprint/clock/rng isolation controls) are not yet threaded
//! through `AutomationCommand` — they return local, documented defaults so
//! this type satisfies the trait without silently pretending to support
//! features the live channel doesn't carry yet.

use std::sync::Mutex;
use std::time::Duration;

use lumen_core::error::{Error, Result};

use crate::{
    A11yNode, AutomationCommand, AutomationHandle, AutomationReply, AxQuery, BoxModel,
    BrowserSession, ComputedProperties, ComputedStyleSnapshot, ConsoleEntry, FingerprintProfile,
    NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
};

/// Default timeout for a single automation round-trip to the live window.
///
/// Generous enough for a real page navigation (network fetch + layout) but
/// still bounded — a hung shell must not block the caller forever.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// [`BrowserSession`] adapter that drives a live `lumen-shell` window through
/// its [`AutomationHandle`] channel (SDC-2).
///
/// One instance is bound to one live window process; `current_url` is
/// tracked locally (updated on every successful `navigate()`) since the
/// automation channel has no dedicated "current URL" query.
pub struct LiveWindowSession {
    handle: AutomationHandle,
    current_url: Mutex<String>,
}

impl LiveWindowSession {
    /// Bind a new session to `handle`, the sending half of a live window's
    /// automation channel (see `lumen-shell`'s `automation_sender()`).
    pub fn new(handle: AutomationHandle) -> Self {
        Self { handle, current_url: Mutex::new(String::new()) }
    }

    /// Send `command` and unwrap the expected reply variant, mapping
    /// `AutomationReply::Error` and any other unexpected reply to `Err`.
    fn execute(&self, command: AutomationCommand) -> Result<AutomationReply> {
        match self.handle.execute(command, DEFAULT_TIMEOUT)? {
            AutomationReply::Error(msg) => Err(Error::Other(msg)),
            other => Ok(other),
        }
    }
}

impl BrowserSession for LiveWindowSession {
    // ── Ресурсы ────────────────────────────────────────────────────────────

    fn screenshot(&self) -> Result<Vec<u8>> {
        match self.execute(AutomationCommand::Screenshot)? {
            AutomationReply::Screenshot(png) => Ok(png),
            other => Err(unexpected_reply("Screenshot", &other)),
        }
    }

    fn a11y_tree(&self) -> Result<A11yNode> {
        match self.execute(AutomationCommand::A11yTree)? {
            AutomationReply::A11yTree(tree) => Ok(*tree),
            other => Err(unexpected_reply("A11yTree", &other)),
        }
    }

    fn query_a11y(&self, query: &AxQuery) -> Result<Option<A11yNode>> {
        let tree = self.a11y_tree()?;
        Ok(find_a11y_node(&tree, query))
    }

    fn query_a11y_all(&self, query: &AxQuery) -> Result<Vec<A11yNode>> {
        let tree = self.a11y_tree()?;
        let mut out = Vec::new();
        find_all_a11y_nodes(&tree, query, &mut out);
        Ok(out)
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always empty.
    fn layout_snapshot(&self) -> Result<Vec<BoxModel>> {
        Ok(Vec::new())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always `None`.
    fn computed_style(&self, _selector: &str) -> Result<Option<ComputedProperties>> {
        Ok(None)
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always `None`.
    fn computed_style_snapshot(&self, _selector: &str) -> Result<Option<ComputedStyleSnapshot>> {
        Ok(None)
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always `None`.
    fn layout_box_by_selector(&self, _selector: &str) -> Result<Option<BoxModel>> {
        Ok(None)
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always empty.
    fn all_layout_boxes_by_selector(&self, _selector: &str) -> Result<Vec<BoxModel>> {
        Ok(Vec::new())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always empty.
    fn network_log(&self) -> Result<Vec<NetworkEntry>> {
        Ok(Vec::new())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always empty.
    fn console_log(&self) -> Result<Vec<ConsoleEntry>> {
        Ok(Vec::new())
    }

    fn current_url(&self) -> String {
        self.current_url.lock().map(|g| g.clone()).unwrap_or_default()
    }

    // ── Инструменты ────────────────────────────────────────────────────────

    fn navigate(&mut self, url: &str) -> Result<()> {
        self.execute(AutomationCommand::Navigate(url.to_owned()))?;
        if let Ok(mut cur) = self.current_url.lock() {
            *cur = url.to_owned();
        }
        Ok(())
    }

    fn click(&mut self, target: &Target) -> Result<()> {
        self.execute(AutomationCommand::Click(target.clone()))?;
        Ok(())
    }

    fn type_text(&mut self, target: &Target, text: &str) -> Result<()> {
        self.execute(AutomationCommand::Type(target.clone(), text.to_owned()))?;
        Ok(())
    }

    fn scroll(&mut self, _target: &Target, delta: ScrollDelta) -> Result<()> {
        self.execute(AutomationCommand::Scroll(delta))?;
        Ok(())
    }

    fn wait(&mut self, cond: WaitCondition, timeout_ms: u64) -> Result<()> {
        // The live window polls the condition once per frame and only replies
        // once it's satisfied or its own deadline passes — give the round-trip
        // itself a little headroom over that deadline.
        let round_trip_timeout = Duration::from_millis(timeout_ms) + Duration::from_secs(2);
        match self.handle.execute(AutomationCommand::Wait(cond, timeout_ms), round_trip_timeout)? {
            AutomationReply::Error(msg) => Err(Error::Other(msg)),
            _ => Ok(()),
        }
    }

    fn eval(&self, js: &str) -> Result<String> {
        match self.execute(AutomationCommand::Eval(js.to_owned()))? {
            AutomationReply::Eval(json) => Ok(json),
            other => Err(unexpected_reply("Eval", &other)),
        }
    }

    fn query(&self, selector: &str) -> Result<Vec<NodeRef>> {
        match self.execute(AutomationCommand::Query(selector.to_owned()))? {
            AutomationReply::Query(nodes) => Ok(nodes),
            other => Err(unexpected_reply("Query", &other)),
        }
    }

    // ── Isolation & Fingerprinting ───────────────────────────────────────────
    // Not yet wired to the live window (SDC-2 MVP scope): the automation
    // channel carries page-interaction commands only, not per-session
    // isolation controls. These return local no-op defaults so
    // `LiveWindowSession` satisfies the trait; live wiring is future work.

    fn fingerprint_profile(&self) -> FingerprintProfile {
        FingerprintProfile::Standard
    }

    fn set_fingerprint_profile(&mut self, _profile: FingerprintProfile) -> Result<()> {
        Ok(())
    }

    fn user_agent(&self) -> String {
        format!("Lumen/{}", env!("CARGO_PKG_VERSION"))
    }

    fn set_user_agent(&mut self, _ua: &str) -> Result<()> {
        Ok(())
    }

    fn set_clock(&mut self, _mode: crate::ClockMode) -> Result<()> {
        Ok(())
    }

    fn set_rng_seed(&mut self, _seed: Option<u64>) -> Result<()> {
        Ok(())
    }

    fn freeze_fingerprint(&mut self, _profile: FingerprintProfile) -> Result<()> {
        Ok(())
    }
}

fn unexpected_reply(expected: &str, got: &AutomationReply) -> Error {
    Error::Other(format!("live window: expected {expected} reply, got {got:?}"))
}

fn find_a11y_node(node: &A11yNode, query: &AxQuery) -> Option<A11yNode> {
    if matches_query(node, query) {
        return Some(node.clone());
    }
    node.children.iter().find_map(|c| find_a11y_node(c, query))
}

fn find_all_a11y_nodes(node: &A11yNode, query: &AxQuery, out: &mut Vec<A11yNode>) {
    if matches_query(node, query) {
        out.push(node.clone());
    }
    for child in &node.children {
        find_all_a11y_nodes(child, query, out);
    }
}

fn matches_query(node: &A11yNode, query: &AxQuery) -> bool {
    match query {
        AxQuery::Role { role, name } => {
            if !node.role.eq_ignore_ascii_case(role) {
                return false;
            }
            name.as_ref().is_none_or(|n| node.name.to_lowercase().contains(&n.to_lowercase()))
        }
        AxQuery::NameContains(name) => node.name.to_lowercase().contains(&name.to_lowercase()),
    }
}
