//! Точки расширения: trait-ы с возможностью разных реализаций.
//!
//! Каждый trait — это место, куда можно подложить альтернативу (другой бэкенд,
//! mock для тестов, плагин-обёртка). Реализации живут в своих крейтах
//! (например, NetworkTransport — в lumen-network).
//!
//! Trait-ы определены здесь централизованно, чтобы граф зависимостей не
//! раздувался: потребитель зависит только от lumen-core и выбранной
//! реализации, а не от всех альтернатив.

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
pub trait FilterListSource: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_rules(&self) -> Result<String>;
}

/// Определение кодировки HTML-документа. Для кириллицы критично уметь
/// детектировать Windows-1251 и KOI8-R (см. §10.1).
pub trait EncodingDetector: Send + Sync {
    /// Возвращает имя кодировки (`"utf-8"`, `"windows-1251"`, …) или None,
    /// если уверенности недостаточно.
    fn detect(&self, bytes: &[u8], content_type_hint: Option<&str>) -> Option<&'static str>;
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
// - JsRuntime         — исполнение JavaScript. Реализации: QuickJS (v0.5),
//                       V8 (v1.0+).
//
// Остальные точки расширения без выбранной зависимости — пишем свои
// реализации сразу:
//
// - FontProvider      — поиск шрифтов с поддержкой кириллицы. Phase 1.
// - HyphenationEngine — переносы слов для CSS hyphens. Phase 2.
// - DnsResolver       — DNS, включая DoH/DoT. Phase 1.
// - Hasher            — единый интерфейс хэшей (для CSP, SRI). Phase 1.
