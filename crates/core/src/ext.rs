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
use crate::url::Url;

/// Сетевой транспорт. Подменяется на mock для тестов или на альтернативный стек.
pub trait NetworkTransport: Send + Sync {
    fn fetch(&self, url: &Url) -> Result<Vec<u8>>;
}

/// Хранилище ключ/значение для cookies, истории, кэша.
pub trait StorageBackend: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()>;
    fn delete(&mut self, key: &str) -> Result<()>;
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

// Точки расширения, спроектированные, но без интерфейса до Phase 1+:
//
// - JsRuntime         — мост к JS-движку (QuickJS / V8). Phase 1.
// - RenderBackend     — растеризация (tiny-skia / wgpu). Phase 0–1.
// - FontProvider      — поиск шрифтов с поддержкой кириллицы. Phase 1.
// - HyphenationEngine — переносы слов для CSS hyphens. Phase 2.
