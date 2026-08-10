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
//! query/a11y_tree/console_log/layout_snapshot/network_log are real
//! round-trips to the live window (console_log added DEVX-1: reads the
//! DevTools console buffer, cleared on each `navigate()`; layout_snapshot/
//! network_log added DEVX-14: read the live window's layout tree and DevTools
//! network panel log respectively). The remaining `BrowserSession` methods
//! (scoped layout/display-list reads, computed style, fingerprint/clock/rng
//! isolation controls) are not yet threaded through `AutomationCommand` —
//! they return local, documented defaults so this type satisfies the trait
//! without silently pretending to support features the live channel doesn't
//! carry yet.

use std::sync::Mutex;
use std::time::Duration;

use lumen_core::error::{Error, Result};

use crate::{
    A11yNode, AutomationCommand, AutomationHandle, AutomationReply, AxQuery, BoxModel,
    BrowserSession, ComputedProperties, ComputedStyleSnapshot, ConsoleEntry, ExplainElement,
    ExplainPage, FingerprintProfile, InterceptedRequest, NetworkEntry, NodeRef, ScrollDelta, Target,
    WaitCondition,
};

/// Default timeout for a single automation round-trip to the live window.
///
/// Generous enough for a real page navigation (network fetch + layout) but
/// still bounded — a hung shell must not block the caller forever.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// [`BrowserSession`] adapter that drives a live `lumen-shell` window through
/// its [`AutomationHandle`] channel (SDC-2).
///
/// One instance is bound to one live window process. The automation channel
/// has no dedicated "current URL" query, so `current_url` keeps the address of
/// the last requested navigation; it is only a fallback — the reader
/// ([`BrowserSession::current_url`]) asks the document itself, because the
/// requested address and the document's real one diverge on every server
/// redirect (BUG-757).
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

    /// Box-model snapshot of the whole page (DEVX-14) — round-trips through
    /// `AutomationCommand::LayoutSnapshot` to the live window's layout tree.
    fn layout_snapshot(&self) -> Result<Vec<BoxModel>> {
        match self.execute(AutomationCommand::LayoutSnapshot)? {
            AutomationReply::LayoutSnapshot(boxes) => Ok(boxes),
            other => Err(unexpected_reply("LayoutSnapshot", &other)),
        }
    }

    /// Not yet wired to the live window: `AutomationCommand` has no scoped
    /// layout-snapshot variant yet (SDC-2 MVP scope, DEVX-14 wired the
    /// whole-page `layout_snapshot` but not this) — always empty.
    fn layout_snapshot_scoped(&self, _selector: &str) -> Result<Vec<BoxModel>> {
        Ok(Vec::new())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always `Err`
    /// (unlike the other scoped reads, an empty screenshot/dump isn't a
    /// meaningfully distinct "not found" answer, so this mirrors
    /// `screenshot_scoped`'s own not-found error rather than silently
    /// returning nothing).
    fn screenshot_scoped(&self, _selector: &str) -> Result<Vec<u8>> {
        Err(Error::Other("screenshot_scoped: не реализовано для LiveWindowSession (SDC-2 MVP)".into()))
    }

    /// Not yet wired to the live window (SDC-2 MVP scope) — always `Err`,
    /// see `screenshot_scoped`. Unlike `display_list_scoped`, the whole-page
    /// [`display_list`](BrowserSession::display_list) has the same gap —
    /// `AutomationCommand` carries no display-list variant at all yet
    /// (DEVX-14 wired `layout_snapshot`/`network_log` only).
    fn display_list_scoped(&self, _selector: &str) -> Result<String> {
        Err(Error::Other("display_list_scoped: не реализовано для LiveWindowSession (SDC-2 MVP)".into()))
    }

    /// Not yet wired to the live window (SDC-2 MVP scope): `AutomationCommand`
    /// carries no display-list variant yet — always `Err`. Unlike
    /// `layout_snapshot`/`network_log`, DEVX-14 left this one a documented
    /// gap rather than wiring it (the brief scoped live-window work to
    /// `resource://layout`/`resource://network` only).
    fn display_list(&self) -> Result<String> {
        Err(Error::Other("display_list: не реализовано для LiveWindowSession (SDC-2 MVP)".into()))
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

    /// Not yet wired to the live window (SDC-2 MVP scope, same gap as
    /// `layout_box_by_selector`) — always the all-default `ExplainElement`
    /// (`in_dom: false`), not an error: `explain_element` needs per-selector
    /// layout/provenance data, which `AutomationCommand` still doesn't carry
    /// even after DEVX-14 wired the whole-page `layout_snapshot`.
    fn explain_element(&self, _selector: &str) -> Result<ExplainElement> {
        Ok(ExplainElement::default())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope, same gap as
    /// `explain_element`) — always the all-default `ExplainPage`.
    fn explain_page(&self) -> Result<ExplainPage> {
        Ok(ExplainPage::default())
    }

    /// Network request log since the last `navigate()` (DEVX-14) —
    /// round-trips through `AutomationCommand::NetworkLog` to the live
    /// window's DevTools network panel log.
    fn network_log(&self) -> Result<Vec<NetworkEntry>> {
        match self.execute(AutomationCommand::NetworkLog)? {
            AutomationReply::NetworkLog(entries) => Ok(entries),
            other => Err(unexpected_reply("NetworkLog", &other)),
        }
    }

    /// Captured JS console messages since the last `navigate()` (DEVX-1) —
    /// round-trips through `AutomationCommand::ConsoleLog` to the live
    /// window's DevTools console buffer.
    fn console_log(&self) -> Result<Vec<ConsoleEntry>> {
        match self.execute(AutomationCommand::ConsoleLog)? {
            AutomationReply::ConsoleLog(entries) => Ok(entries),
            other => Err(unexpected_reply("ConsoleLog", &other)),
        }
    }

    /// URL текущего документа живого окна.
    ///
    /// BUG-757: локальный слепок хранит адрес, который *запросили*, поэтому
    /// после серверного редиректа (и после любой навигации, инициированной
    /// самой страницей) он врёт. Авторитетный источник — сам документ, и он же
    /// теперь корректен, потому что база документа строится из финального URL
    /// ответа. Слепок остаётся фолбэком на случай, когда JS-контекста ещё нет
    /// (окно между навигациями, страница без исполненных скриптов) — там
    /// «адрес, который запросили» единственное, что вообще известно.
    fn current_url(&self) -> String {
        let snapshot = self.current_url.lock().map(|g| g.clone()).unwrap_or_default();
        match self.execute(AutomationCommand::Eval("location.href".to_owned())) {
            Ok(AutomationReply::Eval(json)) => serde_json::from_str::<String>(&json)
                .ok()
                .filter(|u| !u.is_empty())
                .unwrap_or(snapshot),
            _ => snapshot,
        }
    }

    // ── Инструменты ────────────────────────────────────────────────────────

    fn navigate(&mut self, url: &str) -> Result<()> {
        self.execute(AutomationCommand::Navigate(url.to_owned()))?;
        if let Ok(mut cur) = self.current_url.lock() {
            *cur = url.to_owned();
        }
        Ok(())
    }

    fn new_tab(&mut self, url: &str) -> Result<()> {
        self.execute(AutomationCommand::NewTab(url.to_owned()))?;
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

    fn eval(&mut self, js: &str) -> Result<String> {
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

    /// Not yet wired to the live window (SDC-2 MVP scope): `AutomationCommand`
    /// has no scoped-query variant yet — always empty.
    fn query_scoped(&self, _root_selector: &str, _selector: &str) -> Result<Vec<NodeRef>> {
        Ok(Vec::new())
    }

    /// Not yet wired to the live window (SDC-2 MVP scope, same gap as
    /// `query_scoped`) — always `Err`.
    fn relayout_scoped(&mut self, _selector: &str) -> Result<()> {
        Err(Error::Other("relayout_scoped: не реализовано для LiveWindowSession (SDC-2 MVP)".into()))
    }

    /// Not yet wired to the live window (SDC-2 MVP scope, same gap as
    /// `relayout_scoped`) — always `Err`.
    fn eval_scoped(&mut self, _selector: &str, _js: &str) -> Result<String> {
        Err(Error::Other("eval_scoped: не реализовано для LiveWindowSession (SDC-2 MVP)".into()))
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

    /// BUG-295: round-trips to the live window, which threads the override
    /// into both the real HTTP `User-Agent` header (`lumen_network`'s
    /// process-global override) and `navigator.userAgent` (V8 runtime).
    fn set_user_agent(&mut self, ua: &str) -> Result<()> {
        self.execute(AutomationCommand::SetUserAgent(ua.to_owned()))?;
        Ok(())
    }

    /// BUG-295: round-trips to the live window, which flips
    /// `lumen_network`'s process-global offline flag — every fetch path
    /// (navigation, JS `fetch()`/XHR, subresources) fails immediately while
    /// active.
    fn set_offline(&mut self, offline: bool) -> Result<()> {
        self.execute(AutomationCommand::SetOffline(offline))?;
        Ok(())
    }

    /// BUG-295: round-trips to the live window, which sets a global marker
    /// and wraps `Intl.DateTimeFormat` to consult it at construction time
    /// (`v8_runtime::set_global_timezone_override`/`timezone_override_script`).
    fn set_timezone(&mut self, timezone_id: Option<&str>) -> Result<()> {
        self.execute(AutomationCommand::SetTimezone(timezone_id.map(str::to_owned)))?;
        Ok(())
    }

    /// BUG-295 remainder: round-trips to the live window, which registers
    /// the rule in `lumen_network`'s process-global intercept registry —
    /// consulted at the same `fetch_with_redirect` chokepoint every fetch
    /// path already funnels through.
    fn add_intercept(&mut self, id: &str, phases: &[String], url_patterns: &[String]) -> Result<()> {
        self.execute(AutomationCommand::AddIntercept {
            id: id.to_owned(),
            phases: phases.to_vec(),
            url_patterns: url_patterns.to_vec(),
        })?;
        Ok(())
    }

    /// BUG-295 remainder: round-trips to the live window, which removes the
    /// rule from `lumen_network`'s process-global intercept registry.
    fn remove_intercept(&mut self, id: &str) -> Result<()> {
        self.execute(AutomationCommand::RemoveIntercept(id.to_owned()))?;
        Ok(())
    }

    /// BUG-295 remainder: round-trips to the live window, which delivers the
    /// decision to `lumen_network`'s paused-request registry — unblocking the
    /// fetch worker thread waiting on it, if any request is still paused
    /// under this id.
    fn resolve_intercepted_request(&mut self, request_id: &str, continue_request: bool) -> Result<bool> {
        match self.execute(AutomationCommand::ResolveIntercept {
            request_id: request_id.to_owned(),
            continue_request,
        })? {
            AutomationReply::InterceptResolved(matched) => Ok(matched),
            other => Err(unexpected_reply("InterceptResolved", &other)),
        }
    }

    /// BUG-295 remainder: round-trips to the live window, which drains
    /// requests newly paused (since the last poll) from `lumen_network`'s
    /// registry — data for the `network.beforeRequestSent` event a BiDi
    /// connection should deliver.
    fn poll_intercepted_requests(&mut self) -> Result<Vec<InterceptedRequest>> {
        match self.execute(AutomationCommand::PollIntercepts)? {
            AutomationReply::Intercepts(requests) => Ok(requests),
            other => Err(unexpected_reply("Intercepts", &other)),
        }
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
