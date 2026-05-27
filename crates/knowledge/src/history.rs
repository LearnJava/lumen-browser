//! История браузера с FTS-интеграцией.
//!
//! Этот модуль координирует две таблицы:
//! 1. `lumen-storage::history::History` — базовая таблица (URL, title, visit_date, visit_count, favicon)
//! 2. `HistoryFts` здесь — FTS5-индекс для быстрого поиска по text-содержимому
//!
//! `rowid` в `history_fts` совпадает с `History.id` — это позволяет
//! эффективно джойнить при поиске (join history ON history.id = history_fts.rowid).
//!
//! Phase 0 API:
//! - `HistoryWithFts` — обёртка над `History`, которая при `record_visit()`
//!   автоматически индексирует текст в `HistoryFts`.
//! - `search_history()` — полнотекстовый поиск с ранжированием.

use crate::fts::{HistoryFts, SearchHit};
use lumen_core::Result;
use std::path::Path;

/// История с интегрированным FTS-индексом. Оборачивает
/// `lumen-storage::history::History` и синхронизирует индекс.
///
/// **Интеграция:** Для использования в реальном браузере P3 должен:
/// 1. После `History::record_visit()` вызвать `HistoryWithFts::index_text(rowid, url, title, text)`
/// 2. После `History::delete()` вызвать `HistoryWithFts::unindex(rowid)`
///
/// На Phase 1+ можно перейти на trigger-синхронизацию или расширить API.
pub struct HistoryWithFts {
    /// FTS5-индекс над (url, title, text).
    pub fts: HistoryFts,
}

impl HistoryWithFts {
    /// Открыть или создать FTS-индекс истории. Обычно открывается
    /// в тот же файл, что и основная история в `lumen-storage`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let fts = HistoryFts::open(path)?;
        Ok(Self { fts })
    }

    /// Открыть in-memory FTS-индекс (для тестов).
    pub fn open_in_memory() -> Result<Self> {
        let fts = HistoryFts::open_in_memory()?;
        Ok(Self { fts })
    }

    /// Индексировать запись истории в FTS. Обычно вызывается после
    /// `History::record_visit()`. `rowid` должна совпадать с `History.id`.
    ///
    /// `text` — extracted text из содержимого страницы (readability-результат).
    /// Может быть пустой строкой, если текст ещё не был обработан.
    pub fn index_text(&self, rowid: i64, url: &str, title: &str, text: &str) -> Result<()> {
        self.fts.index(rowid, url, title, text)
    }

    /// Удалить запись из FTS-индекса. Обычно вызывается после
    /// `History::delete(url)` (переда `rowid` из записи).
    pub fn unindex(&self, rowid: i64) -> Result<()> {
        self.fts.unindex(rowid)
    }

    /// Полнотекстовый поиск по истории. Возвращает совпадения,
    /// отсортированные по BM25-релевантности.
    ///
    /// **Контракт:** Поиск индексирует `(url, title, text)` тройку.
    /// Простой запрос вроде "rust async" работает как implicit AND.
    /// FTS5-синтаксис поддерживается: `"foo bar"` (phrase), `foo OR bar`,
    /// `^prefix`, и т.д.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
        self.fts.search(query, limit)
    }

    /// Очистить весь FTS-индекс. Обычно вызывается при
    /// `History::clear()` или вручную для сброса поиска.
    pub fn clear(&self) -> Result<()> {
        self.fts.clear()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> HistoryWithFts {
        HistoryWithFts::open_in_memory().unwrap()
    }

    #[test]
    fn index_and_search_basic() {
        let h = make();
        // Имитируем: History::record_visit() добавила запись с id=1.
        h.index_text(1, "https://rust-lang.org/", "Rust", "Rust is a systems programming language with zero-cost abstractions")
            .unwrap();
        let hits = h.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, 1);
        assert_eq!(hits[0].url, "https://rust-lang.org/");
    }

    #[test]
    fn search_finds_in_title() {
        let h = make();
        h.index_text(1, "https://example.com/", "Rust Programming Guide", "some content")
            .unwrap();
        let hits = h.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_finds_in_text() {
        let h = make();
        h.index_text(1, "https://example.com/", "Article", "This is about Rust's ownership system")
            .unwrap();
        let hits = h.search("ownership", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn search_implicit_and() {
        let h = make();
        h.index_text(1, "https://a/", "t", "Rust async programming")
            .unwrap();
        h.index_text(2, "https://b/", "t", "Python async programming")
            .unwrap();
        // "rust async" должна найти только первую.
        let hits = h.search("rust async", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, 1);
    }

    #[test]
    fn unindex_removes_from_search() {
        let h = make();
        h.index_text(1, "https://example.com/", "t", "content here")
            .unwrap();
        assert_eq!(h.search("content", 10).unwrap().len(), 1);
        h.unindex(1).unwrap();
        assert!(h.search("content", 10).unwrap().is_empty());
    }

    #[test]
    fn clear_wipes_index() {
        let h = make();
        h.index_text(1, "https://a/", "t", "x").unwrap();
        h.index_text(2, "https://b/", "t", "y").unwrap();
        h.clear().unwrap();
        assert!(h.search("x", 10).unwrap().is_empty());
        assert!(h.search("y", 10).unwrap().is_empty());
    }

    #[test]
    fn cyrillic_indexing() {
        let h = make();
        h.index_text(
            1,
            "https://пример.рф/",
            "Статья о Rust",
            "Rust — это язык программирования с отличной системой владения",
        )
        .unwrap();
        let hits = h.search("программирования", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
