//! HTML-парсер для Lumen.
//!
//! Phase 0 — минимальный токенизатор (`tokenizer`) + lenient tree builder
//! (`tree_builder`). Удобный вход — функция [`parse`]: строка → [`lumen_dom::Document`].
//!
//! Что поддерживается: открывающие/закрывающие/самозакрывающиеся теги,
//! атрибуты (quoted/unquoted), комментарии, базовые character references,
//! void-элементы, lenient end-tag matching.
//!
//! Что не поддерживается (отложено до Phase 1+): DOCTYPE-разбор, CDATA,
//! raw-text script/style states, insertion modes (in_table, in_select),
//! полный набор named entities, foster parent reparenting.

pub mod tokenizer;
pub mod tree_builder;

pub use tokenizer::{Token, Tokenizer};
pub use tree_builder::parse;
