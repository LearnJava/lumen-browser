//! WebDriver BiDi — диспетчер команд (Phase 1, §6.11, ADR-006).
//!
//! Разбирает BiDi-команду `{"id":N,"method":"module.command","params":{...}}`,
//! маршрутизирует на обработчик, возвращает один или несколько фреймов для
//! отправки клиенту (W3C WebDriver BiDi, Working Draft).
//!
//! Форматы сообщений (BiDi §3.4):
//! - Команда: `{"id":<js-uint>,"method":"<str>","params":<obj>}`
//! - Успех:   `{"type":"success","id":<js-uint>,"result":<obj>}`
//! - Ошибка:  `{"type":"error","id":<js-uint|null>,"error":"<code>","message":"<str>","stacktrace":""}`
//! - Событие: `{"type":"event","method":"<str>","params":<obj>}`
//!
//! Реализованные команды:
//! - `session.status` — `ready:true`, пока сессия не создана.
//! - `session.new` — создаёт сессию + дефолтный browsing context (без события,
//!   подписок ещё нет — порядок BiDi: `session.new` → `session.subscribe`).
//! - `session.subscribe` / `session.unsubscribe` — реально хранят набор подписок;
//!   события эмитятся только подписанным клиентам (точное имя метода или модуль).
//! - `session.end` — ACK + закрытие соединения.
//! - `browsingContext.create` — новый контекст (опц. `referenceContext` → parent);
//!   эмитит `browsingContext.created`, если подписан.
//! - `browsingContext.close` — удаляет контекст (и его потомков); эмитит
//!   `browsingContext.contextDestroyed`, если подписан.
//! - `browsingContext.navigate` — обновляет URL контекста, возвращает
//!   `{navigation, url}`; эмитит `browsingContext.load`, если подписан.
//! - `browsingContext.activate` — ACK (валидирует контекст).
//! - `browsingContext.getTree` — дерево всех контекстов (вложенность по parent).
//!
//! Live-wiring к реальному движку (фактическая навигация в `lumen-driver`,
//! `domContentLoaded`/response-body/cookie-события) — handoff P3, см. 8H.3.
//! Этот слой — чистая state-машина протокола: одно соединение = одно [`BidiState`].
//!
//! Всё остальное → ошибка `unknown command`.

use std::collections::{BTreeMap, BTreeSet};

use lumen_core::json::{parse as parse_json, JsonValue};

/// Один browsing context в рамках соединения.
struct BidiContext {
    /// Непрозрачный идентификатор контекста (BiDi `context`).
    id: String,
    /// Текущий URL контекста; `about:blank` до первой навигации.
    url: String,
    /// Родительский контекст (`Some` для вложенных, `None` для top-level).
    parent: Option<String>,
}

/// Состояние одного BiDi-соединения.
///
/// Хранит сессию, набор browsing context'ов и активные подписки на события.
/// Не разделяется между соединениями — каждое соединение изолировано.
#[derive(Default)]
pub struct BidiState {
    /// ID активной сессии (`None`, пока не вызван `session.new`).
    session_id: Option<String>,
    /// Все browsing context'ы соединения; первый создаётся вместе с сессией.
    contexts: Vec<BidiContext>,
    /// Активные подписки: имя события (`module.event`) или модуль (`module`).
    subscriptions: BTreeSet<String>,
    /// Монотонный счётчик для детерминированной генерации id-ов.
    counter: u64,
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
    fn next_id(&mut self, tag: u16) -> String {
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
}
