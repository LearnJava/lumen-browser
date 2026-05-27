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
use lumen_storage::history::History;
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

    /// Записать визит в History и автоматически индексировать текст в FTS.
    /// Это синхронная операция: сначала вызывается `history.record_visit()`,
    /// затем получается запись с ID, и она индексируется в FTS.
    ///
    /// `text` — extracted text из содержимого страницы (readability-результат).
    /// Может быть пустой строкой, если текст ещё не был обработан (индекс всё равно создаётся).
    pub fn record_visit_with_text(
        &self,
        history: &History,
        url: &str,
        title: &str,
        text: &str,
        visit_date: i64,
    ) -> Result<()> {
        // 1. Записываем визит в базовую таблицу.
        history.record_visit(url, title, visit_date)?;

        // 2. Получаем ID записи (может быть новой или обновлённой).
        if let Some(entry) = history.get(url)? {
            // 3. Индексируем в FTS.
            self.fts.index(entry.id, url, title, text)?;
        }
        Ok(())
    }

    /// Удалить запись из History и автоматически удалить из FTS.
    /// Сначала получаем ID по URL, затем удаляем, затем удаляем из FTS.
    pub fn delete_with_fts(&self, history: &History, url: &str) -> Result<()> {
        // 1. Получаем ID удаляемой записи до удаления.
        if let Some(entry) = history.get(url)? {
            // 2. Удаляем из базовой таблицы.
            history.delete(url)?;
            // 3. Удаляем из FTS по ID.
            self.fts.unindex(entry.id)?;
        } else {
            // Если записи нет в History, просто удалим из FTS (если там есть).
            // В обычном случае это не должно произойти, но на случай несинхронизации.
            history.delete(url)?;
        }
        Ok(())
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

    #[test]
    fn record_visit_with_text_integration() {
        use lumen_storage::history::History;

        let hwf = make();
        let hist = History::open_in_memory().unwrap();

        // Записываем визит с текстом.
        hwf.record_visit_with_text(
            &hist,
            "https://example.com/",
            "Example Site",
            "This is example content about Rust",
            100,
        )
        .unwrap();

        // Проверяем, что запись создана в History.
        let entry = hist.get("https://example.com/").unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.url, "https://example.com/");
        assert_eq!(entry.visit_count, 1);

        // Проверяем, что текст индексирован в FTS.
        let hits = hwf.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rowid, entry.id);
    }

    #[test]
    fn delete_with_fts_integration() {
        use lumen_storage::history::History;

        let hwf = make();
        let hist = History::open_in_memory().unwrap();

        // Добавляем запись.
        hwf.record_visit_with_text(
            &hist,
            "https://example.com/",
            "Example",
            "content",
            100,
        )
        .unwrap();

        // Проверяем, что запись есть в FTS.
        assert_eq!(hwf.search("content", 10).unwrap().len(), 1);

        // Удаляем с синхронизацией.
        hwf.delete_with_fts(&hist, "https://example.com/").unwrap();

        // Проверяем, что удалена из History.
        assert!(hist.get("https://example.com/").unwrap().is_none());

        // Проверяем, что удалена из FTS.
        assert!(hwf.search("content", 10).unwrap().is_empty());
    }

    #[test]
    fn record_visit_with_text_update_reindexes() {
        use lumen_storage::history::History;

        let hwf = make();
        let hist = History::open_in_memory().unwrap();

        // Первый визит.
        hwf.record_visit_with_text(
            &hist,
            "https://example.com/",
            "Old Title",
            "old content",
            100,
        )
        .unwrap();

        // Второй визит (обновляет).
        hwf.record_visit_with_text(
            &hist,
            "https://example.com/",
            "New Title",
            "new content about Rust",
            200,
        )
        .unwrap();

        // Проверяем, что visit_count = 2 в History.
        let entry = hist.get("https://example.com/").unwrap().unwrap();
        assert_eq!(entry.visit_count, 2);
        assert_eq!(entry.title, "New Title");

        // Проверяем, что поиск находит обновлённый текст.
        let hits = hwf.search("rust", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "New Title");

        // Проверяем, что старый текст больше не найдётся.
        let old_hits = hwf.search("old", 10).unwrap();
        assert!(old_hits.is_empty());
    }
}
