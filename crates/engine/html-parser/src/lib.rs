//! HTML-парсер для Lumen.
//!
//! Phase 0 — минимальный токенизатор (`tokenizer`) + lenient tree builder
//! (`tree_builder`). Удобный вход — функция [`parse`]: строка → [`lumen_dom::Document`].
//!
//! Что поддерживается: открывающие/закрывающие/самозакрывающиеся теги,
//! атрибуты (quoted/unquoted), комментарии, базовые character references,
//! void-элементы, lenient end-tag matching.
//!
//! Что не поддерживается (отложено до Phase 1+): CDATA, insertion modes
//! (in_table, in_select), полный набор named entities (~2125 имён —
//! у нас 250+ самых частых), foster parent reparenting.

mod entities;
pub mod quirks_mode;
pub mod tokenizer;
pub mod tree_builder;

pub use quirks_mode::detect_document_mode;
pub use tokenizer::{Token, Tokenizer};
pub use tree_builder::parse;
