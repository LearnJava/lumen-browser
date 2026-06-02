//! WebDriver BiDi — минимальный диспетчер команд (Phase 0 stub, §6.11).
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
//! Реализованные команды (stub):
//! - `session.status` — всегда `ready:true`, пока сессия не создана.
//! - `session.new` — создаёт сессию + дефолтный browsing context; эмитит
//!   событие `browsingContext.created`.
//! - `session.subscribe` / `session.unsubscribe` — ACK (подписки не хранятся).
//! - `session.end` — ACK + закрытие соединения.
//! - `browsingContext.getTree` — дерево из единственного дефолтного контекста.
//!
//! Всё остальное → ошибка `unknown command`.

use std::collections::BTreeMap;

use lumen_core::json::{parse as parse_json, JsonValue};

/// Состояние одного BiDi-соединения.
///
/// Хранит идентификаторы созданной сессии и дефолтного browsing context.
/// Не разделяется между соединениями — каждое соединение изолировано.
#[derive(Default)]
pub struct BidiState {
    /// ID активной сессии (`None`, пока не вызван `session.new`).
    session_id: Option<String>,
    /// ID дефолтного browsing context (создаётся вместе с сессией).
    context_id: Option<String>,
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
    /// клиенты BiDi трактуют context/session id как непрозрачные строки,
    /// а детерминизм нужен для unit-тестов.
    fn next_id(&mut self, tag: u16) -> String {
        self.counter += 1;
        let n = self.counter;
        format!("{:08x}-{tag:04x}-4000-8000-{n:012x}", n as u32)
    }
}

/// Результат обработки одной команды.
pub struct DispatchResult {
    /// Фреймы для последовательной отправки клиенту (ответ + события).
    pub frames: Vec<String>,
    /// Закрыть ли соединение после отправки фреймов (`session.end`).
    pub close: bool,
}

/// Обработать одно BiDi-сообщение, вернуть фреймы для отправки клиенту.
pub fn dispatch(message: &str, state: &mut BidiState) -> DispatchResult {
    let val = match parse_json(message) {
        Ok(v) => v,
        Err(e) => {
            return DispatchResult {
                frames: vec![make_error(None, "invalid argument", &format!("parse error: {e}"))],
                close: false,
            };
        }
    };

    let id = val.get("id").and_then(|v| v.as_number()).map(|n| n as i64);
    let method = val.get("method").and_then(|v| v.as_str());
    let params = val.get("params").cloned().unwrap_or(JsonValue::Null);

    let Some(id) = id else {
        return DispatchResult {
            frames: vec![make_error(None, "invalid argument", "missing or non-integer id")],
            close: false,
        };
    };
    let Some(method) = method else {
        return DispatchResult {
            frames: vec![make_error(Some(id), "invalid argument", "missing method")],
            close: false,
        };
    };

    match method {
        "session.status" => DispatchResult {
            frames: vec![make_success(id, session_status_result(state))],
            close: false,
        },
        "session.new" => session_new(id, &params, state),
        "session.subscribe" | "session.unsubscribe" => DispatchResult {
            frames: vec![make_success(id, JsonValue::Object(BTreeMap::new()))],
            close: false,
        },
        "session.end" => DispatchResult {
            frames: vec![make_success(id, JsonValue::Object(BTreeMap::new()))],
            close: true,
        },
        "browsingContext.getTree" => DispatchResult {
            frames: vec![make_success(id, browsing_context_tree(state))],
            close: false,
        },
        other => DispatchResult {
            frames: vec![make_error(Some(id), "unknown command", &format!("unknown command: {other}"))],
            close: false,
        },
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
/// Возвращает успешный ответ с `sessionId` + минимальными capabilities,
/// затем эмитит событие `browsingContext.created` для дефолтного контекста.
fn session_new(id: i64, _params: &JsonValue, state: &mut BidiState) -> DispatchResult {
    if state.session_id.is_some() {
        return DispatchResult {
            frames: vec![make_error(Some(id), "session not created", "session already exists")],
            close: false,
        };
    }

    let session_id = state.next_id(0x5e55);
    let context_id = state.next_id(0xc047);
    state.session_id = Some(session_id.clone());
    state.context_id = Some(context_id.clone());

    let mut result = BTreeMap::new();
    result.insert("sessionId".into(), JsonValue::String(session_id));
    result.insert("capabilities".into(), capabilities());

    let success = make_success(id, JsonValue::Object(result));
    let created = make_event(
        "browsingContext.created",
        browsing_context_info(&context_id),
    );

    DispatchResult { frames: vec![success, created], close: false }
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

/// `BrowsingContextInfo` для одного контекста (BiDi §browsingContext).
fn browsing_context_info(context_id: &str) -> JsonValue {
    let mut info = BTreeMap::new();
    info.insert("context".into(), JsonValue::String(context_id.into()));
    info.insert("url".into(), JsonValue::String("about:blank".into()));
    info.insert("children".into(), JsonValue::Null);
    info.insert("parent".into(), JsonValue::Null);
    info.insert("userContext".into(), JsonValue::String("default".into()));
    info.insert("originalOpener".into(), JsonValue::Null);
    JsonValue::Object(info)
}

/// `browsingContext.getTree` — `{"contexts":[BrowsingContextInfo...]}`.
///
/// Возвращает дефолтный контекст, если сессия создана; иначе пустой список.
fn browsing_context_tree(state: &BidiState) -> JsonValue {
    let contexts = match &state.context_id {
        Some(cid) => vec![browsing_context_info(cid)],
        None => vec![],
    };
    let mut obj = BTreeMap::new();
    obj.insert("contexts".into(), JsonValue::Array(contexts));
    JsonValue::Object(obj)
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
    fn session_new_returns_session_and_emits_created() {
        let mut state = BidiState::new();
        let r = dispatch(
            r#"{"id":2,"method":"session.new","params":{"capabilities":{}}}"#,
            &mut state,
        );
        // success + browsingContext.created event
        assert_eq!(r.frames.len(), 2);
        assert!(!r.close);

        let success = parse(&r.frames[0]);
        assert_eq!(success.get("type").and_then(|x| x.as_str()), Some("success"));
        assert_eq!(success.get("id").and_then(|x| x.as_number()), Some(2.0));
        let result = success.get("result").unwrap();
        assert!(result.get("sessionId").and_then(|x| x.as_str()).is_some());
        let caps = result.get("capabilities").unwrap();
        assert_eq!(caps.get("browserName").and_then(|x| x.as_str()), Some("Lumen"));

        let event = parse(&r.frames[1]);
        assert_eq!(event.get("type").and_then(|x| x.as_str()), Some("event"));
        assert_eq!(
            event.get("method").and_then(|x| x.as_str()),
            Some("browsingContext.created")
        );
        let info = event.get("params").unwrap();
        assert!(info.get("context").and_then(|x| x.as_str()).is_some());
        assert_eq!(info.get("url").and_then(|x| x.as_str()), Some("about:blank"));
        assert!(matches!(info.get("parent"), Some(JsonValue::Null)));
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
}
