//! Knowledge layer для Lumen — §12 плана.
//!
//! Phase 0-1 покрывает §12.1 «Полнотекстовый поиск по истории»: FTS5-
//! таблица над `(url, title, text)` поверх SQLite (exception #5);
//! ранжирование через встроенный bm25(); custom-tokenizer для
//! ё↔е equivalence и русского Porter-stemmer — отдельная задача в
//! Phase 2 (FTS5 supports external tokenizers через C-callback, нам
//! пока хватает дефолтного unicode61).
//!
//! Этот модуль — только поисковый индекс. Сама история (URL, dates,
//! favicons, visit_count) живёт в `lumen-storage::history::History` —
//! здесь только FTS5-зеркало текстового содержимого для быстрого
//! omnibox-поиска. Связь — через `rowid`, который равен `History.id`.
//!
//! Phase 1+ задачи: §12.2 аннотации/заметки (своя FTS5-таблица),
//! §12.3 read-later (snapshot HTML + текст), §12.4 поиск по открытым
//! вкладкам (live-индекс без disk-persistence). Все три предполагают
//! отдельные модули в этом крейте, но контракт `KnowledgeStore` будет
//! общим.

pub mod fts;
pub mod history;
pub mod notes;
pub mod read_later;

pub use fts::{HistoryFts, SearchHit};
pub use history::HistoryWithFts;
pub use notes::{Note, NoteSearchHit, Notes};
pub use read_later::{ReadLater, ReadLaterEntry, ReadLaterSearchHit, ReadStatus};
