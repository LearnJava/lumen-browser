//! Точки расширения: trait-ы с возможностью разных реализаций.
//!
//! Каждый trait — это место, куда можно подложить альтернативу (другой бэкенд,
//! mock для тестов, плагин-обёртка). Реализации живут в своих крейтах
//! (например, NetworkTransport — в lumen-network).
//!
//! Trait-ы определены здесь централизованно, чтобы граф зависимостей не
//! раздувался: потребитель зависит только от lumen-core и выбранной
//! реализации, а не от всех альтернатив.

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::Result;
use crate::event::Event;
use crate::url::Url;

/// Сетевой транспорт. Подменяется на mock для тестов или на альтернативный стек.
pub trait NetworkTransport: Send + Sync {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>>;
}

/// Приёмник событий из подсистем (network, навигация, вкладки).
///
/// Реализует принцип №4 «каждый исходящий байт виден»: транспорты эмитят
/// `Event::RequestStarted` / `RequestCompleted` / `RequestBlocked`, а
/// наблюдатель (shell, network-log UI, тесты, плагины) получает их через
/// единый интерфейс. Реализация шины (EventBus) появится позже, когда
/// потребителей станет больше одного; пока — single sink, передаваемый явно
/// в подсистему при конструировании.
///
/// `&self` без `&mut`: типичная реализация — `Mutex<Vec<Event>>` или channel,
/// и каждый `emit` атомарен. `Send + Sync` — sink можно делить между потоками
/// (фоновая загрузка favicon + main thread).
///
/// Принимаем `&Event` (а не `Event` по значению): caller обычно не нуждается
/// в Event после emit, но и платить за clone там, где sink его не сохраняет
/// (например, счётчик), не должен.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &Event);
}

/// EventSink, который молча игнорирует все события. Дефолт для подсистем,
/// у которых наблюдатель не подключён (тесты, headless-режимы). Применять
/// через `Arc::new(NoopEventSink)`, чтобы избавить hot-path от `Option`-веток.
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: &Event) {}
}

/// Хранилище ключ/значение для cookies, истории, кэша.
///
/// Все операции принимают `origin` и `top_level_site` для партиционирования
/// данных по источнику (cookie isolation, storage partitioning). `None` означает
/// глобальный профильный namespace (история, настройки).
pub trait StorageBackend: Send + Sync {
    fn get(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<u8>>>;

    fn put(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
        value: &[u8],
    ) -> Result<()>;

    fn delete(
        &mut self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
        key: &str,
    ) -> Result<()>;

    /// Перечислить все ключи в данном (origin, top_level_site) partition.
    fn list_keys(
        &self,
        origin: Option<&str>,
        top_level_site: Option<&str>,
    ) -> Result<Vec<String>>;
}

/// Поисковая система для omnibox.
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    fn query_url(&self, query: &str) -> Url;
}

/// Источник списка фильтров рекламы / трекеров.
///
/// Отвечает за подгрузку текста правил (EasyList, uBlock-формат). Применение
/// этих правил к конкретному URL — задача [`RequestFilter`]; разделение
/// сделано намеренно, чтобы загрузчик и matcher жили в разных слоях
/// (потребитель `HttpClient` зависит только от `RequestFilter`).
pub trait FilterListSource: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_rules(&self) -> Result<String>;
}

/// Решение «блокировать ли исходящий запрос». Реализация смотрит URL и
/// возвращает `None` для разрешённых, `Some(reason)` для блокируемых.
///
/// `reason` попадает в [`Event::RequestBlocked`](crate::event::Event)
/// и в текст возвращаемой ошибки — это пользовательская строка для UI
/// (network log: «✗ <url> (tracker)»), не машинно-читаемый код.
///
/// Отделено от [`FilterListSource`] намеренно: типичная полная цепочка —
/// `FilterListSource` (текст правил) → парсер/индекс правил → `RequestFilter`
/// (per-URL решение). `HttpClient` зависит только от `RequestFilter` и
/// ничего не знает о формате правил.
pub trait RequestFilter: Send + Sync {
    fn should_block(&self, url: &Url) -> Option<String>;
}

/// DNS-резолвер: hostname → список IP-адресов (с портом, готовых к connect).
///
/// Trait-точка вместо прямого `(host, port).to_socket_addrs()` нужна по двум
/// причинам: (1) тестируемость — mock-resolver возвращает loopback-адреса
/// без реального DNS-вызова; (2) подмена бэкенда — поверх системного
/// resolver-а в Phase 2+ появятся `CachedDnsResolver` (использует
/// `lumen-storage::DnsCache` для TTL-кеша), `DohResolver` (DNS-over-HTTPS
/// через `lumen-network::HttpClient`), `DotResolver` (DNS-over-TLS) — все
/// под одной trait-сигнатурой.
///
/// Принципы:
/// - `port` пробрасывается в SocketAddr-ы (не отдельно), чтобы вызывающий
///   слой (`HttpClient`) мог сразу `TcpStream::connect_timeout(&addr, ...)`
///   без склейки;
/// - возврат `Vec<SocketAddr>`, а не одиночный — DNS round-robin (`A` /
///   `AAAA` могут отдать несколько записей) разрешается `HttpClient` сам
///   (try-each до первого успешного connect);
/// - пустой `Vec` = NXDOMAIN / ошибка resolve — реализация может либо
///   вернуть `Err(...)`, либо пустой список; потребители трактуют оба
///   варианта одинаково («не смогли получить адрес»).
pub trait DnsResolver: Send + Sync {
    /// Разрешить hostname в список SocketAddr с указанным port.
    /// Hostname может быть Punycode-формой (`xn--…`) — резолверу всё равно.
    fn resolve(&self, hostname: &str, port: u16) -> Result<Vec<SocketAddr>>;
}

/// HSTS-политика: должны ли HTTP-запросы к данному host принудительно
/// upgrade-иться на HTTPS, и как сохранять policy из ответа.
///
/// RFC 6797 (HTTP Strict Transport Security). Trait-точка нужна, чтобы
/// `HttpClient` (`lumen-network`) не зависел напрямую от `HstsStore`
/// (`lumen-storage`) — реальный SQLite-backed store живёт в storage,
/// а network знает только про этот контракт. Тесты могут подложить
/// in-memory mock без SQLite. Аналогично DnsResolver / EncodingDetector.
///
/// **Семантика без `Result`** — fail-open: ошибка БД (поломанный диск,
/// заблокированная mutex) не должна валить fetch. `is_https_only` возвращает
/// `false` при любой проблеме (не upgrade-им сомнительный host), а
/// `record_sts` тихо проглатывает ошибки persistence (best-effort —
/// при повторе сервер всё равно пришлёт STS header). Это та же логика,
/// что у браузеров: HSTS — soft policy, не блокирующая.
///
/// Реализация в `lumen-storage::hsts::HstsStore`. RFC-семантика
/// `max-age=0` (снять HSTS) обрабатывается на стороне реализации —
/// trait просто пробрасывает значение.
pub trait HstsEnforcement: Send + Sync {
    /// Должен ли HTTP-запрос к `host` (ASCII / Punycode) быть переписан в
    /// HTTPS? Учитывает entries с `includeSubDomains` (longest-suffix-match
    /// по родительским доменам). `now_unix` — текущее время для фильтрации
    /// истёкших entries.
    fn is_https_only(&self, host: &str, now_unix: i64) -> bool;

    /// Записать HSTS policy из заголовка `Strict-Transport-Security`.
    /// `host` — ASCII / Punycode. Реализация обязана трактовать
    /// `max_age = 0` как «снять HSTS для этого host» (RFC 6797 §6.1.1).
    fn record_sts(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    );
}

/// Определение кодировки HTML-документа. Для кириллицы критично уметь
/// детектировать Windows-1251 и KOI8-R (см. §10.1).
pub trait EncodingDetector: Send + Sync {
    /// Возвращает имя кодировки (`"utf-8"`, `"windows-1251"`, …) или None,
    /// если уверенности недостаточно.
    fn detect(&self, bytes: &[u8], content_type_hint: Option<&str>) -> Option<&'static str>;
}

/// Источник системных шрифтов. Реализация — в `lumen-font::system_fonts`.
///
/// CSS-каскад даёт `font-family: ["Roboto", "Arial", sans-serif]` — приоритетный
/// список; rasterizer должен решить, какой реальный файл `.ttf` загрузить.
/// `FontProvider` отделяет «как найти шрифт на этой ОС» от «что с ним делать
/// дальше» (распарсить, растеризовать, добавить в атлас).
///
/// Возвращает `Vec<PathBuf>`: для одного семейства часто есть несколько
/// face-ов (Regular / Bold / Italic / Bold Italic / разные weight-ы). Конкретный
/// выбор по `font-style` / `font-weight` — задача потребителя; провайдер только
/// перечисляет кандидатов.
///
/// Имена сравниваются ASCII-case-insensitive: CSS `"Times New Roman"` должен
/// найти файл, у которого family name записан как `"Times New Roman"` или
/// `"TIMES NEW ROMAN"` — спецификация (CSS Fonts L4 §4.3) явно требует
/// case-insensitive matching.
///
/// `&[&str]` — codepoint coverage lookup отложен (для эмодзи / CJK fallback);
/// добавим, когда пойдёт реальная страница. Сейчас провайдер — только индекс
/// по имени.
pub trait FontProvider: Send + Sync {
    /// Найти все пути к файлам шрифтов, объявленным под данным family name.
    /// Пустой Vec — семейство не найдено.
    fn lookup_family(&self, family: &str) -> Vec<PathBuf>;

    /// Имена всех известных семейств. Для отладки и тестов; в production
    /// потребители используют `lookup_family`.
    fn list_families(&self) -> Vec<String>;
}

/// JavaScript runtime — исполнение JS-кода (HTML inline scripts, `eval`,
/// custom elements, и т.д.). Trait абстрагирует выбор движка: первая
/// реализация — `rquickjs` поверх QuickJS (exception #4 в §5),
/// v1.0+ — `rusty_v8` поверх V8. Свой JS-движок не пишем.
///
/// Phase 0: trait определён, NullJsRuntime (всегда возвращает ошибку)
/// — placeholder. Реальный QuickJS-runtime появится в Phase 2-3.
///
/// Возвращаемые `JsValue`-ы — простые JSON-совместимые типы. Полный
/// JS-объект (с прототипами, методами, замыканиями) не пробрасывается
/// через границу trait-а — это намеренное ограничение, чтобы не привязать
/// API к конкретному движку.
pub trait JsRuntime: Send + Sync {
    /// Выполнить script-text и вернуть результат последнего выражения.
    fn eval(&self, script: &str) -> JsResult<JsValue>;

    /// Записать глобальную переменную в текущий runtime context.
    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()>;

    /// Прочитать значение глобальной переменной.
    fn get_global(&self, name: &str) -> JsResult<JsValue>;

    /// Вызвать функцию `name(args)` в global scope и вернуть результат.
    /// Функция должна быть определена через `eval` или `set_global`.
    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue>;

    /// Имя движка для debug-вывода. Реализация: `"quickjs"`, `"v8"`,
    /// `"null"`.
    fn engine_name(&self) -> &'static str;
}

/// Простые JSON-совместимые типы для передачи через trait-границу.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsValue>),
    /// Поля сортированы по ключу, чтобы сравнения детерминированы.
    Object(Vec<(String, JsValue)>),
}

impl JsValue {
    /// Хелпер: построить object из key-value пар.
    pub fn object<I: IntoIterator<Item = (String, JsValue)>>(entries: I) -> Self {
        let mut v: Vec<(String, JsValue)> = entries.into_iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        Self::Object(v)
    }
}

/// Ошибка исполнения JavaScript: либо syntax error (parse), либо runtime
/// exception (`throw`), либо неподдержанная операция (Null runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsError {
    /// Lexical / parser error в скрипте.
    Parse(String),
    /// Runtime error — uncaught exception в JS.
    Runtime(String),
    /// Операция не реализована в текущем runtime (например, Null).
    NotImplemented,
}

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "JS parse error: {m}"),
            Self::Runtime(m) => write!(f, "JS runtime error: {m}"),
            Self::NotImplemented => write!(f, "JS not implemented"),
        }
    }
}

impl std::error::Error for JsError {}

pub type JsResult<T> = std::result::Result<T, JsError>;

/// Null implementation — всегда возвращает `JsError::NotImplemented`.
/// Используется как placeholder в shell, пока не подключён реальный движок.
#[derive(Debug, Default)]
pub struct NullJsRuntime;

impl JsRuntime for NullJsRuntime {
    fn eval(&self, _: &str) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn set_global(&self, _: &str, _: JsValue) -> JsResult<()> {
        Err(JsError::NotImplemented)
    }
    fn get_global(&self, _: &str) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn call_function(&self, _: &str, _: &[JsValue]) -> JsResult<JsValue> {
        Err(JsError::NotImplemented)
    }
    fn engine_name(&self) -> &'static str {
        "null"
    }
}

// Точки расширения, спроектированные, но без интерфейса до Phase 1+.
//
// Trait-ы для четырёх «разрешённых exceptions» из §5 (внешние зависимости,
// которые мы используем): каждая зависимость прячется за свой trait,
// чтобы при желании можно было swap-нуть на свою реализацию.
//
// - WindowingBackend  — OS event loop + окна. Первая реализация: winit.
// - RenderBackend     — GPU-абстракция. Первая реализация: wgpu.
// - TlsBackend        — TLS / X.509 / симметричная криптография. Первая
//                       реализация: rustls. Своя — security antipattern;
//                       абстракция нужна только для swap на системный TLS
//                       (SChannel / Network.framework).
// - JsRuntime         — определён выше; реализации: QuickJS (v0.5),
//                       V8 (v1.0+).
//
// Остальные точки расширения без выбранной зависимости — пишем свои
// реализации сразу:
//
// - HyphenationEngine — переносы слов для CSS hyphens. Phase 2.
// - DnsResolver       — определён выше; реализации: SystemDnsResolver
//                       (через `(host, port).to_socket_addrs()`),
//                       CachedDnsResolver (обёртка с `lumen-storage::DnsCache`).
//                       DoH/DoT-резолверы — Phase 2+.
// - Hasher            — единый интерфейс хэшей (для CSP, SRI). Phase 1.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_runtime_returns_not_implemented_for_eval() {
        let rt = NullJsRuntime;
        assert!(matches!(rt.eval("1 + 2"), Err(JsError::NotImplemented)));
    }

    #[test]
    fn null_runtime_returns_not_implemented_for_set_global() {
        let rt = NullJsRuntime;
        assert!(matches!(
            rt.set_global("x", JsValue::Number(1.0)),
            Err(JsError::NotImplemented)
        ));
    }

    #[test]
    fn null_runtime_engine_name() {
        assert_eq!(NullJsRuntime.engine_name(), "null");
    }

    #[test]
    fn js_value_object_sorted_by_key() {
        let v = JsValue::object(vec![
            ("b".into(), JsValue::Number(2.0)),
            ("a".into(), JsValue::Number(1.0)),
            ("c".into(), JsValue::Number(3.0)),
        ]);
        match v {
            JsValue::Object(entries) => {
                let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(keys, vec!["a", "b", "c"]);
            }
            _ => panic!("expected Object"),
        }
    }

    #[test]
    fn js_value_equality() {
        assert_eq!(JsValue::Null, JsValue::Null);
        assert_eq!(JsValue::Number(1.5), JsValue::Number(1.5));
        assert_ne!(JsValue::Bool(true), JsValue::Bool(false));
        assert_eq!(
            JsValue::Array(vec![JsValue::Number(1.0), JsValue::String("a".into())]),
            JsValue::Array(vec![JsValue::Number(1.0), JsValue::String("a".into())]),
        );
    }

    #[test]
    fn js_error_display() {
        assert_eq!(
            format!("{}", JsError::Parse("unexpected }".into())),
            "JS parse error: unexpected }"
        );
        assert_eq!(format!("{}", JsError::NotImplemented), "JS not implemented");
    }

    #[test]
    fn null_runtime_is_send_sync() {
        fn is_send_sync<T: Send + Sync>() {}
        is_send_sync::<NullJsRuntime>();
        // dyn check.
        fn check_dyn(_r: &dyn JsRuntime) {}
        let rt = NullJsRuntime;
        check_dyn(&rt);
    }
}
