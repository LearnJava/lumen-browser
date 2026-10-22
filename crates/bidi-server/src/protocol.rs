//! WebDriver BiDi — command dispatcher (Phase 1, §6.11, ADR-006).
//!
//! Parses a BiDi command `{"id":N,"method":"module.command","params":{...}}`,
//! routes to the handler, and returns one or more frames for the client
//! (W3C WebDriver BiDi, Working Draft).
//!
//! Message formats (BiDi §3.4):
//! - Command: `{"id":<js-uint>,"method":"<str>","params":<obj>}`
//! - Success: `{"type":"success","id":<js-uint>,"result":<obj>}`
//! - Error:   `{"type":"error","id":<js-uint|null>,"error":"<code>","message":"<str>","stacktrace":""}`
//! - Event:   `{"type":"event","method":"<str>","params":<obj>}`
//!
//! Implemented commands:
//! - `session.*`          — status/new/subscribe/unsubscribe/end/setDefaultUserContextLocale
//! - `browsingContext.*`  — create/close/navigate/activate/getTree/captureScreenshot/
//!   handleUserPrompt/setViewport
//! - `script.*`           — evaluate/callFunction/addPreloadScript(+contexts)/removePreloadScript/disown/getRealms
//! - `network.*`          — getResponseBody/setOfflineStatus/addIntercept/removeIntercept/
//!   continueRequest/continueResponse/continueWithAuth/failRequest/setCacheBehavior
//! - `storage.*`          — getCookies/setCookie/deleteCookies(+domain filter)
//! - `input.*`            — performActions/releaseActions/setFiles
//! - `browser.*`          — setTimezoneOverride/getDownloads
//! - `emulation.*`        — setUserAgentOverride
//!
//! Public BiDi event emitters (shell integration):
//! - [`BidiState::fire_user_prompt`]    — emit `browsingContext.userPromptOpened`
//! - [`BidiState::begin_download`]      — emit `browser.downloadWillBegin`
//! - [`BidiState::update_download`]     — emit `browser.downloadItemUpdated`
//! - [`BidiState::complete_download`]   — emit `browser.downloadItemCompleted`
//! - [`BidiState::abort_download`]      — emit `browser.downloadItemAborted`
//! - [`BidiState::record_cookie_change`]— emit `storage.cookieAdded/Changed/Removed`
//!   (also auto-emitted by `storage.setCookie` / `storage.deleteCookies`)
//!
//! Live wiring to the actual engine (SDC-2, `--bidi-port` + an open window,
//! see [`BidiState::with_live_session`]): `browsingContext.navigate`,
//! `script.evaluate`, `browsingContext.captureScreenshot`, and the pointer/key
//! subset of `input.performActions` execute for real against a live
//! [`lumen_driver::LiveWindowSession`] instead of only updating in-memory
//! bookkeeping. `browsingContext.navigate` blocks on the JS runtime's real
//! `document.readyState == "complete"` signal before emitting
//! `browsingContext.load` with a real timestamp (P2-wpt S1).
//! `network.addIntercept`+`continueRequest`/`continueResponse`/`failRequest`
//! genuinely pause and resume a matching `beforeRequestSent` request or
//! `responseStarted` response (headers received, body not yet consumed)
//! against `lumen_network`'s process-global registry (BUG-295 remainder,
//! DEVX-20). The `authRequired` phase is not implemented: `addIntercept`
//! rejects it with `unsupported operation` and `continueWithAuth` answers
//! `no such request` (HTTP-auth challenges are not a first-class event in
//! `lumen_network`), instead of reporting a success with no effect.
//! `continueResponse` overrides (`statusCode`, `headers`, ...) are likewise
//! rejected as `unsupported operation`. Everything else (cookie events, a
//! dedicated `domContentLoaded` event, full input action-chain fidelity)
//! remains a later handoff — roadmap 8H.3.
//! Without a live window this layer stays a pure protocol state machine: one
//! connection = one [`BidiState`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::json::{parse as parse_json, JsonValue};
use lumen_driver::{BrowserSession, InterceptedRequest, LiveWindowSession, Target, WaitCondition};

/// Upper bound on how long `bc_navigate` blocks waiting for
/// `document.readyState == "complete"` before giving up (P2-wpt S1). Generous
/// enough for a real page load (network + script execution) but still bounded
/// so a page that never finishes loading fails the BiDi command instead of
/// hanging the connection forever.
const NAVIGATE_LOAD_TIMEOUT_MS: u64 = 30_000;

/// Opaque `clientWindow` id reported in `BrowsingContextInfo` (BiDi spec field,
/// distinct from `context`: it identifies the OS-level window a context is
/// rendered in). Lumen runs one native window per process, so a single
/// constant is correct until multi-window support exists — `wptrunner`'s
/// `webdriver.bidi.modules.browsing_context._assert_browsing_context_info`
/// asserts this field is present and a string, so omitting it breaks any BiDi
/// client using that module (found while implementing P2-wpt S4).
const CLIENT_WINDOW_ID: &str = "lumen-client-window-1";

/// Один browsing context в рамках соединения.
struct BidiContext {
    /// Непрозрачный идентификатор контекста (BiDi `context`).
    id: String,
    /// Текущий URL контекста; `about:blank` до первой навигации.
    url: String,
    /// Родительский контекст (`Some` для вложенных, `None` для top-level).
    parent: Option<String>,
    /// Per-context User-Agent override; `None` означает использование сессионного переопределения.
    ua_override: Option<String>,
    /// Viewport dimensions in CSS pixels set by `browsingContext.setViewport`.
    ///
    /// `None` = no explicit viewport set (browser defaults apply).
    viewport: Option<(u32, u32)>,
}

/// One network intercept rule registered via `network.addIntercept`. This
/// connection-local record is the Phase 1 bookkeeping used by
/// `intercept_count()`; the actual matching happens against a separate copy
/// synced into `lumen_network`'s process-global registry (BUG-295 remainder,
/// `LiveWindowSession::add_intercept`), since the network layer running on
/// another thread has no access to this connection's `BidiState`.
struct NetworkIntercept {
    /// Opaque intercept identifier (BiDi `intercept`).
    id: String,
    /// Phases at which to intercept: "beforeRequestSent", "responseStarted", "authRequired".
    #[allow(dead_code)]
    phases: Vec<String>,
    /// URL patterns to match; empty means match-all.
    #[allow(dead_code)]
    url_patterns: Vec<String>,
}

/// A preload script registered via `script.addPreloadScript` (BiDi §10.2.1).
struct PreloadScript {
    /// Opaque preload-script identifier returned to the client.
    id: String,
    /// JS source text to evaluate before each new document loads.
    source: String,
    /// Contexts to which this script applies; empty = all contexts.
    contexts: Vec<String>,
}

/// Lifecycle state of a browser download item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DownloadState {
    /// Download in progress.
    InProgress,
    /// Download finished successfully.
    Completed,
    /// Download was aborted or failed.
    Aborted,
}

/// A download item tracked via `browser.download*` events (BiDi §7).
struct DownloadItem {
    /// Opaque download identifier.
    id: String,
    /// Source URL of the download.
    url: String,
    /// Suggested file name (from Content-Disposition or URL path).
    file_name: String,
    /// Expected total size in bytes; 0 if unknown.
    total_bytes: u64,
    /// Bytes received so far.
    received_bytes: u64,
    /// Current lifecycle state.
    state: DownloadState,
}

/// A cookie stored in the BiDi session (mirrors `storage.Cookie`, BiDi §13).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BidiCookie {
    /// Cookie name.
    name: String,
    /// Cookie string value.
    value: String,
    /// Domain (e.g. `"example.com"`).
    domain: String,
    /// Path component (default `"/"`).
    path: String,
    /// Secure flag — cookie sent only over HTTPS.
    secure: bool,
    /// HttpOnly flag — inaccessible to JS.
    http_only: bool,
    /// SameSite policy: `"strict"`, `"lax"`, or `"none"`.
    same_site: String,
    /// Unix timestamp expiry; 0 = session cookie.
    expiry: u64,
}

/// An open user-prompt dialog (alert / confirm / prompt / beforeUnload).
struct UserPrompt {
    /// Opaque prompt identifier.
    #[allow(dead_code)]
    id: String,
    /// ID of the browsing context that opened the prompt.
    context: String,
    /// Prompt type: `"alert"`, `"confirm"`, `"prompt"`, or `"beforeUnload"`.
    type_: String,
    /// Default value for `"prompt"` dialogs; empty for alert/confirm.
    #[allow(dead_code)]
    default_value: String,
    /// Prompt message text shown to the user.
    #[allow(dead_code)]
    message: String,
}

/// Connection-level BiDi state.
///
/// Holds the session, all browsing contexts, active event subscriptions,
/// buffered network response bodies, and network intercept rules.
/// Not shared between connections — each connection is fully isolated.
#[derive(Default)]
pub struct BidiState {
    /// Active session ID (`None` until `session.new` is called).
    session_id: Option<String>,
    /// All browsing contexts in this connection; first one is created with the session.
    contexts: Vec<BidiContext>,
    /// Active subscriptions: event name (`module.event`) or module (`module`).
    subscriptions: BTreeSet<String>,
    /// Monotonic counter for deterministic ID generation.
    counter: u64,
    /// Buffered response bodies: requestId → body (most recent).
    ///
    /// Populated via [`BidiState::record_response_body`] from the network layer.
    /// Queried by `network.getResponseBody`.
    response_bodies: HashMap<u64, Vec<u8>>,
    /// Default locale for user contexts (IETF BCP 47).
    ///
    /// Set by `session.setDefaultUserContextLocale`; `None` = browser system locale.
    default_locale: Option<String>,
    /// IANA timezone override identifier.
    ///
    /// Set by `browser.setTimezoneOverride`; `None` = system timezone.
    timezone_override: Option<String>,
    /// Permission states set by `permissions.setPermission` (name → state).
    permission_states: HashMap<String, String>,
    /// Offline network simulation: `true` = all network requests fail.
    ///
    /// Set by `network.setOfflineStatus`.
    offline: bool,
    /// Session-level User-Agent override; per-context overrides take priority.
    ///
    /// Set by `emulation.setUserAgentOverride` without `contexts`.
    session_ua_override: Option<String>,
    /// Registered network intercept rules — connection-local bookkeeping
    /// (`intercept_count()`); the actual matching copy lives in
    /// `lumen_network`'s process-global registry, see [`NetworkIntercept`].
    ///
    /// Populated by `network.addIntercept`, removed by `network.removeIntercept`.
    intercepts: Vec<NetworkIntercept>,
    /// Cache behaviour override: "default", "bypass", or "restore".
    ///
    /// Set by `network.setCacheBehavior`; `None` = default browser caching.
    cache_behavior: Option<String>,
    /// Registered preload scripts — evaluated before each new document (BiDi §10.2.1).
    ///
    /// Populated by `script.addPreloadScript`, removed by `script.removePreloadScript`.
    preload_scripts: Vec<PreloadScript>,
    /// Active browser download items keyed by their opaque ID (BiDi §7).
    ///
    /// Populated by [`BidiState::begin_download`]; updated/removed via the progress methods.
    download_items: HashMap<String, DownloadItem>,
    /// Cookies stored in this BiDi session (BiDi §13).
    ///
    /// Modified by `storage.setCookie` / `storage.deleteCookies`; queried by `storage.getCookies`.
    cookies: Vec<BidiCookie>,
    /// Open user-prompt dialogs (alert/confirm/prompt/beforeUnload, BiDi §6.8).
    ///
    /// Populated by [`BidiState::fire_user_prompt`]; dismissed via `browsingContext.handleUserPrompt`.
    user_prompts: Vec<UserPrompt>,
    /// Live shell window this connection drives (SDC-2), if one is available.
    ///
    /// `None` when `--bidi-port` was passed without opening a window (e.g.
    /// `--dump`/`--screenshot`/`--mcp`) — in that case `browsingContext.navigate`,
    /// `script.evaluate`, `browsingContext.captureScreenshot`, and
    /// `input.performActions` fall back to their Phase 1 in-memory stub behavior.
    live: Option<LiveWindowSession>,
    /// Token `session.new` must be given (ADR-024 §Access model, DEVX-15).
    /// `None` disables the check — the [`BidiState::new`] unit-test
    /// constructor never sets it, so the existing `dispatch()` tests are
    /// unaffected; production connections always carry `Some` (see
    /// `transport::handle`).
    required_token: Option<String>,
}

impl BidiState {
    /// Новое пустое состояние соединения (без живого окна — Phase 1 stub behavior).
    ///
    /// Production connections always go through [`BidiState::with_live_session`]
    /// (see `transport::handle`); this constructor is the unit-test entry point.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// State connected to a live shell window (SDC-2): real navigation,
    /// script evaluation, screenshots, and pointer/key input.
    ///
    /// `required_token` — see [`BidiState::required_token`]; `None` disables
    /// the `session.new` token check.
    pub fn with_live_session(live: LiveWindowSession, required_token: Option<String>) -> Self {
        Self { live: Some(live), required_token, ..Self::default() }
    }

    /// Сгенерировать псевдо-UUID из монотонного счётчика.
    ///
    /// Формат совпадает с UUID v4 по длине полей, но детерминирован —
    /// клиенты BiDi трактуют context/session/navigation id как непрозрачные
    /// строки, а детерминизм нужен для unit-тестов.
    pub(crate) fn next_id(&mut self, tag: u16) -> String {
        self.counter += 1;
        let n = self.counter;
        format!("{:08x}-{tag:04x}-4000-8000-{n:012x}", n as u32)
    }

    /// Подписан ли клиент на событие `method`.
    ///
    /// Совпадение по точному имени (`browsingContext.load`) ИЛИ по имени
    /// модуля (`browsingContext` подписывает на все его события) — BiDi
    /// допускает подписку как на конкретное событие, так и на весь модуль.
    fn is_subscribed(&self, method: &str) -> bool {
        if self.subscriptions.contains(method) {
            return true;
        }
        method
            .split_once('.')
            .is_some_and(|(module, _)| self.subscriptions.contains(module))
    }

    /// Найти контекст по id (`None`, если такого нет).
    fn find(&self, id: &str) -> Option<&BidiContext> {
        self.contexts.iter().find(|c| c.id == id)
    }

    /// Current default locale for user contexts (IETF BCP 47).
    ///
    /// `None` if not set by `session.setDefaultUserContextLocale`.
    // Shell layer reads this when initialising the JS engine context; not wired yet → false dead_code.
    #[allow(dead_code)]
    pub fn locale(&self) -> Option<&str> {
        self.default_locale.as_deref()
    }

    /// Current timezone override (IANA identifier).
    ///
    /// `None` if not set by `browser.setTimezoneOverride`.
    // Shell layer passes this to the JS engine at init; not wired yet → false dead_code.
    #[allow(dead_code)]
    pub fn timezone(&self) -> Option<&str> {
        self.timezone_override.as_deref()
    }

    /// State set for permission `name` by `permissions.setPermission`.
    #[allow(dead_code)]
    pub fn permission_state(&self, name: &str) -> Option<&str> {
        self.permission_states.get(name).map(String::as_str)
    }

    /// Whether offline network simulation is active.
    // Shell layer blocks network requests when offline=true; not wired yet → false dead_code.
    #[allow(dead_code)]
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Effective User-Agent for a context: per-context → session-level → `None`.
    ///
    /// BUG-295: also used by [`emulation_set_ua_override`] to compute the
    /// effective UA pushed to the live window.
    pub fn user_agent_for(&self, context_id: &str) -> Option<&str> {
        self.contexts
            .iter()
            .find(|c| c.id == context_id)
            .and_then(|c| c.ua_override.as_deref())
            .or(self.session_ua_override.as_deref())
    }

    /// Viewport dimensions `(width, height)` in CSS pixels for `context_id`.
    ///
    /// `None` if `browsingContext.setViewport` was not yet called for this context.
    // Shell layer applies this to the windowing layer when resizing the content area; not wired yet.
    #[allow(dead_code)]
    pub fn viewport_for(&self, context_id: &str) -> Option<(u32, u32)> {
        self.contexts
            .iter()
            .find(|c| c.id == context_id)
            .and_then(|c| c.viewport)
    }

    /// Active cache behavior override: "default", "bypass", or "restore".
    ///
    /// `None` if not set by `network.setCacheBehavior`.
    // Shell layer applies this to the HTTP client cache policy; not wired yet → false dead_code.
    #[allow(dead_code)]
    pub fn cache_behavior(&self) -> Option<&str> {
        self.cache_behavior.as_deref()
    }

    /// Number of active network intercept rules.
    // Used in tests and future shell integration.
    #[allow(dead_code)]
    pub fn intercept_count(&self) -> usize {
        self.intercepts.len()
    }

    /// Return preload scripts that apply to `context_id`.
    ///
    /// A script applies if its `contexts` list is empty (global) or contains `context_id`.
    /// Callers should evaluate these scripts at document-creation time — 8H.3 wiring.
    #[allow(dead_code)]
    pub fn preload_scripts_for_context(&self, context_id: &str) -> Vec<&str> {
        self.preload_scripts
            .iter()
            .filter(|s| s.contexts.is_empty() || s.contexts.iter().any(|c| c == context_id))
            .map(|s| s.source.as_str())
            .collect()
    }

    /// Register a new download and emit `browser.downloadWillBegin` if subscribed.
    ///
    /// Returns the opaque download ID and the serialised event frames (may be empty).
    #[allow(dead_code)]
    pub fn begin_download(&mut self, url: String, file_name: String) -> (String, Vec<String>) {
        let item_id = self.next_id(0xd15c);
        let item = DownloadItem {
            id: item_id.clone(),
            url: url.clone(),
            file_name: file_name.clone(),
            total_bytes: 0,
            received_bytes: 0,
            state: DownloadState::InProgress,
        };
        self.download_items.insert(item_id.clone(), item);

        let frames = if self.is_subscribed("browser.downloadWillBegin") {
            vec![make_event(
                "browser.downloadWillBegin",
                download_event_params(&item_id, &url, &file_name, 0, 0, "inProgress"),
            )]
        } else {
            vec![]
        };
        (item_id, frames)
    }

    /// Update download progress and emit `browser.downloadItemUpdated` if subscribed.
    ///
    /// Returns event frames (may be empty if not subscribed or item unknown).
    #[allow(dead_code)]
    pub fn update_download(
        &mut self,
        item_id: &str,
        received_bytes: u64,
        total_bytes: u64,
    ) -> Vec<String> {
        let Some(item) = self.download_items.get_mut(item_id) else {
            return vec![];
        };
        item.received_bytes = received_bytes;
        item.total_bytes = total_bytes;
        let (url, file_name) = (item.url.clone(), item.file_name.clone());

        if self.is_subscribed("browser.downloadItemUpdated") {
            vec![make_event(
                "browser.downloadItemUpdated",
                download_event_params(item_id, &url, &file_name, received_bytes, total_bytes, "inProgress"),
            )]
        } else {
            vec![]
        }
    }

    /// Mark download as completed and emit `browser.downloadItemCompleted` if subscribed.
    #[allow(dead_code)]
    pub fn complete_download(&mut self, item_id: &str) -> Vec<String> {
        let Some(item) = self.download_items.get_mut(item_id) else {
            return vec![];
        };
        item.state = DownloadState::Completed;
        let (url, file_name, received, total) =
            (item.url.clone(), item.file_name.clone(), item.received_bytes, item.total_bytes);

        if self.is_subscribed("browser.downloadItemCompleted") {
            vec![make_event(
                "browser.downloadItemCompleted",
                download_event_params(item_id, &url, &file_name, received, total, "completed"),
            )]
        } else {
            vec![]
        }
    }

    /// Mark download as aborted and emit `browser.downloadItemAborted` if subscribed.
    #[allow(dead_code)]
    pub fn abort_download(&mut self, item_id: &str) -> Vec<String> {
        let Some(item) = self.download_items.get_mut(item_id) else {
            return vec![];
        };
        item.state = DownloadState::Aborted;
        let (url, file_name, received, total) =
            (item.url.clone(), item.file_name.clone(), item.received_bytes, item.total_bytes);

        if self.is_subscribed("browser.downloadItemAborted") {
            vec![make_event(
                "browser.downloadItemAborted",
                download_event_params(item_id, &url, &file_name, received, total, "aborted"),
            )]
        } else {
            vec![]
        }
    }

    /// Record a cookie change (add/update/remove) and emit `storage.cookie*` events.
    ///
    /// `action` must be `"added"`, `"changed"`, or `"removed"`.
    /// Returns event frames to send to the client (empty if not subscribed).
    #[allow(dead_code)]
    pub fn record_cookie_change(&mut self, action: &str, cookie: BidiCookie) -> Vec<String> {
        match action {
            "added" => {
                self.cookies.push(cookie.clone());
                if self.is_subscribed("storage.cookieAdded") {
                    vec![make_event("storage.cookieAdded", cookie_event_params(&cookie))]
                } else {
                    vec![]
                }
            }
            "changed" => {
                if let Some(existing) = self
                    .cookies
                    .iter_mut()
                    .find(|c| c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path)
                {
                    *existing = cookie.clone();
                } else {
                    self.cookies.push(cookie.clone());
                }
                if self.is_subscribed("storage.cookieChanged") {
                    vec![make_event("storage.cookieChanged", cookie_event_params(&cookie))]
                } else {
                    vec![]
                }
            }
            "removed" => {
                self.cookies.retain(|c| {
                    !(c.name == cookie.name
                        && c.domain == cookie.domain
                        && c.path == cookie.path)
                });
                if self.is_subscribed("storage.cookieRemoved") {
                    vec![make_event("storage.cookieRemoved", cookie_event_params(&cookie))]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    /// Open a user-prompt dialog and emit `browsingContext.userPromptOpened` if subscribed.
    ///
    /// `prompt_type`: `"alert"`, `"confirm"`, `"prompt"`, or `"beforeUnload"`.
    /// Returns `(prompt_id, event_frames)`.
    #[allow(dead_code)]
    pub fn fire_user_prompt(
        &mut self,
        context_id: &str,
        prompt_type: &str,
        message: &str,
        default_value: &str,
    ) -> (String, Vec<String>) {
        let prompt_id = self.next_id(0xd1a1);
        self.user_prompts.push(UserPrompt {
            id: prompt_id.clone(),
            context: context_id.to_owned(),
            type_: prompt_type.to_owned(),
            default_value: default_value.to_owned(),
            message: message.to_owned(),
        });

        let frames = if self.is_subscribed("browsingContext.userPromptOpened") {
            let mut params = BTreeMap::new();
            params.insert("context".into(), JsonValue::String(context_id.to_owned()));
            params.insert("type".into(), JsonValue::String(prompt_type.to_owned()));
            params.insert("message".into(), JsonValue::String(message.to_owned()));
            if prompt_type == "prompt" {
                params.insert(
                    "defaultValue".into(),
                    JsonValue::String(default_value.to_owned()),
                );
            }
            vec![make_event(
                "browsingContext.userPromptOpened",
                JsonValue::Object(params),
            )]
        } else {
            vec![]
        };
        (prompt_id, frames)
    }

    /// Number of currently open user prompts (for testing).
    #[allow(dead_code)]
    pub fn open_prompt_count(&self) -> usize {
        self.user_prompts.len()
    }

    /// Number of cookies in the session (for testing).
    #[allow(dead_code)]
    pub fn cookie_count(&self) -> usize {
        self.cookies.len()
    }

    /// Number of active download items.
    #[allow(dead_code)]
    pub fn download_count(&self) -> usize {
        self.download_items.len()
    }

    /// Number of registered preload scripts.
    #[allow(dead_code)]
    pub fn preload_script_count(&self) -> usize {
        self.preload_scripts.len()
    }

    /// Буферизовать тело сетевого ответа и эмитировать `network.responseBodyReceived`.
    ///
    /// Вызывается из сетевого слоя при получении тела ответа. Возвращает BiDi-фреймы
    /// для отправки клиенту (пустой вектор, если подписки нет).
    /// Повторный вызов с тем же `request_id` перезаписывает предыдущее тело.
    // Вызывается из network-слоя при 8H.3 live-wiring; пока stub-слой не подключён,
    // сигнал dead_code ложный.
    #[allow(dead_code)]
    pub fn record_response_body(&mut self, request_id: u64, body: Vec<u8>) -> Vec<String> {
        self.response_bodies.insert(request_id, body.clone());
        if self.is_subscribed("network.responseBodyReceived") {
            vec![make_event(
                "network.responseBodyReceived",
                network_response_body_event_params(request_id, &body),
            )]
        } else {
            vec![]
        }
    }
}

/// Результат обработки одной команды.
pub struct DispatchResult {
    /// Фреймы для последовательной отправки клиенту (ответ + события).
    pub frames: Vec<String>,
    /// Закрыть ли соединение после отправки фреймов (`session.end`).
    pub close: bool,
}

impl DispatchResult {
    /// Один кадр-ответ, без закрытия соединения.
    fn single(frame: String) -> Self {
        DispatchResult { frames: vec![frame], close: false }
    }
}

/// Обработать одно BiDi-сообщение, вернуть фреймы для отправки клиенту.
pub fn dispatch(message: &str, state: &mut BidiState) -> DispatchResult {
    let val = match parse_json(message) {
        Ok(v) => v,
        Err(e) => {
            return DispatchResult::single(make_error(
                None,
                "invalid argument",
                &format!("parse error: {e}"),
            ));
        }
    };

    let id = val.get("id").and_then(|v| v.as_number()).map(|n| n as i64);
    let method = val.get("method").and_then(|v| v.as_str());
    let params = val.get("params").cloned().unwrap_or(JsonValue::Null);

    let Some(id) = id else {
        return DispatchResult::single(make_error(
            None,
            "invalid argument",
            "missing or non-integer id",
        ));
    };
    let Some(method) = method else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing method"));
    };

    let mut result = match method {
        "session.status" => DispatchResult::single(make_success(id, session_status_result(state))),
        "session.new" => session_new(id, &params, state),
        "session.subscribe" => session_subscribe(id, &params, state),
        "session.unsubscribe" => session_unsubscribe(id, &params, state),
        "session.end" => {
            DispatchResult { frames: vec![make_success(id, empty_obj())], close: true }
        }
        "browsingContext.create" => bc_create(id, &params, state),
        "browsingContext.close" => bc_close(id, &params, state),
        "browsingContext.navigate" => bc_navigate(id, &params, state),
        "browsingContext.activate" => bc_activate(id, &params, state),
        "browsingContext.getTree" => {
            DispatchResult::single(make_success(id, browsing_context_tree(state)))
        }
        "browsingContext.captureScreenshot" => bc_capture_screenshot(id, &params, state),
        "script.evaluate" => script_evaluate(id, &params, state),
        "script.callFunction" => script_call_function(id, &params, state),
        "script.addPreloadScript" => script_add_preload(id, &params, state),
        "script.removePreloadScript" => script_remove_preload(id, &params, state),
        "script.disown" => script_disown(id, &params),
        "script.getRealms" => script_get_realms(id, state),
        "network.getResponseBody" => network_get_response_body(id, &params, state),
        "network.setOfflineStatus" => network_set_offline(id, &params, state),
        "network.addIntercept" => network_add_intercept(id, &params, state),
        "network.removeIntercept" => network_remove_intercept(id, &params, state),
        "network.continueRequest" => network_continue_request(id, &params, state),
        "network.continueResponse" => network_continue_response(id, &params, state),
        // `authRequired` is rejected by `addIntercept`, so no request can ever
        // be blocked on auth — the spec's answer for such a request id.
        "network.continueWithAuth" => DispatchResult::single(make_error(
            Some(id),
            "no such request",
            "no request is blocked at the authRequired phase (unsupported by this browser)",
        )),
        "network.failRequest" => network_fail_request(id, &params, state),
        "network.setCacheBehavior" => network_set_cache_behavior(id, &params, state),
        "input.performActions" => input_perform_actions(id, &params, state),
        "input.releaseActions" => input_release_actions(id, &params, state),
        "input.setFiles" => input_set_files(id, &params, state),
        "session.setDefaultUserContextLocale" => session_set_locale(id, &params, state),
        "browser.setTimezoneOverride" => browser_set_timezone(id, &params, state),
        "browser.getDownloads" => browser_get_downloads(id, state),
        "emulation.setUserAgentOverride" => emulation_set_ua_override(id, &params, state),
        "browsingContext.handleUserPrompt" => bc_handle_user_prompt(id, &params, state),
        "browsingContext.setViewport" => bc_set_viewport(id, &params, state),
        "permissions.setPermission" => permissions_set_permission(id, &params, state),
        "lumen.getComputedA11y" => lumen_get_computed_a11y(id, &params, state),
        "storage.getCookies" => storage_get_cookies(id, &params, state),
        "storage.setCookie" => storage_set_cookie(id, &params, state),
        "storage.deleteCookies" => storage_delete_cookies(id, &params, state),
        other => DispatchResult::single(make_error(
            Some(id),
            "unknown command",
            &format!("unknown command: {other}"),
        )),
    };

    // BUG-295 remainder: this transport has no spontaneous event-push path
    // (one connection = one blocking read-dispatch-write loop, see
    // `transport::handle`) — every event, including this one, can only ride
    // along on the response to SOME incoming command. So opportunistically
    // drain any requests newly paused by an active intercept (on whatever
    // thread is running the matching fetch) and prepend `network.
    // beforeRequestSent` / `network.responseStarted` events (by the phase the
    // request is paused at, DEVX-20) ahead of `method`'s own response frames,
    // regardless of which command triggered this dispatch call. A client
    // that polls at all (as any BiDi automation client driving a paused
    // request must) observes the event within its next round-trip. A pause
    // whose phase event nobody subscribed to is drained and dropped.
    if (state.is_subscribed("network.beforeRequestSent") || state.is_subscribed("network.responseStarted"))
        && let Some(live) = &mut state.live
        && let Ok(requests) = live.poll_intercepted_requests()
    {
        for req in requests.iter().rev() {
            let event = if req.phase == "responseStarted" {
                "network.responseStarted"
            } else {
                "network.beforeRequestSent"
            };
            if !state.is_subscribed(event) {
                continue;
            }
            let frame = make_event(event, before_request_sent_params(state, req));
            result.frames.insert(0, frame);
        }
    }
    result
}

/// Parameters for the `network.beforeRequestSent` / `network.responseStarted`
/// events (BiDi §12.5.1 / §12.5.3, simplified — BUG-295 remainder, DEVX-20;
/// the latter adds a `response` object with the real status). `context` is the first browsing context
/// in this connection (Lumen runs one window per process, so there is
/// exactly one meaningful context to attribute a live-window request to).
fn before_request_sent_params(state: &BidiState, req: &InterceptedRequest) -> JsonValue {
    let mut request_data = BTreeMap::new();
    request_data.insert("request".into(), JsonValue::String(req.request_id.clone()));
    request_data.insert("url".into(), JsonValue::String(req.url.clone()));
    request_data.insert("method".into(), JsonValue::String("GET".into()));
    request_data.insert("headersSize".into(), JsonValue::Number(-1.0));
    request_data.insert("bodySize".into(), JsonValue::Null);

    let mut params = BTreeMap::new();
    if let Some(ctx) = state.contexts.first() {
        params.insert("context".into(), JsonValue::String(ctx.id.clone()));
    }
    params.insert("isBlocked".into(), JsonValue::Bool(true));
    params.insert("navigation".into(), JsonValue::Null);
    params.insert("redirectCount".into(), JsonValue::Number(0.0));
    params.insert("request".into(), JsonValue::Object(request_data));
    params.insert("timestamp".into(), JsonValue::Number(unix_timestamp_ms()));
    if req.phase == "responseStarted" {
        let mut response = BTreeMap::new();
        response.insert("url".into(), JsonValue::String(req.url.clone()));
        response.insert("status".into(), JsonValue::Number(f64::from(req.status.unwrap_or(0))));
        response.insert("fromCache".into(), JsonValue::Bool(false));
        response.insert("headers".into(), JsonValue::Array(Vec::new()));
        response.insert("headersSize".into(), JsonValue::Number(-1.0));
        response.insert("bodySize".into(), JsonValue::Null);
        params.insert("response".into(), JsonValue::Object(response));
    }
    JsonValue::Object(params)
}

/// `session.status` — готовность к созданию новой сессии.
///
/// BiDi допускает лишь одну сессию на соединение: после `session.new`
/// `ready` становится `false`.
fn session_status_result(state: &BidiState) -> JsonValue {
    let ready = state.session_id.is_none();
    let mut obj = BTreeMap::new();
    obj.insert("ready".into(), JsonValue::Bool(ready));
    obj.insert(
        "message".into(),
        JsonValue::String(
            if ready { "ready for new session" } else { "session already created" }.into(),
        ),
    );
    JsonValue::Object(obj)
}

/// `session.new` — создать сессию и дефолтный browsing context.
///
/// Возвращает успешный ответ с `sessionId` + минимальными capabilities.
/// Событие `browsingContext.created` НЕ эмитится: на момент `session.new`
/// клиент ещё не подписан (BiDi-порядок — сначала сессия, затем `subscribe`).
///
/// If [`BidiState::required_token`] is set (ADR-024 §Access model, DEVX-15),
/// `params.capabilities.alwaysMatch.token` must match it — the standard
/// WebDriver extension-capability slot. No session (and thus no browsing
/// context) is created on mismatch.
fn session_new(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    if state.session_id.is_some() {
        return DispatchResult::single(make_error(
            Some(id),
            "session not created",
            "session already exists",
        ));
    }

    if let Some(expected) = &state.required_token {
        let provided = params
            .get("capabilities")
            .and_then(|c| c.get("alwaysMatch"))
            .and_then(|a| a.get("token"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !lumen_core::auth::tokens_match(expected, provided) {
            return DispatchResult::single(make_error(
                Some(id),
                "session not created",
                "invalid or missing capabilities.alwaysMatch.token",
            ));
        }
    }

    let session_id = state.next_id(0x5e55);
    let context_id = state.next_id(0xc047);
    state.session_id = Some(session_id.clone());
    state.contexts.push(BidiContext {
        id: context_id,
        url: "about:blank".into(),
        parent: None,
        ua_override: None,
        viewport: None,
    });

    let mut result = BTreeMap::new();
    result.insert("sessionId".into(), JsonValue::String(session_id));
    result.insert("capabilities".into(), capabilities());

    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `session.subscribe` — сохранить набор подписок на события.
///
/// `params.events` — массив строк (`module.event` или `module`). Несуществующие
/// `params.events` → пустой массив (BiDi требует поле, но трактуем мягко).
fn session_subscribe(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    for e in event_names(params) {
        state.subscriptions.insert(e);
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `session.unsubscribe` — удалить подписки по именам событий/модулей.
fn session_unsubscribe(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    for e in event_names(params) {
        state.subscriptions.remove(&e);
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// Извлечь `params.events` как `Vec<String>` (пустой, если поле отсутствует).
fn event_names(params: &JsonValue) -> Vec<String> {
    params
        .get("events")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// `browsingContext.create` — создать новый browsing context.
///
/// `params.referenceContext` (опц.) задаёт родителя; если указан, но не найден —
/// ошибка `no such frame`. Эмитит `browsingContext.created`, если подписан.
#[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
fn bc_create(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    if state.session_id.is_none() {
        return DispatchResult::single(make_error(Some(id), "session not created", "no active session"));
    }

    let parent = params.get("referenceContext").and_then(|v| v.as_str()).map(str::to_owned);
    if let Some(ref p) = parent
        && state.find(p).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {p}"),
        ));
    }

    let context_id = state.next_id(0xc047);
    state.contexts.push(BidiContext {
        id: context_id.clone(),
        url: "about:blank".into(),
        parent,
        ua_override: None,
        viewport: None,
    });

    // Collect applicable preload script IDs for this context (BiDi §10.2.1).
    let script_ids: Vec<JsonValue> = state
        .preload_scripts
        .iter()
        .filter(|s| s.contexts.is_empty() || s.contexts.iter().any(|c| c == &context_id))
        .map(|s| JsonValue::String(s.id.clone()))
        .collect();

    let mut result = BTreeMap::new();
    result.insert("context".into(), JsonValue::String(context_id.clone()));
    result.insert("preloadScripts".into(), JsonValue::Array(script_ids));
    let mut frames = vec![make_success(id, JsonValue::Object(result))];

    if state.is_subscribed("browsingContext.created") {
        let ctx = state.find(&context_id).expect("just inserted");
        frames.push(make_event("browsingContext.created", context_info_flat(ctx)));
    }
    DispatchResult { frames, close: false }
}

/// `browsingContext.close` — закрыть контекст и его прямых потомков.
///
/// Эмитит `browsingContext.contextDestroyed` для закрытого контекста, если
/// подписан. Потомки удаляются каскадно (без отдельных событий — упрощение
/// stub-слоя; полный per-node teardown — handoff P3).
fn bc_close(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(cid) = params.get("context").and_then(|v| v.as_str()).map(str::to_owned) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing context"));
    };
    let Some(ctx) = state.find(&cid) else {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {cid}"),
        ));
    };

    // Снимок инфо до удаления — для события contextDestroyed.
    let destroyed_info = context_info_flat(ctx);
    let subscribed = state.is_subscribed("browsingContext.contextDestroyed");

    // Каскадно удаляем контекст и его прямых потомков.
    state.contexts.retain(|c| c.id != cid && c.parent.as_deref() != Some(cid.as_str()));

    let mut frames = vec![make_success(id, empty_obj())];
    if subscribed {
        frames.push(make_event("browsingContext.contextDestroyed", destroyed_info));
    }
    DispatchResult { frames, close: false }
}

/// `browsingContext.navigate` — обновить URL контекста.
///
/// Возвращает `{navigation, url}`. Когда соединение привязано к живому окну
/// (SDC-2, `--bidi-port` + окно), реально загружает страницу через
/// `LiveWindowSession::navigate`, затем блокируется на
/// `LiveWindowSession::wait(WaitCondition::DocumentReady, …)` — реальном
/// `document.readyState == "complete"` от JS-рантайма (P2-wpt S1) — прежде
/// чем ответить клиенту; ошибка навигации или таймаут ожидания возвращаются
/// как BiDi `unknown error`. Без живого окна — только bookkeeping контекста
/// (Phase 1 stub), event эмитится немедленно. Эмитит `browsingContext.load`
/// с реальным Unix-timestamp (мс), если подписан.
fn bc_navigate(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(cid) = params.get("context").and_then(|v| v.as_str()).map(str::to_owned) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing context"));
    };
    let Some(url) = params.get("url").and_then(|v| v.as_str()).map(str::to_owned) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing url"));
    };
    if state.find(&cid).is_none() {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {cid}"),
        ));
    }

    if let Some(live) = &mut state.live {
        if let Err(e) = live.navigate(&url) {
            return DispatchResult::single(make_error(Some(id), "unknown error", &format!("navigate: {e}")));
        }
        if let Err(e) = live.wait(WaitCondition::DocumentReady, NAVIGATE_LOAD_TIMEOUT_MS) {
            return DispatchResult::single(make_error(Some(id), "unknown error", &format!("navigate: {e}")));
        }
    }

    let navigation_id = state.next_id(0x4a71);
    // Обновляем URL контекста.
    if let Some(ctx) = state.contexts.iter_mut().find(|c| c.id == cid) {
        ctx.url = url.clone();
    }

    let mut result = BTreeMap::new();
    result.insert("navigation".into(), JsonValue::String(navigation_id.clone()));
    result.insert("url".into(), JsonValue::String(url.clone()));
    let mut frames = vec![make_success(id, JsonValue::Object(result))];

    if state.is_subscribed("browsingContext.load") {
        let mut ev = BTreeMap::new();
        ev.insert("context".into(), JsonValue::String(cid));
        ev.insert("navigation".into(), JsonValue::String(navigation_id));
        ev.insert("url".into(), JsonValue::String(url));
        ev.insert("timestamp".into(), JsonValue::Number(unix_timestamp_ms()));
        frames.push(make_event("browsingContext.load", JsonValue::Object(ev)));
    }
    DispatchResult { frames, close: false }
}

/// Current Unix timestamp in milliseconds, for BiDi event `timestamp` fields
/// (e.g. `browsingContext.load`). Falls back to `0.0` only if the system
/// clock is set before the Unix epoch, which never happens in practice.
fn unix_timestamp_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// `browsingContext.activate` — пометить контекст активным (ACK).
///
/// Валидирует существование контекста; реальной активации окна на этом слое
/// нет (handoff P3).
fn bc_activate(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(cid) = params.get("context").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing context"));
    };
    if state.find(cid).is_none() {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {cid}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `browsingContext.captureScreenshot` — снимок текущей страницы (BiDi §6.6.1, SDC-2).
///
/// Возвращает `{data: <base64 PNG>}`. Требует живое окно (SDC-2); без него —
/// `unknown error` (Phase 1 не может растеризовать без реального рендерера).
fn bc_capture_screenshot(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    if let Some(cid) = params.get("context").and_then(|v| v.as_str())
        && state.find(cid).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {cid}"),
        ));
    }

    let Some(live) = &state.live else {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            "captureScreenshot requires a live window (--bidi-port with an open window)",
        ));
    };

    match live.screenshot() {
        Ok(png) => {
            let mut result = BTreeMap::new();
            result.insert("data".into(), JsonValue::String(lumen_core::hash::base64_encode(&png)));
            DispatchResult::single(make_success(id, JsonValue::Object(result)))
        }
        Err(e) => DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("captureScreenshot: {e}"),
        )),
    }
}

/// Capabilities, которые движок объявляет клиенту (BiDi §session.new).
fn capabilities() -> JsonValue {
    let mut caps = BTreeMap::new();
    caps.insert("acceptInsecureCerts".into(), JsonValue::Bool(false));
    caps.insert("browserName".into(), JsonValue::String("Lumen".into()));
    caps.insert(
        "browserVersion".into(),
        JsonValue::String(env!("CARGO_PKG_VERSION").into()),
    );
    caps.insert(
        "platformName".into(),
        JsonValue::String(std::env::consts::OS.into()),
    );
    caps.insert("setWindowRect".into(), JsonValue::Bool(false));
    // BUG-1614: the UA the network actually sends (default profile), not a Lumen token.
    caps.insert("userAgent".into(), JsonValue::String(lumen_core::chrome_user_agent().into()));
    JsonValue::Object(caps)
}

/// `BrowsingContextInfo` с `children:null` — для событий created/destroyed.
fn context_info_flat(ctx: &BidiContext) -> JsonValue {
    let mut info = BTreeMap::new();
    info.insert("context".into(), JsonValue::String(ctx.id.clone()));
    info.insert("url".into(), JsonValue::String(ctx.url.clone()));
    info.insert("children".into(), JsonValue::Null);
    info.insert(
        "parent".into(),
        ctx.parent.clone().map_or(JsonValue::Null, JsonValue::String),
    );
    info.insert("userContext".into(), JsonValue::String("default".into()));
    info.insert("originalOpener".into(), JsonValue::Null);
    info.insert("clientWindow".into(), JsonValue::String(CLIENT_WINDOW_ID.into()));
    info.insert("viewport".into(), viewport_json(ctx.viewport));
    JsonValue::Object(info)
}

/// `BrowsingContextInfo` с вложенными `children` — для `getTree`.
///
/// Рекурсивно собирает потомков (по `parent == ctx.id`).
fn context_info_tree(state: &BidiState, ctx: &BidiContext) -> JsonValue {
    let children: Vec<JsonValue> = state
        .contexts
        .iter()
        .filter(|c| c.parent.as_deref() == Some(ctx.id.as_str()))
        .map(|c| context_info_tree(state, c))
        .collect();

    let mut info = BTreeMap::new();
    info.insert("context".into(), JsonValue::String(ctx.id.clone()));
    info.insert("url".into(), JsonValue::String(ctx.url.clone()));
    info.insert("children".into(), JsonValue::Array(children));
    info.insert(
        "parent".into(),
        ctx.parent.clone().map_or(JsonValue::Null, JsonValue::String),
    );
    info.insert("userContext".into(), JsonValue::String("default".into()));
    info.insert("originalOpener".into(), JsonValue::Null);
    info.insert("clientWindow".into(), JsonValue::String(CLIENT_WINDOW_ID.into()));
    info.insert("viewport".into(), viewport_json(ctx.viewport));
    JsonValue::Object(info)
}

/// `browsingContext.getTree` — `{"contexts":[BrowsingContextInfo...]}`.
///
/// Возвращает top-level контексты (parent == None); потомки вложены в `children`.
fn browsing_context_tree(state: &BidiState) -> JsonValue {
    let contexts: Vec<JsonValue> = state
        .contexts
        .iter()
        .filter(|c| c.parent.is_none())
        .map(|c| context_info_tree(state, c))
        .collect();
    let mut obj = BTreeMap::new();
    obj.insert("contexts".into(), JsonValue::Array(contexts));
    JsonValue::Object(obj)
}

/// Пустой JSON-объект `{}` — типичный ACK-результат.
fn empty_obj() -> JsonValue {
    JsonValue::Object(BTreeMap::new())
}

/// Сериализовать успешный ответ `{"type":"success","id":id,"result":result}`.
fn make_success(id: i64, result: JsonValue) -> String {
    let mut obj = BTreeMap::new();
    obj.insert("type".into(), JsonValue::String("success".into()));
    obj.insert("id".into(), JsonValue::Number(id as f64));
    obj.insert("result".into(), result);
    JsonValue::Object(obj).to_string()
}

/// Сериализовать событие `{"type":"event","method":method,"params":params}`.
fn make_event(method: &str, params: JsonValue) -> String {
    let mut obj = BTreeMap::new();
    obj.insert("type".into(), JsonValue::String("event".into()));
    obj.insert("method".into(), JsonValue::String(method.into()));
    obj.insert("params".into(), params);
    JsonValue::Object(obj).to_string()
}

/// Сериализовать ошибку. `id` = `None` → `null` (parse error до чтения id).
fn make_error(id: Option<i64>, code: &str, message: &str) -> String {
    let mut obj = BTreeMap::new();
    obj.insert("type".into(), JsonValue::String("error".into()));
    obj.insert(
        "id".into(),
        id.map_or(JsonValue::Null, |n| JsonValue::Number(n as f64)),
    );
    obj.insert("error".into(), JsonValue::String(code.into()));
    obj.insert("message".into(), JsonValue::String(message.into()));
    obj.insert("stacktrace".into(), JsonValue::String(String::new()));
    JsonValue::Object(obj).to_string()
}

// ──────────────────────────────────────────
// script.* handlers (BiDi §10)
// ──────────────────────────────────────────

/// `script.evaluate` — выполнить JS expression в browsing context (BiDi §10.2.4).
///
/// When bound to a live window (SDC-2), runs `params.expression` through
/// `LiveWindowSession::eval` and maps the JSON result to a best-effort BiDi
/// `RemoteValue` (primitives get their proper `type` tag; arrays/objects fall
/// back to a `"string"` value carrying their JSON text — full `RemoteValue`
/// serialization is future work). Without a live window, or when
/// `expression` is absent (e.g. forwarded from `script.callFunction`),
/// returns the Phase 1 `{type:"undefined"}` stub.
fn script_evaluate(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let ctx_id = params
        .get("target")
        .and_then(|t| t.get("context"))
        .and_then(|v| v.as_str());

    if let Some(ctx_id) = ctx_id
        && state.find(ctx_id).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("unknown browsing context: {ctx_id}"),
        ));
    }

    // BiDi §10.2.4: `awaitPromise:true` means a promise-valued result must be
    // awaited and its resolved value returned (or a `javascript error` on
    // rejection). Defaults to `false` when absent/non-boolean.
    let await_promise = params
        .get("awaitPromise")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let expression = params.get("expression").and_then(|v| v.as_str());
    let result = match (expression, &mut state.live) {
        (Some(expr), Some(live)) if await_promise => match eval_await_promise(live, expr) {
            Ok(rv) => rv,
            Err(e) => {
                return DispatchResult::single(make_error(Some(id), "javascript error", &e));
            }
        },
        (Some(expr), Some(live)) => match live.eval(expr) {
            Ok(json) => eval_result_to_remote_value(&json),
            Err(e) => {
                return DispatchResult::single(make_error(
                    Some(id),
                    "javascript error",
                    &format!("eval: {e}"),
                ));
            }
        },
        _ => undefined_remote_value(),
    };

    let mut outer = BTreeMap::new();
    outer.insert("type".into(), JsonValue::String("success".into()));
    outer.insert("result".into(), result);
    outer.insert("realm".into(), JsonValue::String("stub-realm".into()));

    DispatchResult::single(make_success(id, JsonValue::Object(outer)))
}

/// Map an `eval()` JSON-string result to a best-effort BiDi `RemoteValue`.
///
/// Primitives (`null`/`bool`/`number`/`string`) get their proper BiDi `type`
/// tag. Arrays and objects — and unparseable input — fall back to a
/// `"string"` RemoteValue carrying the raw JSON text; full structural
/// `RemoteValue` serialization (BiDi §10.1.2 LocalValue tables) is future work.
fn eval_result_to_remote_value(json: &str) -> JsonValue {
    match parse_json(json) {
        Ok(v) => remote_value_from_json(&v),
        Err(_) => {
            let mut obj = BTreeMap::new();
            obj.insert("type".into(), JsonValue::String("string".into()));
            obj.insert("value".into(), JsonValue::String(json.to_owned()));
            JsonValue::Object(obj)
        }
    }
}

/// Map an already-parsed JSON value to a best-effort BiDi `RemoteValue`
/// (same primitive-vs-fallback rules as [`eval_result_to_remote_value`]).
fn remote_value_from_json(v: &JsonValue) -> JsonValue {
    let mut obj = BTreeMap::new();
    match v {
        JsonValue::Null => {
            obj.insert("type".into(), JsonValue::String("null".into()));
        }
        JsonValue::Bool(b) => {
            obj.insert("type".into(), JsonValue::String("boolean".into()));
            obj.insert("value".into(), JsonValue::Bool(*b));
        }
        JsonValue::Number(n) => {
            obj.insert("type".into(), JsonValue::String("number".into()));
            obj.insert("value".into(), JsonValue::Number(*n));
        }
        JsonValue::String(s) => {
            obj.insert("type".into(), JsonValue::String("string".into()));
            obj.insert("value".into(), JsonValue::String(s.clone()));
        }
        other => {
            obj.insert("type".into(), JsonValue::String("string".into()));
            obj.insert("value".into(), JsonValue::String(other.to_string()));
        }
    }
    JsonValue::Object(obj)
}

/// The BiDi `{type:"undefined"}` RemoteValue.
fn undefined_remote_value() -> JsonValue {
    let mut obj = BTreeMap::new();
    obj.insert("type".into(), JsonValue::String("undefined".into()));
    JsonValue::Object(obj)
}

/// Await a promise-valued expression for BiDi `script.evaluate` with
/// `awaitPromise:true` (BiDi §10.2.4).
///
/// `AutomationCommand::Eval` is synchronous and snapshots the expression's
/// value before the microtask queue drains, so a promise-valued expression
/// evaluates to the unsettled promise object. V8, however, auto-runs a
/// microtask checkpoint at the end of each eval, so this uses two eval
/// round-trips:
///
/// 1. wrap the expression in `Promise.resolve(...)` and attach a settle
///    handler that records `{state, value|error}` on a well-known global. The
///    checkpoint runs the handler before round 1 returns.
/// 2. read that global back and translate it into a `RemoteValue` (fulfilled)
///    or an `Err` message (rejected → the caller emits a `javascript error`).
///
/// Only microtask-settleable promises resolve this way; a promise still pending
/// on a macrotask/IO reports `state:"pending"` and falls back to the promise
/// object (`{type:"string", value:"{}"}`), matching the non-awaited path. This
/// limitation is acceptable because the awaited-value use cases (`Promise.resolve`,
/// sync `async` functions) settle purely on microtasks.
fn eval_await_promise(live: &mut LiveWindowSession, expr: &str) -> Result<JsonValue, String> {
    // Round 1: evaluate the expression, record its settled outcome on a global.
    let setup = format!(
        "(function(){{\
             globalThis.__lumen_bidi_await = {{ state: \"pending\" }};\
             try {{\
                 Promise.resolve(({expr})).then(\
                     function(v){{ globalThis.__lumen_bidi_await = {{ state: \"fulfilled\", value: v }}; }},\
                     function(e){{ globalThis.__lumen_bidi_await = {{ state: \"rejected\", error: (e && e.message !== undefined) ? String(e.message) : String(e) }}; }}\
                 );\
             }} catch (e) {{\
                 globalThis.__lumen_bidi_await = {{ state: \"rejected\", error: (e && e.message !== undefined) ? String(e.message) : String(e) }};\
             }}\
             return \"pending\";\
         }})()"
    );
    live.eval(&setup).map_err(|e| format!("eval: {e}"))?;

    // Round 2: read the outcome the microtask checkpoint left behind.
    let read = "(function(){ var r = globalThis.__lumen_bidi_await; \
         try { delete globalThis.__lumen_bidi_await; } catch (e) {} return r; })()";
    let raw = live.eval(read).map_err(|e| format!("eval: {e}"))?;

    match parse_json(&raw) {
        Ok(JsonValue::Object(map)) => match map.get("state").and_then(|v| v.as_str()) {
            Some("fulfilled") => Ok(match map.get("value") {
                Some(v) => remote_value_from_json(v),
                // A missing `value` means the promise fulfilled with `undefined`
                // (JSON serialization drops undefined-valued keys).
                None => undefined_remote_value(),
            }),
            Some("rejected") => Err(format!(
                "uncaught (in promise): {}",
                map.get("error").and_then(|v| v.as_str()).unwrap_or("")
            )),
            // Still pending (needs a macrotask/IO) — fall back to the promise
            // object, same as the non-awaited path.
            _ => Ok(eval_result_to_remote_value("{}")),
        },
        _ => Ok(eval_result_to_remote_value(&raw)),
    }
}

/// `script.callFunction` — вызвать функцию в browsing context (BiDi §10.2.5).
///
/// Phase 1 stub: те же проверки что script.evaluate, возвращает `{type:"undefined"}`
/// (no `expression` field in its params, so `script_evaluate` always takes its
/// stub branch here — real `functionDeclaration` + argument marshalling is future work).
fn script_call_function(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    script_evaluate(id, params, state)
}

/// `script.addPreloadScript` — зарегистрировать preload script (BiDi §10.2.1).
///
/// Параметры: `functionDeclaration` (JS source), необязательный `contexts` (массив context id).
/// Пустой `contexts` = применять ко всем контекстам.
/// Реальное выполнение при загрузке страницы требует 8A.7 (shell-as-driver-client).
fn script_add_preload(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let source = params
        .get("functionDeclaration")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let contexts: Vec<String> = params
        .get("contexts")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    // Validate that context ids exist if specified.
    for ctx_id in &contexts {
        if state.find(ctx_id).is_none() {
            return DispatchResult::single(make_error(
                Some(id),
                "no such frame",
                &format!("unknown context: {ctx_id}"),
            ));
        }
    }

    let script_id = state.next_id(0xaaaa);
    state.preload_scripts.push(PreloadScript {
        id: script_id.clone(),
        source,
        contexts,
    });

    let mut result = BTreeMap::new();
    result.insert("script".into(), JsonValue::String(script_id));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `script.removePreloadScript` — удалить preload script по id (BiDi §10.2.2).
///
/// Возвращает `no such script`, если id неизвестен.
fn script_remove_preload(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(script_id) = params.get("script").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "missing script id",
        ));
    };
    let before = state.preload_scripts.len();
    state.preload_scripts.retain(|s| s.id != script_id);
    if state.preload_scripts.len() == before {
        return DispatchResult::single(make_error(
            Some(id),
            "no such script",
            &format!("unknown preload script: {script_id}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

// ──────────────────────────────────────────
// network.* handlers (BiDi §12)
// ──────────────────────────────────────────

/// `network.getResponseBody` — вернуть буферизованное тело ответа (BiDi §12.6.4).
///
/// Ожидает `params.request.requestId` (число). Возвращает тело в base64.
/// Ошибка `no such request`, если тело не буферизовано (запрос неизвестен).
fn network_get_response_body(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    let request_id = params
        .get("request")
        .and_then(|r| r.get("requestId"))
        .and_then(|v| v.as_number())
        .map(|n| n as u64);

    let Some(request_id) = request_id else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "missing request.requestId",
        ));
    };

    let Some(body) = state.response_bodies.get(&request_id) else {
        return DispatchResult::single(make_error(
            Some(id),
            "no such request",
            &format!("no buffered body for requestId: {request_id}"),
        ));
    };

    let encoded = lumen_core::hash::base64_encode(body);
    let mut body_obj = BTreeMap::new();
    body_obj.insert("type".into(), JsonValue::String("base64".into()));
    body_obj.insert("value".into(), JsonValue::String(encoded));

    let mut result = BTreeMap::new();
    result.insert("body".into(), JsonValue::Object(body_obj));

    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// Параметры события `network.responseBodyReceived` (BiDi §12.5.3, упрощённые).
///
/// Содержит `request.requestId` и тело в base64 — минимальный набор для stub-слоя.
// Вызывается только из record_response_body, которая помечена #[allow(dead_code)].
#[allow(dead_code)]
fn network_response_body_event_params(request_id: u64, body: &[u8]) -> JsonValue {
    let mut req = BTreeMap::new();
    req.insert("requestId".into(), JsonValue::Number(request_id as f64));

    let encoded = lumen_core::hash::base64_encode(body);
    let mut body_obj = BTreeMap::new();
    body_obj.insert("type".into(), JsonValue::String("base64".into()));
    body_obj.insert("value".into(), JsonValue::String(encoded));

    let mut params = BTreeMap::new();
    params.insert("request".into(), JsonValue::Object(req));
    params.insert("body".into(), JsonValue::Object(body_obj));

    JsonValue::Object(params)
}

/// `session.setDefaultUserContextLocale` — установить локаль по умолчанию для контекстов.
///
/// Параметр `locale` — IETF BCP 47 тег (напр., `"en-US"`, `"ru"`).
/// Хранится в [`BidiState::default_locale`] и читается через [`BidiState::locale()`].
fn session_set_locale(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    state.default_locale = params.get("locale").and_then(|v| v.as_str()).map(str::to_owned);
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `browser.setTimezoneOverride` — установить переопределение часового пояса.
///
/// Параметр `timezoneId` — IANA-идентификатор (напр., `"America/New_York"`, `"Europe/Moscow"`).
/// Хранится в [`BidiState::timezone_override`] и читается через [`BidiState::timezone()`].
///
/// BUG-295: с живым окном (`--bidi-port` + открытое окно) также толкает
/// значение в [`LiveWindowSession::set_timezone`] — реально сдвигает
/// `Intl.DateTimeFormat().resolvedOptions().timeZone` живой страницы, не
/// только in-memory bookkeeping.
fn browser_set_timezone(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    state.timezone_override =
        params.get("timezoneId").and_then(|v| v.as_str()).map(str::to_owned);
    if let Some(live) = &mut state.live
        && let Err(e) = live.set_timezone(state.timezone_override.as_deref())
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("setTimezoneOverride: {e}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `permissions.setPermission` (WebDriver BiDi Permissions module, BUG-1014).
///
/// Параметры: `descriptor: {name}`, `state: granted|denied|prompt`, `origin`
/// (обязателен по спецификации; в живом окне не разделяет состояние — см.
/// `lumen_js::v8_runtime::set_global_permission_override`). С живым окном
/// состояние попадает в то, что реально читает `navigator.permissions.query()`;
/// имя, неизвестное движку, — `invalid argument`, а не молчаливый успех.
fn permissions_set_permission(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let bad = |msg: &str| DispatchResult::single(make_error(Some(id), "invalid argument", msg));
    let Some(name) = params.get("descriptor").and_then(|d| d.get("name")).and_then(|v| v.as_str())
    else {
        return bad("setPermission: descriptor.name is required");
    };
    let Some(perm_state) = params.get("state").and_then(|v| v.as_str()) else {
        return bad("setPermission: state is required");
    };
    if !matches!(perm_state, "granted" | "denied" | "prompt") {
        return bad(&format!("setPermission: invalid state {perm_state:?}"));
    }
    if params.get("origin").and_then(|v| v.as_str()).is_none() {
        return bad("setPermission: origin is required");
    }
    if let Some(live) = &mut state.live {
        match live.set_permission(name, perm_state) {
            Ok(true) => {}
            Ok(false) => return bad(&format!("setPermission: unknown permission {name:?}")),
            Err(e) => {
                return DispatchResult::single(make_error(
                    Some(id),
                    "unknown error",
                    &format!("setPermission: {e}"),
                ));
            }
        }
    }
    state.permission_states.insert(name.to_owned(), perm_state.to_owned());
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `lumen.getComputedA11y` — расширение Lumen (не часть BiDi): роль и
/// accessible name элемента по цепочке селекторов `params.selectors`
/// (внешний документ → вниз через shadow root). Нужен WPT-экшенам
/// `get_computed_role` / `get_computed_label` (BUG-1014). Без живого окна
/// дерева доступности нет — `no such element`.
fn lumen_get_computed_a11y(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let selectors: Vec<String> = params
        .get("selectors")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    if selectors.is_empty() {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "getComputedA11y: selectors must be a non-empty array of strings",
        ));
    }
    let found = match &mut state.live {
        Some(live) => match live.computed_a11y(&selectors) {
            Ok(v) => v,
            Err(e) => {
                return DispatchResult::single(make_error(
                    Some(id),
                    "unknown error",
                    &format!("getComputedA11y: {e}"),
                ));
            }
        },
        None => None,
    };
    let Some((role, name)) = found else {
        return DispatchResult::single(make_error(Some(id), "no such element", "getComputedA11y: no such element"));
    };
    let mut result = BTreeMap::new();
    result.insert("role".into(), JsonValue::String(role));
    result.insert("name".into(), JsonValue::String(name));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `network.setOfflineStatus` — переключить симуляцию offline-режима сети.
///
/// Параметры: `{"status": {"offline": true}}` или `{"offline": true}` (упрощённая форма).
/// После установки `true` все сетевые запросы должны имитировать ошибку подключения.
/// Читается через [`BidiState::is_offline()`].
///
/// BUG-295: с живым окном (`--bidi-port` + открытое окно) также толкает флаг
/// в [`LiveWindowSession::set_offline`] — реально фейлит навигацию/`fetch()`/XHR
/// живой страницы, не только in-memory bookkeeping.
fn network_set_offline(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    // Поддерживаем обе формы: {"status":{"offline":true}} и {"offline":true}.
    let offline = params
        .get("status")
        .and_then(|s| s.get("offline"))
        .and_then(JsonValue::as_bool)
        .or_else(|| params.get("offline").and_then(JsonValue::as_bool))
        .unwrap_or(false);
    state.offline = offline;
    if let Some(live) = &mut state.live
        && let Err(e) = live.set_offline(offline)
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("setOfflineStatus: {e}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `emulation.setUserAgentOverride` — переопределить User-Agent на уровне сессии или контекста.
///
/// Параметр `userAgent` — строка UA. Необязательный параметр `contexts` — массив context id:
/// если задан, переопределение применяется к указанным контекстам; иначе — ко всей сессии.
/// Per-context переопределение имеет приоритет над сессионным при чтении
/// через [`BidiState::user_agent_for()`].
///
/// BUG-295: с живым окном также толкает эффективный UA (после применения
/// этого вызова) в [`LiveWindowSession::set_user_agent`] — реально
/// переопределяет `navigator.userAgent` и реальный HTTP `User-Agent` для
/// будущих запросов. Lumen — один процесс с одним нативным окном (см.
/// `CLIENT_WINDOW_ID`), поэтому per-context targeting не может маршрутизировать
/// разные UA в разные окна; живому окну всегда толкается эффективный UA
/// **первого** контекста сессии (обычно единственного).
fn emulation_set_ua_override(
    id: i64,
    params: &JsonValue,
    state: &mut BidiState,
) -> DispatchResult {
    let ua = params.get("userAgent").and_then(|v| v.as_str()).map(str::to_owned);
    let ctx_ids: Vec<String> = params
        .get("contexts")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    if ctx_ids.is_empty() {
        state.session_ua_override = ua;
    } else {
        for ctx_id in &ctx_ids {
            if state.contexts.iter().all(|c| c.id != *ctx_id) {
                return DispatchResult::single(make_error(
                    Some(id),
                    "no such frame",
                    &format!("no such context: {ctx_id}"),
                ));
            }
        }
        for ctx_id in &ctx_ids {
            if let Some(ctx) = state.contexts.iter_mut().find(|c| c.id == *ctx_id) {
                ctx.ua_override = ua.clone();
            }
        }
    }

    let effective_ua = state
        .contexts
        .first()
        .and_then(|c| state.user_agent_for(&c.id).map(str::to_owned))
        .or_else(|| state.session_ua_override.clone());
    if let Some(live) = &mut state.live
        && let Err(e) = live.set_user_agent(effective_ua.as_deref().unwrap_or(""))
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("setUserAgentOverride: {e}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `browser.getDownloads` — list all tracked download items (BiDi §7).
///
/// Returns `{downloads: [{downloadId, url, fileName, receivedBytes, totalBytes, state}]}`.
fn browser_get_downloads(id: i64, state: &BidiState) -> DispatchResult {
    let downloads: Vec<JsonValue> = state
        .download_items
        .values()
        .map(|item| {
            download_event_params(
                &item.id,
                &item.url,
                &item.file_name,
                item.received_bytes,
                item.total_bytes,
                match item.state {
                    DownloadState::InProgress => "inProgress",
                    DownloadState::Completed => "completed",
                    DownloadState::Aborted => "aborted",
                },
            )
        })
        .collect();
    let mut result = BTreeMap::new();
    result.insert("downloads".into(), JsonValue::Array(downloads));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// Extract the effective domain from an origin URL (e.g. `"https://example.com"` → `"example.com"`).
///
/// Strips scheme and port; used for `partition.sourceOrigin` cookie filtering.
fn origin_to_domain(origin: &str) -> String {
    let host = origin
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("//");
    // Drop port if present.
    let host = host.split(':').next().unwrap_or(host);
    // Drop trailing slash or path.
    let host = host.split('/').next().unwrap_or(host);
    host.to_owned()
}

// ──────────────────────────────────────────
// script.disown / script.getRealms (BiDi §10)
// ──────────────────────────────────────────

/// `script.disown` — release remote value handles (BiDi §10.2.3).
///
/// Phase 1 stub: validates params shape, returns ACK.
/// Real handle tracking requires 8A.7 (shell-as-driver-client).
fn script_disown(id: i64, _params: &JsonValue) -> DispatchResult {
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `script.getRealms` — list all realms in the session (BiDi §10.2.6).
///
/// Phase 1 stub: returns an empty realms array.
/// Real realm tracking requires live wiring to the JS engine — 8H.3.
fn script_get_realms(id: i64, _state: &BidiState) -> DispatchResult {
    let mut result = BTreeMap::new();
    result.insert("realms".into(), JsonValue::Array(vec![]));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

// ──────────────────────────────────────────
// network.addIntercept / removeIntercept / setCacheBehavior (BiDi §12)
// ──────────────────────────────────────────

/// `network.addIntercept` — register a network intercept rule (BiDi §12.6.9).
///
/// Stores the rule and returns an opaque `intercept` ID. BUG-295 remainder:
/// with a live window (`--bidi-port` + an open window), also syncs the rule
/// into [`LiveWindowSession::add_intercept`] — `lumen_network`'s
/// process-global registry, consulted at the `fetch_with_redirect` chokepoint
/// every fetch path funnels through. Only the `"beforeRequestSent"` phase is
/// actually paused on — `"beforeRequestSent"` and `"responseStarted"`
/// (DEVX-20). `"authRequired"` is rejected as `unsupported operation`
/// (HTTP-auth challenges are not a first-class event in `lumen_network`; a
/// silently-stored phase that never fires would mislead the client), any
/// other phase name as `invalid argument`.
fn network_add_intercept(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let phases: Vec<String> = params
        .get("phases")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    if phases.is_empty() {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "phases must be a non-empty array",
        ));
    }

    if phases.iter().any(|p| p == "authRequired") {
        return DispatchResult::single(make_error(
            Some(id),
            "unsupported operation",
            "the authRequired intercept phase is not supported",
        ));
    }
    if let Some(bad) = phases.iter().find(|p| !matches!(p.as_str(), "beforeRequestSent" | "responseStarted")) {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            &format!("unknown intercept phase: {bad}"),
        ));
    }

    let url_patterns: Vec<String> = params
        .get("urlPatterns")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("pattern").and_then(|p| p.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let intercept_id = state.next_id(0x1ce0);
    if let Some(live) = &mut state.live
        && let Err(e) = live.add_intercept(&intercept_id, &phases, &url_patterns)
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("addIntercept: {e}"),
        ));
    }
    state.intercepts.push(NetworkIntercept {
        id: intercept_id.clone(),
        phases,
        url_patterns,
    });

    let mut result = BTreeMap::new();
    result.insert("intercept".into(), JsonValue::String(intercept_id));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `network.removeIntercept` — remove a registered intercept rule (BiDi §12.6.11).
///
/// Returns `no such intercept` if the ID is unknown. BUG-295 remainder: with a
/// live window, also removes the rule from `lumen_network`'s process-global
/// registry via [`LiveWindowSession::remove_intercept`].
fn network_remove_intercept(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(intercept_id) = params.get("intercept").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "missing intercept id",
        ));
    };

    let before = state.intercepts.len();
    state.intercepts.retain(|i| i.id != intercept_id);
    if state.intercepts.len() == before {
        return DispatchResult::single(make_error(
            Some(id),
            "no such intercept",
            &format!("unknown intercept: {intercept_id}"),
        ));
    }
    if let Some(live) = &mut state.live
        && let Err(e) = live.remove_intercept(intercept_id)
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("removeIntercept: {e}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `network.continueRequest` — resolve a paused request as "continue"
/// (BiDi §12.6.13, BUG-295 remainder).
///
/// Delivers the decision to `lumen_network`'s paused-request registry via
/// [`LiveWindowSession::resolve_intercepted_request`] when a live window is
/// attached. An unknown/already-resolved `request` id still ACKs — no
/// paused-request bookkeeping exists without a live window (Phase 1 stub),
/// and matching the bare-ACK tolerance this handler already had before real
/// bookkeeping existed avoids breaking callers that resolve defensively.
fn network_continue_request(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    resolve_intercepted_request(id, params, state, true)
}

/// `network.continueResponse` — resume a response paused at the
/// `responseStarted` phase (BiDi §12.6.14, DEVX-20). Unlike
/// [`network_continue_request`] an unknown/expired `request` id is an error
/// (`no such request`): success must mean a paused response was released.
/// The optional response overrides are not applied, so they are rejected
/// rather than silently ignored.
fn network_continue_response(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(request_id) = params.get("request").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing request id"));
    };
    for key in ["statusCode", "reasonPhrase", "headers", "cookies", "credentials"] {
        if params.get(key).is_some() {
            return DispatchResult::single(make_error(
                Some(id),
                "unsupported operation",
                &format!("network.continueResponse does not support overriding `{key}`"),
            ));
        }
    }
    let resolved = match &mut state.live {
        Some(live) => match live.resolve_intercepted_request(request_id, true) {
            Ok(matched) => matched,
            Err(e) => {
                return DispatchResult::single(make_error(
                    Some(id),
                    "unknown error",
                    &format!("resolve intercepted response: {e}"),
                ));
            }
        },
        None => false,
    };
    if !resolved {
        return DispatchResult::single(make_error(
            Some(id),
            "no such request",
            &format!("no paused request with id: {request_id}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `network.failRequest` — resolve a paused request as "fail" (BiDi §12.6.15,
/// BUG-295 remainder). See [`network_continue_request`] for the shared shape.
fn network_fail_request(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    resolve_intercepted_request(id, params, state, false)
}

/// Shared implementation of [`network_continue_request`]/[`network_fail_request`].
fn resolve_intercepted_request(
    id: i64,
    params: &JsonValue,
    state: &mut BidiState,
    continue_request: bool,
) -> DispatchResult {
    let Some(request_id) = params.get("request").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing request id"));
    };
    if let Some(live) = &mut state.live
        && let Err(e) = live.resolve_intercepted_request(request_id, continue_request)
    {
        return DispatchResult::single(make_error(
            Some(id),
            "unknown error",
            &format!("resolve intercepted request: {e}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `network.setCacheBehavior` — configure browser cache behaviour (BiDi §12.6.12).
///
/// Accepts `{"cacheBehavior":"default"|"bypass"|"restore"}`.
/// Phase 1: stored in state; actual HTTP client integration is 8H.3.
fn network_set_cache_behavior(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let behavior = params
        .get("cacheBehavior")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    state.cache_behavior = behavior;
    DispatchResult::single(make_success(id, empty_obj()))
}

// ──────────────────────────────────────────
// browsingContext.handleUserPrompt (BiDi §6.8)
// ──────────────────────────────────────────

/// `browsingContext.handleUserPrompt` — dismiss or accept an open dialog (BiDi §6.8.7).
///
/// Params: `context` (required), `accept: bool` (default `true`), `userText` (prompt only).
/// Emits `browsingContext.userPromptClosed` if subscribed.
fn bc_handle_user_prompt(
    id: i64,
    params: &JsonValue,
    state: &mut BidiState,
) -> DispatchResult {
    let Some(ctx_id) = params.get("context").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "missing context",
        ));
    };

    // Find the first open prompt for this context.
    let pos = state.user_prompts.iter().position(|p| p.context == ctx_id);
    let Some(pos) = pos else {
        return DispatchResult::single(make_error(
            Some(id),
            "no such alert",
            &format!("no open prompt for context: {ctx_id}"),
        ));
    };

    let prompt = state.user_prompts.remove(pos);
    let accepted = params.get("accept").and_then(|v| v.as_bool()).unwrap_or(true);
    let user_text = params
        .get("userText")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();

    let mut frames = vec![make_success(id, empty_obj())];
    if state.is_subscribed("browsingContext.userPromptClosed") {
        let mut ev = BTreeMap::new();
        ev.insert("context".into(), JsonValue::String(prompt.context));
        ev.insert("accepted".into(), JsonValue::Bool(accepted));
        ev.insert("type".into(), JsonValue::String(prompt.type_));
        if !user_text.is_empty() {
            ev.insert("userText".into(), JsonValue::String(user_text));
        }
        frames.push(make_event(
            "browsingContext.userPromptClosed",
            JsonValue::Object(ev),
        ));
    }

    DispatchResult { frames, close: false }
}

// ──────────────────────────────────────────
// browsingContext.setViewport (BiDi §6.6.9)
// ──────────────────────────────────────────

/// `browsingContext.setViewport` — set the viewport dimensions of a context (BiDi §6.6.9).
///
/// Params: `context` (required), `viewport: {width, height}` (optional), `devicePixelRatio` (ignored, Phase 1).
/// Phase 1: stores dimensions in state; emits `browsingContext.viewportChanged` if subscribed.
/// Actual window resize is an 8H.3 handoff to the shell windowing layer.
fn bc_set_viewport(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(cid) = params.get("context").and_then(|v| v.as_str()).map(str::to_owned) else {
        return DispatchResult::single(make_error(Some(id), "invalid argument", "missing context"));
    };
    if state.find(&cid).is_none() {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("no such context: {cid}"),
        ));
    }

    // Extract viewport {width, height}; absence means "no viewport override" (reset to None).
    let new_viewport = params.get("viewport").and_then(|vp| {
        let w = vp.get("width").and_then(|v| v.as_number()).map(|n| n as u32)?;
        let h = vp.get("height").and_then(|v| v.as_number()).map(|n| n as u32)?;
        Some((w, h))
    });

    if let Some(ctx) = state.contexts.iter_mut().find(|c| c.id == cid) {
        ctx.viewport = new_viewport;
    }

    let mut frames = vec![make_success(id, empty_obj())];
    if state.is_subscribed("browsingContext.viewportChanged") {
        let mut ev = BTreeMap::new();
        ev.insert("context".into(), JsonValue::String(cid));
        ev.insert("viewport".into(), viewport_json(new_viewport));
        frames.push(make_event("browsingContext.viewportChanged", JsonValue::Object(ev)));
    }
    DispatchResult { frames, close: false }
}

/// Serialize viewport `(w, h)` to BiDi `Viewport` JSON object; `Null` if not set.
fn viewport_json(vp: Option<(u32, u32)>) -> JsonValue {
    match vp {
        Some((w, h)) => {
            let mut obj = BTreeMap::new();
            obj.insert("width".into(), JsonValue::Number(w as f64));
            obj.insert("height".into(), JsonValue::Number(h as f64));
            JsonValue::Object(obj)
        }
        None => JsonValue::Null,
    }
}

// ──────────────────────────────────────────
// storage.* handlers (BiDi §13)
// ──────────────────────────────────────────

/// `storage.getCookies` / `storage.deleteCookies` filter (BiDi §13): `filter`
/// with `name`, `domain`, `path`, plus `partition.sourceOrigin` as an
/// origin-based domain filter. An absent field matches everything.
struct CookieFilter {
    name: Option<String>,
    domain: Option<String>,
    path: Option<String>,
    origin_domain: Option<String>,
}

impl CookieFilter {
    fn from_params(params: &JsonValue) -> Self {
        let filter = params.get("filter");
        let field = |key: &str| filter.and_then(|f| f.get(key)).and_then(|v| v.as_str()).map(str::to_owned);
        Self {
            name: field("name"),
            domain: field("domain"),
            path: field("path"),
            // `partition.sourceOrigin` strips the scheme+port to extract the effective domain.
            origin_domain: params
                .get("partition")
                .and_then(|p| p.get("sourceOrigin"))
                .and_then(|v| v.as_str())
                .map(origin_to_domain),
        }
    }

    fn matches(&self, c: &BidiCookie) -> bool {
        let domain_ok = |d: &str| c.domain == d || c.domain.ends_with(&format!(".{d}"));
        self.name.as_deref().is_none_or(|n| c.name == n)
            && self.domain.as_deref().is_none_or(domain_ok)
            && self.path.as_deref().is_none_or(|p| c.path == p)
            && self.origin_domain.as_deref().is_none_or(domain_ok)
    }
}

/// Cookies visible to `storage.*`: the live window's jar (the one pages read
/// through `document.cookie` and requests send from, `HttpOnly` included) when
/// the session is attached to a window, else the session's own memory. DEVX-20.
fn session_cookies(state: &BidiState) -> Result<Vec<BidiCookie>, String> {
    let Some(live) = state.live.as_ref() else { return Ok(state.cookies.clone()) };
    let cookies = live.get_cookies().map_err(|e| e.to_string())?;
    Ok(cookies
        .into_iter()
        .map(|c| BidiCookie {
            name: c.name,
            value: c.value,
            domain: c.domain,
            path: c.path,
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site,
            expiry: c.expiry,
        })
        .collect())
}

/// `storage.Cookie` value: a plain string or a BiDi `BytesValue`
/// (`{"type":"string","value":…}`). `base64` bytes are not decoded.
fn cookie_value_text(value: &JsonValue) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_owned());
    }
    if value.get("type").and_then(|t| t.as_str()) == Some("string") {
        return value.get("value").and_then(|v| v.as_str()).map(str::to_owned);
    }
    None
}

/// `storage.getCookies` — retrieve cookies matching an optional filter (BiDi §13.2.1).
///
/// Params: `filter` (optional) with `name`, `domain`, `path` string matchers.
/// Also accepts `partition.sourceOrigin` as an origin-based domain filter (BiDi §13).
/// With a live window the answer comes from the real cookie jar (DEVX-20).
fn storage_get_cookies(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    let filter = CookieFilter::from_params(params);
    let all = match session_cookies(state) {
        Ok(all) => all,
        Err(e) => return DispatchResult::single(make_error(Some(id), "unknown error", &e)),
    };
    let cookies: Vec<JsonValue> = all.iter().filter(|c| filter.matches(c)).map(cookie_to_json).collect();

    let mut result = BTreeMap::new();
    result.insert("cookies".into(), JsonValue::Array(cookies));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `storage.setCookie` — add or replace a cookie (BiDi §13.2.2).
///
/// Params: `cookie { name, value, domain, path?, secure?, httpOnly?, sameSite?, expiry? }`;
/// `value` is a string or a `{type:"string", value}` BytesValue.
/// With a live window the cookie lands in the real jar (DEVX-20).
/// Auto-emits `storage.cookieAdded` or `storage.cookieChanged` when subscribed.
fn storage_set_cookie(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let Some(cookie_params) = params.get("cookie") else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "missing cookie",
        ));
    };

    let Some(name) = cookie_params.get("name").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "cookie.name is required",
        ));
    };
    let Some(value) = cookie_params.get("value").and_then(cookie_value_text) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "cookie.value is required (string or {type:\"string\", value})",
        ));
    };
    let Some(domain) = cookie_params.get("domain").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "cookie.domain is required",
        ));
    };

    let cookie = BidiCookie {
        name: name.to_owned(),
        value,
        domain: domain.to_owned(),
        path: cookie_params.get("path").and_then(|v| v.as_str()).unwrap_or("/").to_owned(),
        secure: cookie_params.get("secure").and_then(|v| v.as_bool()).unwrap_or(false),
        http_only: cookie_params.get("httpOnly").and_then(|v| v.as_bool()).unwrap_or(false),
        same_site: cookie_params
            .get("sameSite")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_owned(),
        expiry: cookie_params.get("expiry").and_then(|v| v.as_number()).unwrap_or(0.0) as u64,
    };

    let same_key = |c: &BidiCookie| c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path;
    let is_update;
    if let Some(live) = state.live.as_ref() {
        let existing = match session_cookies(state) {
            Ok(all) => all,
            Err(e) => return DispatchResult::single(make_error(Some(id), "unknown error", &e)),
        };
        is_update = existing.iter().any(same_key);
        let live_cookie = lumen_driver::AutomationCookie {
            name: cookie.name.clone(),
            value: cookie.value.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
            secure: cookie.secure,
            http_only: cookie.http_only,
            same_site: cookie.same_site.clone(),
            expiry: cookie.expiry,
        };
        if let Err(e) = live.set_cookie(live_cookie) {
            return DispatchResult::single(make_error(Some(id), "unable to set cookie", &e.to_string()));
        }
    } else {
        // Replace existing or append; track whether this is an add or update.
        is_update = state.cookies.iter().any(same_key);
        if is_update {
            if let Some(slot) = state.cookies.iter_mut().find(|c| same_key(c)) {
                *slot = cookie.clone();
            }
        } else {
            state.cookies.push(cookie.clone());
        }
    }

    let mut frames = vec![make_success(id, empty_obj())];
    let event_name = if is_update { "storage.cookieChanged" } else { "storage.cookieAdded" };
    if state.is_subscribed(event_name) {
        frames.push(make_event(event_name, cookie_event_params(&cookie)));
    }
    DispatchResult { frames, close: false }
}

/// `storage.deleteCookies` — remove cookies matching filter (BiDi §13.2.3).
///
/// Params: `filter` with optional `name`, `domain`, `path`.
/// Also accepts `partition.sourceOrigin` for per-origin deletion (BiDi §13).
/// With a live window the cookies are removed from the real jar (DEVX-20).
/// Auto-emits `storage.cookieRemoved` for each deleted cookie when subscribed.
fn storage_delete_cookies(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let filter = CookieFilter::from_params(params);

    // Collect deleted cookies before removal so we can emit events.
    let all = match session_cookies(state) {
        Ok(all) => all,
        Err(e) => return DispatchResult::single(make_error(Some(id), "unknown error", &e)),
    };
    let deleted: Vec<BidiCookie> = all.into_iter().filter(|c| filter.matches(c)).collect();
    if let Some(live) = state.live.as_ref() {
        for c in &deleted {
            if let Err(e) = live.delete_cookie(&c.domain, &c.path, &c.name) {
                return DispatchResult::single(make_error(Some(id), "unable to set cookie", &e.to_string()));
            }
        }
    } else {
        state.cookies.retain(|c| !filter.matches(c));
    }

    let mut frames = vec![make_success(id, empty_obj())];
    if state.is_subscribed("storage.cookieRemoved") {
        for c in &deleted {
            frames.push(make_event("storage.cookieRemoved", cookie_event_params(c)));
        }
    }
    DispatchResult { frames, close: false }
}

// ──────────────────────────────────────────
// Helper: cookie serialisation
// ──────────────────────────────────────────

/// Serialize a `BidiCookie` to BiDi `storage.Cookie` JSON object.
fn cookie_to_json(c: &BidiCookie) -> JsonValue {
    let mut obj = BTreeMap::new();
    obj.insert("name".into(), JsonValue::String(c.name.clone()));
    obj.insert("value".into(), JsonValue::String(c.value.clone()));
    obj.insert("domain".into(), JsonValue::String(c.domain.clone()));
    obj.insert("path".into(), JsonValue::String(c.path.clone()));
    obj.insert("secure".into(), JsonValue::Bool(c.secure));
    obj.insert("httpOnly".into(), JsonValue::Bool(c.http_only));
    obj.insert("sameSite".into(), JsonValue::String(c.same_site.clone()));
    if c.expiry > 0 {
        obj.insert("expiry".into(), JsonValue::Number(c.expiry as f64));
    }
    JsonValue::Object(obj)
}

/// Build `storage.cookie*` event params containing the changed cookie.
fn cookie_event_params(cookie: &BidiCookie) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("cookie".into(), cookie_to_json(cookie));
    JsonValue::Object(params)
}

/// Build `browser.download*` event params.
fn download_event_params(
    item_id: &str,
    url: &str,
    file_name: &str,
    received: u64,
    total: u64,
    state: &str,
) -> JsonValue {
    let mut obj = BTreeMap::new();
    obj.insert("downloadId".into(), JsonValue::String(item_id.to_owned()));
    obj.insert("url".into(), JsonValue::String(url.to_owned()));
    obj.insert("fileName".into(), JsonValue::String(file_name.to_owned()));
    obj.insert("receivedBytes".into(), JsonValue::Number(received as f64));
    obj.insert("totalBytes".into(), JsonValue::Number(total as f64));
    obj.insert("state".into(), JsonValue::String(state.to_owned()));
    JsonValue::Object(obj)
}

// ──────────────────────────────────────────
// input.* handlers (BiDi §15)
// ──────────────────────────────────────────

/// `input.performActions` — execute a sequence of input actions (BiDi §15.7.3).
///
/// Phase 1 stub: validates `context` and `actions` parameters, returns ACK.
/// Actual action dispatch to the windowing layer requires 8H.3.
fn input_perform_actions(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    // Validate context if provided.
    if let Some(ctx_id) = params.get("context").and_then(|v| v.as_str())
        && state.find(ctx_id).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("unknown browsing context: {ctx_id}"),
        ));
    }

    // Validate actions is an array.
    let Some(sources) = (match params.get("actions") {
        Some(JsonValue::Array(arr)) => Some(arr.clone()),
        None => Some(Vec::new()),
        _ => None,
    }) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "actions must be an array",
        ));
    };

    if let Some(live) = &mut state.live {
        replay_input_actions(live, &sources);
    }

    DispatchResult::single(make_success(id, empty_obj()))
}

/// WebDriver "normalised key value" (W3C WebDriver 17.4.2, PUA range
/// `\u{E000}`-`\u{E05D}`) -> `(KeyboardEvent.key, KeyboardEvent.code)`.
/// `None` - an ordinary character, which is typed as text.
fn webdriver_special_key(ch: char) -> Option<(&'static str, &'static str)> {
    Some(match ch {
        '\u{E003}' => ("Backspace", "Backspace"),
        '\u{E004}' => ("Tab", "Tab"),
        '\u{E005}' => ("Clear", "Clear"),
        '\u{E006}' => ("Enter", "Enter"),
        '\u{E007}' => ("Enter", "NumpadEnter"),
        '\u{E008}' => ("Shift", "ShiftLeft"),
        '\u{E009}' => ("Control", "ControlLeft"),
        '\u{E00A}' => ("Alt", "AltLeft"),
        '\u{E00B}' => ("Pause", "Pause"),
        '\u{E00C}' => ("Escape", "Escape"),
        '\u{E00D}' => (" ", "Space"),
        '\u{E00E}' => ("PageUp", "PageUp"),
        '\u{E00F}' => ("PageDown", "PageDown"),
        '\u{E010}' => ("End", "End"),
        '\u{E011}' => ("Home", "Home"),
        '\u{E012}' => ("ArrowLeft", "ArrowLeft"),
        '\u{E013}' => ("ArrowUp", "ArrowUp"),
        '\u{E014}' => ("ArrowRight", "ArrowRight"),
        '\u{E015}' => ("ArrowDown", "ArrowDown"),
        '\u{E016}' => ("Insert", "Insert"),
        '\u{E017}' => ("Delete", "Delete"),
        '\u{E031}' => ("F1", "F1"),
        '\u{E032}' => ("F2", "F2"),
        '\u{E033}' => ("F3", "F3"),
        '\u{E034}' => ("F4", "F4"),
        '\u{E035}' => ("F5", "F5"),
        '\u{E036}' => ("F6", "F6"),
        '\u{E037}' => ("F7", "F7"),
        '\u{E038}' => ("F8", "F8"),
        '\u{E039}' => ("F9", "F9"),
        '\u{E03A}' => ("F10", "F10"),
        '\u{E03B}' => ("F11", "F11"),
        '\u{E03C}' => ("F12", "F12"),
        '\u{E03D}' => ("Meta", "MetaLeft"),
        _ => return None,
    })
}

/// Replay the pointer and key-input subset of a BiDi `input.performActions`
/// action chain against a live window (SDC-2 MVP).
///
/// Supported: a `"pointer"` source's `pointerMove {x,y}` moves the cursor
/// (hover: `mouseover`/`:hover`, BUG-1194) and `pointerDown` clicks at the
/// last point; a `"key"` source's `keyDown`/`keyUp {value}` - WebDriver PUA
/// codes (`\u{E00C}` Escape, `\u{E004}` Tab, ...) become `keydown`/`keyup` with
/// the proper `key`/`code`, ordinary characters are typed: at the last pointer
/// position when a pointer action preceded them (click-then-type), otherwise
/// into the already focused element, without a click. NOT modeled: pauses,
/// multi-touch/wheel sources, drag gestures, or `pointerUp`-gated release
/// semantics - full W3C Actions fidelity is future work.
fn replay_input_actions(live: &mut LiveWindowSession, sources: &[JsonValue]) {
    let mut last_point: Option<Target> = None;
    for source in sources {
        let source_type = source.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let Some(actions) = source.get("actions").and_then(|v| v.as_array()) else { continue };
        match source_type {
            "pointer" => {
                for action in actions {
                    match action.get("type").and_then(|v| v.as_str()) {
                        Some("pointerMove") => {
                            let x = action.get("x").and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
                            let y = action.get("y").and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
                            last_point = Some(Target::Point { x, y });
                            let _ = live.pointer_move(x, y);
                        }
                        Some("pointerDown") => {
                            let point = last_point.clone().unwrap_or(Target::Point { x: 0.0, y: 0.0 });
                            let _ = live.click(&point);
                        }
                        _ => {}
                    }
                }
            }
            "key" => {
                let mut text = String::new();
                for action in actions {
                    let down = match action.get("type").and_then(|v| v.as_str()) {
                        Some("keyDown") => true,
                        Some("keyUp") => false,
                        _ => continue,
                    };
                    let Some(value) = action.get("value").and_then(|v| v.as_str()) else { continue };
                    for ch in value.chars() {
                        match webdriver_special_key(ch) {
                            Some((key, code)) => {
                                flush_typed_text(live, last_point.as_ref(), &mut text);
                                let _ = live.press_key(key, code, down);
                            }
                            None if down && !('\u{E000}'..='\u{E05D}').contains(&ch) => text.push(ch),
                            None => {}
                        }
                    }
                }
                flush_typed_text(live, last_point.as_ref(), &mut text);
            }
            _ => {}
        }
    }
}

/// Type the characters collected from consecutive `keyDown` actions and clear
/// the buffer: after a pointer action - click at its last point, then type;
/// without one - straight into the focused element.
fn flush_typed_text(live: &mut LiveWindowSession, point: Option<&Target>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    let _ = match point {
        Some(p) => live.type_text(p, text),
        None => live.type_focused(text),
    };
    text.clear();
}

/// `input.releaseActions` — release all active input sources (BiDi §15.7.4).
///
/// Phase 1 stub: validates context, returns ACK.
fn input_release_actions(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    if let Some(ctx_id) = params.get("context").and_then(|v| v.as_str())
        && state.find(ctx_id).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("unknown browsing context: {ctx_id}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `input.setFiles` — set files on a file-input element (BiDi §15.7.2).
///
/// Phase 1 stub: validates context, returns ACK.
fn input_set_files(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    if let Some(ctx_id) = params.get("context").and_then(|v| v.as_str())
        && state.find(ctx_id).is_none()
    {
        return DispatchResult::single(make_error(
            Some(id),
            "no such frame",
            &format!("unknown browsing context: {ctx_id}"),
        ));
    }
    DispatchResult::single(make_success(id, empty_obj()))
}

#[cfg(test)]
mod tests;
