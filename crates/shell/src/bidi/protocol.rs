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
//! - `session.*`         — status/new/subscribe/unsubscribe/end/setDefaultUserContextLocale
//! - `browsingContext.*` — create/close/navigate/activate/getTree
//! - `script.*`          — evaluate/callFunction/addPreloadScript/removePreloadScript/disown/getRealms
//! - `network.*`         — getResponseBody/setOfflineStatus/addIntercept/removeIntercept/
//!   continueRequest/continueResponse/continueWithAuth/failRequest/setCacheBehavior
//! - `input.*`           — performActions/releaseActions/setFiles
//! - `browser.*`         — setTimezoneOverride
//! - `emulation.*`       — setUserAgentOverride
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
    pub(super) fn next_id(&mut self, tag: u16) -> String {
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
        "emulation.setUserAgentOverride" => emulation_set_ua_override(id, &params, state),
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
    });

    let mut result = BTreeMap::new();
    result.insert("context".into(), JsonValue::String(context_id.clone()));
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
/// Phase 1 stub: возвращает детерминированный script-id без реального хранения.
fn script_add_preload(id: i64, _params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    let script_id = state.next_id(0xaaaa);
    let mut result = BTreeMap::new();
    result.insert("script".into(), JsonValue::String(script_id));
    DispatchResult::single(make_success(id, JsonValue::Object(result)))
}

/// `script.removePreloadScript` — удалить preload script (BiDi §10.2.2).
///
/// Phase 1 stub: ACK без реального удаления.
fn script_remove_preload(id: i64, _params: &JsonValue, _state: &mut BidiState) -> DispatchResult {
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
        let result = dispatch(
            r#"{"id":4,"method":"script.removePreloadScript","params":{"script":"stub-id"}}"#,
            &mut state,
        );
        assert!(result.frames[0].contains("success"), "got: {}", result.frames[0]);
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
}
