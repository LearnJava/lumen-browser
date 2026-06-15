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
//! - `browsingContext.*`  — create/close/navigate/activate/getTree/handleUserPrompt/setViewport
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
//! Live wiring to the actual engine (real navigation, `domContentLoaded`, cookie events)
//! is a P3 handoff — roadmap 8H.3.
//! This layer is a pure protocol state machine: one connection = one [`BidiState`].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lumen_core::json::{parse as parse_json, JsonValue};

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

/// One network intercept rule registered via `network.addIntercept`.
struct NetworkIntercept {
    /// Opaque intercept identifier (BiDi `intercept`).
    id: String,
    /// Phases at which to intercept: "beforeRequestSent", "responseStarted", "authRequired".
    #[allow(dead_code)]
    phases: Vec<String>,
    /// URL patterns to match; empty means match-all (Phase 1 stub: stored but not evaluated).
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
    /// Offline network simulation: `true` = all network requests fail.
    ///
    /// Set by `network.setOfflineStatus`.
    offline: bool,
    /// Session-level User-Agent override; per-context overrides take priority.
    ///
    /// Set by `emulation.setUserAgentOverride` without `contexts`.
    session_ua_override: Option<String>,
    /// Registered network intercept rules (Phase 1 stub: stored, not evaluated).
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
}

impl BidiState {
    /// Новое пустое состояние соединения.
    pub fn new() -> Self {
        Self::default()
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

    /// Whether offline network simulation is active.
    // Shell layer blocks network requests when offline=true; not wired yet → false dead_code.
    #[allow(dead_code)]
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Effective User-Agent for a context: per-context → session-level → `None`.
    // Shell layer injects this UA into HTTP headers when present.
    #[allow(dead_code)]
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

    match method {
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
        "network.continueRequest" => DispatchResult::single(make_success(id, empty_obj())),
        "network.continueResponse" => DispatchResult::single(make_success(id, empty_obj())),
        "network.continueWithAuth" => DispatchResult::single(make_success(id, empty_obj())),
        "network.failRequest" => DispatchResult::single(make_success(id, empty_obj())),
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
        "storage.getCookies" => storage_get_cookies(id, &params, state),
        "storage.setCookie" => storage_set_cookie(id, &params, state),
        "storage.deleteCookies" => storage_delete_cookies(id, &params, state),
        other => DispatchResult::single(make_error(
            Some(id),
            "unknown command",
            &format!("unknown command: {other}"),
        )),
    }
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
fn session_new(id: i64, _params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    if state.session_id.is_some() {
        return DispatchResult::single(make_error(
            Some(id),
            "session not created",
            "session already exists",
        ));
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
/// Возвращает `{navigation, url}`. Реальной загрузки на этом слое нет (handoff
/// P3 в `lumen-driver`); эмитит `browsingContext.load`, если подписан.
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
        ev.insert("timestamp".into(), JsonValue::Number(0.0));
        frames.push(make_event("browsingContext.load", JsonValue::Object(ev)));
    }
    DispatchResult { frames, close: false }
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
    caps.insert("userAgent".into(), JsonValue::String(format!("Lumen/{}", env!("CARGO_PKG_VERSION"))));
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
/// Phase 1 stub: проверяет что context существует, возвращает `{type:"undefined"}`.
/// Реальное выполнение требует 8A.7 (shell-as-driver-client).
fn script_evaluate(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
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

    // Phase 1: return undefined stub.
    let mut result = BTreeMap::new();
    result.insert("type".into(), JsonValue::String("undefined".into()));

    let mut outer = BTreeMap::new();
    outer.insert("result".into(), JsonValue::Object(result));
    outer.insert("realm".into(), JsonValue::String("stub-realm".into()));

    DispatchResult::single(make_success(id, JsonValue::Object(outer)))
}

/// `script.callFunction` — вызвать функцию в browsing context (BiDi §10.2.5).
///
/// Phase 1 stub: те же проверки что script.evaluate, возвращает `{type:"undefined"}`.
fn script_call_function(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    // Same validation + stub response as evaluate.
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
fn browser_set_timezone(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    state.timezone_override =
        params.get("timezoneId").and_then(|v| v.as_str()).map(str::to_owned);
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `network.setOfflineStatus` — переключить симуляцию offline-режима сети.
///
/// Параметры: `{"status": {"offline": true}}` или `{"offline": true}` (упрощённая форма).
/// После установки `true` все сетевые запросы должны имитировать ошибку подключения.
/// Читается через [`BidiState::is_offline()`].
fn network_set_offline(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    // Поддерживаем обе формы: {"status":{"offline":true}} и {"offline":true}.
    let offline = params
        .get("status")
        .and_then(|s| s.get("offline"))
        .and_then(JsonValue::as_bool)
        .or_else(|| params.get("offline").and_then(JsonValue::as_bool))
        .unwrap_or(false);
    state.offline = offline;
    DispatchResult::single(make_success(id, empty_obj()))
}

/// `emulation.setUserAgentOverride` — переопределить User-Agent на уровне сессии или контекста.
///
/// Параметр `userAgent` — строка UA. Необязательный параметр `contexts` — массив context id:
/// если задан, переопределение применяется к указанным контекстам; иначе — ко всей сессии.
/// Per-context переопределение имеет приоритет над сессионным при чтении
/// через [`BidiState::user_agent_for()`].
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
/// Phase 1: stores the rule and returns an opaque `intercept` ID.
/// Actual request interception (pausing and delivering `network.beforeRequestSent`
/// events) requires live network integration — 8H.3.
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
/// Returns `no such intercept` if the ID is unknown.
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

/// `storage.getCookies` — retrieve cookies matching an optional filter (BiDi §13.2.1).
///
/// Params: `filter` (optional) with `name`, `domain`, `path` string matchers.
/// Also accepts `partition.sourceOrigin` as an origin-based domain filter (BiDi §13).
fn storage_get_cookies(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
    let filter = params.get("filter");
    let filter_name = filter.and_then(|f| f.get("name")).and_then(|v| v.as_str());
    let filter_domain = filter.and_then(|f| f.get("domain")).and_then(|v| v.as_str());
    let filter_path = filter.and_then(|f| f.get("path")).and_then(|v| v.as_str());
    // `partition.sourceOrigin` strips the scheme+port to extract the effective domain.
    let filter_origin_domain =
        params.get("partition").and_then(|p| p.get("sourceOrigin")).and_then(|v| v.as_str()).map(origin_to_domain);

    let cookies: Vec<JsonValue> = state
        .cookies
        .iter()
        .filter(|c| {
            filter_name.is_none_or(|n| c.name == n)
                && filter_domain
                    .is_none_or(|d| c.domain == d || c.domain.ends_with(&format!(".{d}")))
                && filter_path.is_none_or(|p| c.path == p)
                && filter_origin_domain.as_deref().is_none_or(|d| {
                    c.domain == d || c.domain.ends_with(&format!(".{d}"))
                })
        })
        .map(cookie_to_json)
        .collect();

    let mut result = BTreeMap::new();
    result.insert("cookies".into(), JsonValue::Array(cookies));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `storage.setCookie` — add or replace a cookie (BiDi §13.2.2).
///
/// Params: `cookie { name, value, domain, path?, secure?, httpOnly?, sameSite?, expiry? }`.
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
    let Some(value) = cookie_params.get("value").and_then(|v| v.as_str()) else {
        return DispatchResult::single(make_error(
            Some(id),
            "invalid argument",
            "cookie.value is required",
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
        value: value.to_owned(),
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

    // Replace existing or append; track whether this is an add or update.
    let is_update = state.cookies.iter().any(|c| {
        c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path
    });
    if is_update {
        for slot in &mut state.cookies {
            if slot.name == cookie.name && slot.domain == cookie.domain && slot.path == cookie.path
            {
                *slot = cookie.clone();
                break;
            }
        }
    } else {
        state.cookies.push(cookie.clone());
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
/// Auto-emits `storage.cookieRemoved` for each deleted cookie when subscribed.
fn storage_delete_cookies(id: i64, params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let filter = params.get("filter");
    let filter_name = filter.and_then(|f| f.get("name")).and_then(|v| v.as_str()).map(str::to_owned);
    let filter_domain =
        filter.and_then(|f| f.get("domain")).and_then(|v| v.as_str()).map(str::to_owned);
    let filter_path = filter.and_then(|f| f.get("path")).and_then(|v| v.as_str()).map(str::to_owned);
    let filter_origin_domain = params
        .get("partition")
        .and_then(|p| p.get("sourceOrigin"))
        .and_then(|v| v.as_str())
        .map(origin_to_domain);

    let matches_filter = |c: &BidiCookie| -> bool {
        let name_match = filter_name.as_deref().is_some_and(|n| c.name == n);
        let domain_match = filter_domain.as_deref().is_some_and(|d| {
            c.domain == d || c.domain.ends_with(&format!(".{d}"))
        });
        let path_match = filter_path.as_deref().is_some_and(|p| c.path == p);
        let origin_match = filter_origin_domain.as_deref().is_some_and(|d| {
            c.domain == d || c.domain.ends_with(&format!(".{d}"))
        });

        filter_name.as_ref().is_none_or(|_| name_match)
            && filter_domain.as_ref().is_none_or(|_| domain_match)
            && filter_path.as_ref().is_none_or(|_| path_match)
            && filter_origin_domain.as_ref().is_none_or(|_| origin_match)
    };

    // Collect deleted cookies before removal so we can emit events.
    let deleted: Vec<BidiCookie> =
        state.cookies.iter().filter(|c| matches_filter(c)).cloned().collect();
    state.cookies.retain(|c| !matches_filter(c));

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
fn input_perform_actions(id: i64, params: &JsonValue, state: &BidiState) -> DispatchResult {
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
    match params.get("actions") {
        Some(JsonValue::Array(_)) | None => {}
        _ => {
            return DispatchResult::single(make_error(
                Some(id),
                "invalid argument",
                "actions must be an array",
            ));
        }
    }

    DispatchResult::single(make_success(id, empty_obj()))
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
mod tests {
    use super::*;

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
        let mut state = BidiState::new();
        let r = dispatch(
            r#"{"id":1,"method":"network.continueRequest","params":{"request":{"requestId":1}}}"#,
            &mut state,
        );
        assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
    }

    #[test]
    fn network_fail_request_acks() {
        let mut state = BidiState::new();
        let r = dispatch(
            r#"{"id":1,"method":"network.failRequest","params":{"request":{"requestId":1}}}"#,
            &mut state,
        );
        assert!(r.frames[0].contains("success"), "got: {}", r.frames[0]);
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
}
